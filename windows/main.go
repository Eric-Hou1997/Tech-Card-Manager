package main

import (
	"archive/zip"
	"bytes"
	"context"
	"crypto/rand"
	"embed"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"log"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"
)

const appVersion = "4.1.0"

//go:embed web/index.html engine/windows-engine.ps1 engine/technical-specs-card.js assets/TCM_logo_letter_only.png assets/TCM_logo_tiny.png language_catalog.json
var assets embed.FS

type Settings struct {
	IntervalSeconds     int           `json:"interval_seconds"`
	AutoStart           bool          `json:"auto_start"`
	AutoStartConfigured bool          `json:"auto_start_configured"`
	SilentStart         bool          `json:"silent_start"`
	Language            string        `json:"language"`
	RootsConfigured     bool          `json:"roots_configured"`
	LibraryRoots        []LibraryRoot `json:"library_roots,omitempty"`
}

type LibraryRoot struct {
	Path    string `json:"path"`
	Name    string `json:"name,omitempty"`
	Kind    string `json:"kind,omitempty"`
	Source  string `json:"source"`
	Enabled bool   `json:"enabled"`
}

type LibraryInfo struct {
	Name        string `json:"name,omitempty"`
	Path        string `json:"path"`
	Kind        string `json:"kind,omitempty"`
	Online      bool   `json:"online"`
	State       string `json:"state,omitempty"`
	Evidence    string `json:"evidence,omitempty"`
	AccessError string `json:"access_error,omitempty"`
}

type JobState struct {
	Running              bool   `json:"running"`
	Action               string `json:"action,omitempty"`
	StartedAt            string `json:"started_at,omitempty"`
	EndedAt              string `json:"ended_at,omitempty"`
	ExitCode             int    `json:"exit_code,omitempty"`
	Message              string `json:"message,omitempty"`
	Log                  string `json:"log,omitempty"`
	BlocksExit           bool   `json:"blocks_exit,omitempty"`
	NeedsAdmin           bool   `json:"needs_admin,omitempty"`
	AlreadyAdmin         bool   `json:"already_admin,omitempty"`
	Language             string `json:"language,omitempty"`
	LanguagePackRevision int    `json:"language_pack_revision,omitempty"`
	LanguageCatalogHash  string `json:"language_catalog_hash,omitempty"`
}

type AgentCycleState struct {
	Running    bool   `json:"running"`
	StartedAt  string `json:"started_at,omitempty"`
	EndedAt    string `json:"ended_at,omitempty"`
	DurationMs int64  `json:"duration_ms,omitempty"`
	Error      string `json:"error,omitempty"`
}

type XmlErrorInfo struct {
	Path  string `json:"path"`
	Stamp string `json:"stamp,omitempty"`
	Error string `json:"error"`
}

type Status struct {
	AppVersion      string                 `json:"app_version"`
	Platform        string                 `json:"platform"`
	PlatformLabel   string                 `json:"platform_label"`
	Installed       bool                   `json:"installed"`
	AgentRunning    bool                   `json:"agent_running"`
	AutoStart       bool                   `json:"auto_start"`
	SilentStart     bool                   `json:"silent_start"`
	Language        string                 `json:"language"`
	Languages       []LanguageOption       `json:"languages"`
	AgentPID        int                    `json:"agent_pid,omitempty"`
	LastHeartbeat   string                 `json:"last_heartbeat,omitempty"`
	LastCycle       AgentCycleState        `json:"last_cycle"`
	IntervalSeconds int                    `json:"interval_seconds"`
	EngineReady     bool                   `json:"engine_ready"`
	Python          string                 `json:"python,omitempty"`
	Chrome          string                 `json:"chrome,omitempty"`
	IndexedTitles   int                    `json:"indexed_titles,omitempty"`
	NFOTotal        int                    `json:"nfo_total,omitempty"`
	CacheCount      int                    `json:"cache_count,omitempty"`
	XmlErrors       int                    `json:"xml_errors,omitempty"`
	XmlErrorDetails []XmlErrorInfo         `json:"xml_error_details,omitempty"`
	WebPatch        bool                   `json:"web_patch,omitempty"`
	WebVersion      string                 `json:"web_version,omitempty"`
	Libraries       []LibraryInfo          `json:"libraries,omitempty"`
	ConfiguredRoots []LibraryRoot          `json:"configured_roots,omitempty"`
	RootsConfigured bool                   `json:"roots_configured"`
	DiscoveredRoots []LibraryInfo          `json:"discovered_roots,omitempty"`
	Counts          map[string]int         `json:"counts,omitempty"`
	Job             JobState               `json:"job"`
	Capabilities    map[string]bool        `json:"capabilities"`
	Notes           []string               `json:"notes,omitempty"`
	Paths           map[string]string      `json:"paths,omitempty"`
	CleanupRemoved  []string               `json:"cleanup_removed,omitempty"`
	Service         ServiceSnapshot        `json:"service"`
	Extra           map[string]interface{} `json:"extra,omitempty"`
}

type actionRequest struct {
	Action string `json:"action"`
	IMDb   string `json:"imdb,omitempty"`
	Path   string `json:"path,omitempty"`
}

type jobManager struct {
	mu     sync.Mutex
	st     JobState
	cancel context.CancelFunc
	done   chan struct{}
}

type managedJobOptions struct {
	blocksExit   bool
	needsAdmin   bool
	alreadyAdmin bool
	message      string
	run          func(context.Context, io.Writer) error
	after        func(error)
}

var errMaintenanceExitTimeout = errors.New("管理员维护尚未结束，退出已暂停")

var jobs jobManager
var lastHeartbeatUnix atomic.Int64
var uiToken string

func main() {
	loginStartup := false
	if len(os.Args) > 1 {
		switch os.Args[1] {
		case "--agent":
			// The current product has no independent resident Agent. A stale
			// login entry may still invoke this switch during migration; fail
			// closed instead of silently recreating a background process.
			appendManagerLog("ignored deprecated --agent launch; open the visual Manager instead")
			return
		case "--headless-action":
			if len(os.Args) != 7 || os.Args[3] != "--elevated-request" || os.Args[5] != "--manager-pid" {
				os.Exit(2)
			}
			action := strings.TrimSpace(os.Args[2])
			requestID := strings.TrimSpace(os.Args[4])
			managerPID, pidErr := strconv.Atoi(strings.TrimSpace(os.Args[6]))
			if !isPrivilegedAction(action) || !validElevatedRequestID(requestID) || pidErr != nil || managerPID <= 0 {
				os.Exit(2)
			}
			if err := writeElevatedReady(requestID, action); err != nil {
				os.Exit(3)
			}
			permit, err := waitForElevatedPermit(requestID, action, managerPID, 30*time.Second)
			if err != nil {
				_ = writeElevatedResult(requestID, action, false, err.Error())
				os.Exit(3)
			}
			if !permit.Allow {
				message := strings.TrimSpace(permit.Message)
				if message == "" {
					message = "管理员维护操作未获主程序许可"
				}
				_ = writeElevatedResult(requestID, action, false, message)
				os.Exit(4)
			}
			if err := ensureAssets(); err != nil {
				_ = writeElevatedResult(requestID, action, false, err.Error())
				os.Exit(2)
			}
			err = performImmediateAction(action, "")
			if err != nil {
				_ = writeElevatedResult(requestID, action, false, err.Error())
				os.Exit(1)
			}
			if err := writeElevatedResult(requestID, action, true, "管理员维护操作已完成"); err != nil {
				os.Exit(3)
			}
			return
		case "--login-startup":
			loginStartup = true
		}
	}

	releaseInstance, primaryInstance, err := platformAcquireManagerInstance()
	if err != nil {
		appendManagerLog("single instance: " + err.Error())
		return
	}
	if !primaryInstance {
		return
	}
	defer releaseInstance()

	if err := ensurePortableWorkspace(); err != nil {
		appendManagerLog("portable workspace: " + err.Error())
		return
	}
	if err := ensureAssets(); err != nil {
		appendManagerLog("ensureAssets: " + err.Error())
	}
	services.initialize()
	runUI(loginStartup)
}

func derivedIndexesValid() bool {
	return derivedIndexesValidForSettings(loadSettings())
}

func derivedIndexesValidForSettings(settings Settings) bool {
	if !settings.RootsConfigured {
		return false
	}
	type publicIndex struct {
		Version     int                        `json:"version"`
		GeneratedAt string                     `json:"generatedAt"`
		Items       map[string]json.RawMessage `json:"items"`
		ItemTypes   map[string]string          `json:"itemTypes"`
	}
	type catalogIndex struct {
		Items []json.RawMessage `json:"items"`
	}
	type summaryIndex struct {
		GeneratedAt  string                     `json:"generatedAt"`
		LibraryRoots []string                   `json:"libraryRoots"`
		ScanStats    map[string]json.RawMessage `json:"scanStats"`
		Items        map[string]json.RawMessage `json:"items"`
	}
	read := func(path string, target interface{}) bool {
		b, err := os.ReadFile(path)
		return err == nil && json.Unmarshal(b, target) == nil
	}
	var data publicIndex
	var catalog catalogIndex
	var summary summaryIndex
	if !read(techDataFile(), &data) || !read(managerCatalogFile(), &catalog) || !read(indexSummaryFile(), &summary) {
		return false
	}
	if data.Version != 7 || data.Items == nil || data.ItemTypes == nil || catalog.Items == nil ||
		summary.Items == nil || summary.ScanStats == nil || data.GeneratedAt == "" || data.GeneratedAt != summary.GeneratedAt {
		return false
	}
	expected := make(map[string]bool)
	for _, root := range settings.LibraryRoots {
		if root.Enabled {
			expected[strings.ToLower(strings.TrimRight(filepath.Clean(root.Path), `\/`))] = true
		}
	}
	actual := make(map[string]bool)
	for _, root := range summary.LibraryRoots {
		root = strings.TrimSpace(root)
		if root != "" {
			actual[strings.ToLower(strings.TrimRight(filepath.Clean(root), `\/`))] = true
		}
	}
	if len(expected) == 0 || len(expected) != len(actual) {
		return false
	}
	for root := range expected {
		if !actual[root] {
			return false
		}
	}
	return true
}

func runUI(loginStartup bool) {
	uiToken = newSessionToken()
	mux := http.NewServeMux()
	mux.HandleFunc("/", serveIndex)
	mux.HandleFunc("/assets/TCM_logo_letter_only.png", serveAppIcon("assets/TCM_logo_letter_only.png"))
	mux.HandleFunc("/assets/TCM_logo_tiny.png", serveAppIcon("assets/TCM_logo_tiny.png"))
	mux.HandleFunc("/api/status", requireToken(handleStatus))
	mux.HandleFunc("/api/catalog", requireToken(handleCatalog))
	mux.HandleFunc("/api/action", requireToken(handleAction))
	mux.HandleFunc("/api/settings", requireToken(handleSettings))
	mux.HandleFunc("/api/job", requireToken(handleJob))
	mux.HandleFunc("/api/update", requireToken(handleCardUpdate))
	mux.HandleFunc("/api/languages", requireToken(handleLanguagePacks))
	mux.HandleFunc("/api/heartbeat", requireToken(func(w http.ResponseWriter, r *http.Request) {
		lastHeartbeatUnix.Store(time.Now().Unix())
		writeJSON(w, map[string]bool{"ok": true})
	}))

	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		appendManagerLog("listen: " + err.Error())
		return
	}
	srv := &http.Server{
		Handler:           mux,
		ReadHeaderTimeout: 3 * time.Second,
		ReadTimeout:       10 * time.Second,
		WriteTimeout:      30 * time.Second,
		IdleTimeout:       30 * time.Second,
		MaxHeaderBytes:    32 << 10,
	}
	url := "http://" + ln.Addr().String() + "/?token=" + uiToken
	lastHeartbeatUnix.Store(time.Now().Unix())

	go func() {
		if err := srv.Serve(ln); err != nil && !errors.Is(err, http.ErrServerClosed) {
			appendManagerLog("http: " + err.Error())
		}
	}()
	go ensureConfiguredLanguagePack()

	if err := openAppWindow(url); err != nil {
		appendManagerLog("open UI: " + err.Error())
		platformShowStartupError(err)
		_ = services.shutdown()
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		_ = srv.Shutdown(ctx)
		cancel()
		return
	}

	// The Windows GUI is an Edge --app window. A native tray host watches that
	// window: minimizing it hides the window from the taskbar, and the tray icon
	// can restore it. Keep the local server alive while the hidden HWND exists,
	// even if Edge throttles page timers and heartbeat delivery.
	traySignals := startWindowsTray(url)
	defer stopWindowsTray()
	if loginStartup && loadSettings().SilentStart {
		go platformHideManagerWindowToTray()
	}

	shutdown := func() {
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		_ = srv.Shutdown(ctx)
		cancel()
	}

	var exitPromptMu sync.Mutex
	exitPromptActive := false
	confirmExit := func(windowAlreadyClosed bool) bool {
		exitPromptMu.Lock()
		if exitPromptActive {
			exitPromptMu.Unlock()
			return false
		}
		exitPromptActive = true
		exitPromptMu.Unlock()
		defer func() {
			exitPromptMu.Lock()
			exitPromptActive = false
			exitPromptMu.Unlock()
		}()

		jobs.mu.Lock()
		jobRunning := jobs.st.Running
		maintenanceRunning := jobs.st.Running && jobs.st.BlocksExit
		jobs.mu.Unlock()
		if !platformConfirmExit(services.snapshot(), jobRunning, maintenanceRunning, windowAlreadyClosed) {
			if windowAlreadyClosed {
				if err := platformStopAppWindowProcess(); err != nil {
					appendManagerLog("cleanup closed UI before reopen: " + err.Error())
					platformShowShutdownError(err)
					return false
				}
				if err := openAppWindow(url); err != nil {
					appendManagerLog("reopen UI after cancelled exit: " + err.Error())
				}
			} else {
				restoreManagerWindow()
			}
			return false
		}
		platformHideManagerWindow()
		if err := services.shutdown(); err != nil {
			appendManagerLog("shutdown services: " + err.Error())
			platformShowShutdownError(err)
			platformCancelManagerWindowExit()
			restoreManagerWindow()
			return false
		}
		// The same-origin Web Card polls the runtime lease. Keep the process
		// alive for one poll window after publishing enabled=false so open Emby
		// pages can remove their Manager-owned cards before the UI host exits.
		time.Sleep(1700 * time.Millisecond)
		platformRequestCloseManagerWindow()
		if err := platformStopAppWindowProcess(); err != nil {
			appendManagerLog("verified UI shutdown: " + err.Error())
			platformShowShutdownError(err)
			platformCancelManagerWindowExit()
			restoreManagerWindow()
			return false
		}
		shutdown()
		return true
	}

	for {
		select {
		case <-traySignals.WindowClosed:
			if confirmExit(true) {
				return
			}
		case <-traySignals.ExitRequested:
			if confirmExit(false) {
				return
			}
		case <-traySignals.Quit:
			_ = services.shutdown()
			if err := platformStopAppWindowProcess(); err != nil {
				appendManagerLog("tray host UI shutdown: " + err.Error())
			}
			shutdown()
			return
		}
	}
}

func newSessionToken() string {
	b := make([]byte, 24)
	if _, err := rand.Read(b); err != nil {
		return strconv.FormatInt(time.Now().UnixNano(), 36)
	}
	return base64.RawURLEncoding.EncodeToString(b)
}

func requireToken(next http.HandlerFunc) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		t := r.Header.Get("X-Manager-Token")
		if t == "" {
			t = r.URL.Query().Get("token")
		}
		if t == "" || t != uiToken {
			http.Error(w, "forbidden", http.StatusForbidden)
			return
		}
		next(w, r)
	}
}

func serveIndex(w http.ResponseWriter, r *http.Request) {
	b, err := assets.ReadFile("web/index.html")
	if err != nil {
		http.Error(w, err.Error(), 500)
		return
	}
	w.Header().Set("Content-Type", "text/html; charset=utf-8")
	w.Header().Set("Cache-Control", "no-store")
	w.Header().Set("Referrer-Policy", "no-referrer")
	w.Header().Set("X-Content-Type-Options", "nosniff")
	w.Header().Set("X-Frame-Options", "DENY")
	_, _ = w.Write(b)
}

func serveAppIcon(name string) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		b, err := assets.ReadFile(name)
		if err != nil {
			http.Error(w, err.Error(), http.StatusInternalServerError)
			return
		}
		w.Header().Set("Content-Type", "image/png")
		w.Header().Set("Cache-Control", "no-store")
		_, _ = w.Write(b)
	}
}

func handleStatus(w http.ResponseWriter, r *http.Request) {
	st, err := collectStatus()
	if err != nil {
		writeJSONStatus(w, 500, map[string]string{"error": err.Error()})
		return
	}
	decorateCommonStatus(&st)
	jobs.mu.Lock()
	st.Job = jobs.st
	jobs.mu.Unlock()
	st.Service = services.snapshot()
	localizeStatusForPresentation(&st)
	writeJSON(w, st)
}

func handleCatalog(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		writeJSONStatus(w, http.StatusMethodNotAllowed, map[string]string{"error": "GET only"})
		return
	}
	b, err := os.ReadFile(managerCatalogFile())
	if errors.Is(err, os.ErrNotExist) {
		writeJSON(w, map[string]interface{}{"version": 2, "count": 0, "managerCatalog": []interface{}{}})
		return
	}
	if err != nil {
		writeJSONStatus(w, http.StatusInternalServerError, map[string]string{"error": err.Error()})
		return
	}
	var catalog struct {
		GeneratedAt string            `json:"generatedAt"`
		Count       int               `json:"count"`
		Items       []json.RawMessage `json:"items"`
	}
	if err := json.Unmarshal(b, &catalog); err != nil || catalog.Items == nil {
		writeJSONStatus(w, http.StatusInternalServerError, map[string]string{
			"error": "NFO 管理目录损坏，请运行增量检查重建",
		})
		return
	}
	writeJSON(w, map[string]interface{}{
		"version":        2,
		"generated_at":   catalog.GeneratedAt,
		"count":          len(catalog.Items),
		"managerCatalog": catalog.Items,
	})
}

func decorateCommonStatus(st *Status) {
	set := loadSettings()
	st.RootsConfigured = set.RootsConfigured
	st.ConfiguredRoots = append([]LibraryRoot(nil), set.LibraryRoots...)
	st.DiscoveredRoots = loadDiscoveredLibraries()
	st.AutoStart = false
	st.AutoStart = platformAutoStartEnabled()
	st.SilentStart = set.SilentStart
	st.Language = normalizedLanguage(set.Language)
	st.Languages = supportedLanguages()
	st.Service = services.snapshot()
	st.AgentRunning = st.Service.Running
	if st.Service.Running {
		st.AgentPID = os.Getpid()
	}
	if st.Paths == nil {
		st.Paths = map[string]string{}
	}
	st.Paths["manager_data"] = baseDir()
	st.Paths["manager_executable"] = installedExePath()
	st.Paths["engine"] = enginePath()
	st.Paths["logs"] = logDir()
	if b, err := os.ReadFile(cleanupReportPath()); err == nil {
		var report struct {
			Removed []string `json:"removed"`
		}
		if json.Unmarshal(b, &report) == nil {
			st.CleanupRemoved = report.Removed
		}
	}
}

func handleJob(w http.ResponseWriter, r *http.Request) {
	jobs.mu.Lock()
	st := jobs.st
	jobs.mu.Unlock()
	if st.Log == "" {
		if b, err := os.ReadFile(jobLogPath()); err == nil {
			st.Log = tailString(string(b), 40000)
		}
	} else {
		st.Log = tailString(st.Log, 40000)
	}
	writeJSON(w, st)
}

func handleSettings(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		writeJSONStatus(w, 405, map[string]string{"error": "POST only"})
		return
	}
	r.Body = http.MaxBytesReader(w, r.Body, 1<<20)
	var raw map[string]json.RawMessage
	if err := json.NewDecoder(r.Body).Decode(&raw); err != nil {
		writeJSONStatus(w, 400, map[string]string{"error": err.Error()})
		return
	}

	set := loadSettings()
	if v, ok := raw["interval_seconds"]; ok {
		var n int
		if err := json.Unmarshal(v, &n); err != nil || n < 30 || n > 86400 {
			writeJSONStatus(w, 400, map[string]string{"error": "后台检查周期必须在 30 秒到 86400 秒之间"})
			return
		}
		set.IntervalSeconds = n
	}
	if v, ok := raw["auto_start"]; ok {
		var enabled bool
		if err := json.Unmarshal(v, &enabled); err != nil {
			writeJSONStatus(w, 400, map[string]string{"error": err.Error()})
			return
		}
		if err := platformSetAutoStart(enabled, io.Discard); err != nil {
			writeJSONStatus(w, 500, map[string]string{"error": err.Error()})
			return
		}
		set.AutoStart = enabled
		set.AutoStartConfigured = true
	}
	if v, ok := raw["silent_start"]; ok {
		var enabled bool
		if err := json.Unmarshal(v, &enabled); err != nil {
			writeJSONStatus(w, 400, map[string]string{"error": err.Error()})
			return
		}
		set.SilentStart = enabled
	}
	if v, ok := raw["language"]; ok {
		var language string
		if err := json.Unmarshal(v, &language); err != nil {
			writeJSONStatus(w, 400, map[string]string{"error": "所选语言无效"})
			return
		}
		language = normalizedLanguageAlias(language)
		if !supportedLanguage(language) {
			writeJSONStatus(w, 400, map[string]string{"error": "所选语言尚未安装或不受当前版本支持"})
			return
		}
		set.Language = language
	}
	if v, ok := raw["library_roots"]; ok {
		var roots []LibraryRoot
		if err := json.Unmarshal(v, &roots); err != nil {
			writeJSONStatus(w, 400, map[string]string{"error": "媒体目录格式无效：" + err.Error()})
			return
		}
		cleaned, err := sanitizeLibraryRoots(roots)
		if err != nil {
			writeJSONStatus(w, 400, map[string]string{"error": err.Error()})
			return
		}
		set.LibraryRoots = cleaned
	}
	if v, ok := raw["roots_configured"]; ok {
		var configured bool
		if err := json.Unmarshal(v, &configured); err != nil {
			writeJSONStatus(w, 400, map[string]string{"error": "目录配置状态无效"})
			return
		}
		set.RootsConfigured = configured
	}
	if set.RootsConfigured {
		enabled := 0
		for _, root := range set.LibraryRoots {
			if root.Enabled {
				enabled++
			}
		}
		if enabled == 0 {
			writeJSONStatus(w, 400, map[string]string{"error": "至少启用一个媒体目录"})
			return
		}
	}

	if err := saveSettings(set); err != nil {
		writeJSONStatus(w, 500, map[string]string{"error": err.Error()})
		return
	}

	writeJSON(w, map[string]bool{"ok": true})
}

func handleAction(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		writeJSONStatus(w, 405, map[string]string{"error": "POST only"})
		return
	}
	r.Body = http.MaxBytesReader(w, r.Body, 1<<20)
	var req actionRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeJSONStatus(w, 400, map[string]string{"error": err.Error()})
		return
	}
	req.Action = strings.TrimSpace(req.Action)
	req.IMDb = strings.TrimSpace(req.IMDb)
	req.Path = strings.TrimSpace(req.Path)

	switch req.Action {
	case "service-start":
		if err := services.requestStart(); err != nil {
			if errors.Is(err, errLegacyMigrationRequired) {
				writeJSONStatus(w, http.StatusConflict, map[string]interface{}{
					"error":           err.Error(),
					"legacy_required": true,
				})
				return
			}
			writeJSONStatus(w, http.StatusConflict, map[string]string{"error": err.Error()})
			return
		}
		writeJSON(w, map[string]interface{}{"ok": true, "message": "服务已启动"})
		return
	case "service-stop":
		if err := services.stop("服务已停止；Emby 技术规格卡片已撤下"); err != nil {
			appendManagerLog("service stop lease: " + err.Error())
			writeJSONStatus(w, http.StatusInternalServerError, map[string]string{
				"error": "服务调度已停止，但未能确认 Emby 撤卡；请点击按钮重试：" + err.Error(),
			})
			return
		}
		writeJSON(w, map[string]interface{}{"ok": true, "message": "服务已停止，Emby 将不再显示技术规格卡片"})
		return
	case "legacy-cancel":
		services.cancelLegacyMigration()
		writeJSON(w, map[string]interface{}{"ok": true, "message": "已取消；旧版保持不变，新版服务未启动"})
		return
	case "migrate-legacy":
		if err := services.beginLegacyMigration(); err != nil {
			writeJSONStatus(w, http.StatusConflict, map[string]string{"error": err.Error()})
			return
		}
		writeJSON(w, map[string]interface{}{
			"ok":      true,
			"message": "正在处理旧版迁移；如 Windows 需要确认，将显示管理员权限窗口。迁移完成前新版服务保持停止。",
		})
		return
	case "open-data":
		if err := openPath(platformDataPath()); err != nil {
			writeJSONStatus(w, 500, map[string]string{"error": err.Error()})
			return
		}
		writeJSON(w, map[string]bool{"ok": true})
		return
	case "open-logs":
		_ = os.MkdirAll(logDir(), 0755)
		if err := openPath(logDir()); err != nil {
			writeJSONStatus(w, 500, map[string]string{"error": err.Error()})
			return
		}
		writeJSON(w, map[string]bool{"ok": true})
		return
	case "open-path":
		if req.Path == "" {
			writeJSONStatus(w, 400, map[string]string{"error": "路径为空"})
			return
		}
		p := req.Path
		if info, err := os.Stat(p); err == nil && !info.IsDir() {
			p = filepath.Dir(p)
		} else if err != nil {
			p = filepath.Dir(p)
		}
		if err := openPath(p); err != nil {
			writeJSONStatus(w, 500, map[string]string{"error": err.Error()})
			return
		}
		writeJSON(w, map[string]bool{"ok": true})
		return
	case "choose-library-root":
		path, err := platformChooseFolder()
		if err != nil {
			writeJSONStatus(w, 500, map[string]string{"error": err.Error()})
			return
		}
		writeJSON(w, map[string]string{"path": path})
		return
	case "export-diagnostics":
		p, err := exportDiagnostics()
		if err != nil {
			writeJSONStatus(w, 500, map[string]string{"error": err.Error()})
			return
		}
		_ = openPath(filepath.Dir(p))
		writeJSON(w, map[string]string{"ok": "true", "path": p})
		return
	}

	if (req.Action == "run" || req.Action == "scan-space" || req.Action == "scan-root" || req.Action == "rebuild-index" || req.Action == "discover-roots") && !serviceIsRunning() {
		writeJSONStatus(w, http.StatusConflict, map[string]string{"error": "服务已停止，请先启动服务"})
		return
	}

	if isPrivilegedAction(req.Action) && runtime.GOOS == "windows" {
		var beforeRun func() error
		if req.Action == "disable-integration" {
			beforeRun = func() error {
				return services.stopForMaintenance("服务已停止；正在恢复原生 Emby")
			}
		}
		message, err := startPrivilegedJob(req.Action, beforeRun, nil)
		if err != nil {
			writeJSONStatus(w, http.StatusConflict, map[string]string{"error": err.Error()})
			return
		}
		writeJSON(w, map[string]interface{}{"ok": true, "message": message})
		return
	}

	arg := req.IMDb
	if req.Action == "scan-space" {
		arg = strings.TrimSpace(req.Path)
		if arg != "movies" && arg != "tv" {
			writeJSONStatus(w, http.StatusBadRequest, map[string]string{"error": "当前媒体库只能是 movies 或 tv"})
			return
		}
	}
	if req.Path != "" {
		arg = req.Path
	}
	if err := startJob(req.Action, arg); err != nil {
		writeJSONStatus(w, 409, map[string]string{"error": err.Error()})
		return
	}
	writeJSON(w, map[string]bool{"ok": true})
}

func startJob(action, arg string) error {
	return startJobWithParent(context.Background(), action, arg)
}

func startJobWithParent(parent context.Context, action, arg string) error {
	return startManagedJob(parent, action, managedJobOptions{
		message: "运行中",
		run: func(ctx context.Context, w io.Writer) error {
			return performActionWithWriter(ctx, action, arg, w)
		},
	})
}

func startManagedJob(parent context.Context, action string, options managedJobOptions) error {
	jobLanguage := currentLanguage()
	packRevision, catalogHash := languagePackIdentity(jobLanguage)
	jobs.mu.Lock()
	if jobs.st.Running {
		jobs.mu.Unlock()
		return fmt.Errorf("已有任务正在运行：%s", jobs.st.Action)
	}
	ctx, cancel := context.WithCancel(parent)
	done := make(chan struct{})
	now := time.Now().Format(time.RFC3339)
	message := strings.TrimSpace(options.message)
	if message == "" {
		message = "运行中"
	}
	message = localizeBackendText(jobLanguage, message)
	jobs.st = JobState{
		Running:              true,
		Action:               action,
		StartedAt:            now,
		Message:              message,
		BlocksExit:           options.blocksExit,
		NeedsAdmin:           options.needsAdmin,
		AlreadyAdmin:         options.alreadyAdmin,
		Language:             jobLanguage,
		LanguagePackRevision: packRevision,
		LanguageCatalogHash:  catalogHash,
	}
	jobs.cancel = cancel
	jobs.done = done
	jobs.mu.Unlock()

	go func() {
		defer cancel()
		defer close(done)
		_ = os.MkdirAll(logDir(), 0755)
		f, err := os.Create(jobLogPath())
		if err != nil {
			finishJob(action, 1, err.Error())
			if options.after != nil {
				options.after(err)
			}
			return
		}
		defer f.Close()
		jobWriter := newLocalizedLineWriter(f, jobLanguage)
		fmt.Fprintf(jobWriter, "Tech Card Manager %s\n任务：%s\n开始时间：%s\n\n", appVersion, actionLabel(action), now)
		if options.run == nil {
			err = fmt.Errorf("任务没有可执行入口：%s", action)
		} else {
			err = options.run(ctx, jobWriter)
		}
		code := 0
		msg := localizeBackendText(jobLanguage, "完成")
		if err != nil {
			code = 1
			msg = localizeBackendText(jobLanguage, err.Error())
			fmt.Fprintf(jobWriter, "\n错误：%v\n", err)
		}
		_ = jobWriter.Flush()
		_ = f.Sync()
		finishJob(action, code, msg)
		if options.after != nil {
			options.after(err)
		}
	}()
	return nil
}

func isPrivilegedAction(action string) bool {
	switch action {
	case "install", "repair-web", "disable-integration", "migrate-legacy":
		return true
	default:
		return false
	}
}

func startPrivilegedJob(action string, beforeRun func() error, after func(error)) (string, error) {
	if !isPrivilegedAction(action) {
		return "", fmt.Errorf("不允许使用管理员任务执行未知操作：%s", action)
	}
	alreadyAdmin, err := platformProcessIsElevated()
	if err != nil {
		return "", fmt.Errorf("无法确认当前管理员权限状态，操作未开始：%w", err)
	}

	message := "正在申请管理员权限；如 Windows 需要确认，将显示 UAC 窗口。"
	if alreadyAdmin {
		message = "当前程序已具备管理员权限，操作已开始。"
	}
	err = startManagedJob(context.Background(), action, managedJobOptions{
		blocksExit:   true,
		needsAdmin:   true,
		alreadyAdmin: alreadyAdmin,
		message:      message,
		after:        after,
		run: func(ctx context.Context, w io.Writer) error {
			if alreadyAdmin {
				if beforeRun != nil {
					if err := beforeRun(); err != nil {
						return err
					}
				}
				if err := performActionWithWriter(ctx, action, "", w); err != nil {
					return err
				}
			} else if err := platformRunElevatedAction(ctx, action, beforeRun, w); err != nil {
				return err
			}
			return verifyPrivilegedAction(action)
		},
	})
	if err != nil {
		return "", err
	}
	return message, nil
}

func verifyPrivilegedAction(action string) error {
	switch action {
	case "install", "repair-web":
		st, err := collectStatus()
		if err != nil {
			return fmt.Errorf("管理员操作已结束，但网页集成复核失败：%w", err)
		}
		injected, _ := st.Extra["web_index_injected"].(bool)
		jsExists, _ := st.Extra["web_js_exists"].(bool)
		jsMatches, _ := st.Extra["web_js_matches"].(bool)
		if !injected || !jsExists || !jsMatches {
			return fmt.Errorf("管理员操作已结束，但网页集成文件没有通过完整性复核")
		}
	case "disable-integration":
		st, err := collectStatus()
		if err != nil {
			return fmt.Errorf("恢复操作已结束，但结果复核失败：%w", err)
		}
		injected, _ := st.Extra["web_index_injected"].(bool)
		jsExists, _ := st.Extra["web_js_exists"].(bool)
		if injected || jsExists || st.WebPatch {
			return fmt.Errorf("恢复操作未完全移除本程序拥有的 Emby 网页集成")
		}
	case "migrate-legacy":
		if report := platformDetectLegacy(); report.Required {
			return fmt.Errorf("迁移操作已结束，但仍检测到旧版组件；新版服务保持停止")
		}
	}
	return nil
}

func actionLabel(action string) string {
	labels := map[string]string{
		"install": "启用或修复 Emby 集成", "start": "启动后台服务", "stop": "停止后台服务",
		"run": "立即增量检查", "diagnose": "运行诊断", "repair-web": "修复网页卡片",
		"migrate-legacy": "迁移所列旧版组件",
		"scan-root":      "只扫描此目录",
		"scan-space":     "刷新当前媒体库",
		"rebuild-index":  "完整重建索引", "discover-roots": "从 Emby 发现媒体目录",
		"cleanup-legacy": "清理旧版残留", "disable-integration": "彻底停用并恢复原生 Emby",
	}
	if label := labels[action]; label != "" {
		return label
	}
	return action
}

func finishJob(action string, code int, msg string) {
	b, _ := os.ReadFile(jobLogPath())
	jobs.mu.Lock()
	jobs.st.Running = false
	jobs.st.Action = action
	jobs.st.EndedAt = time.Now().Format(time.RFC3339)
	jobs.st.ExitCode = code
	jobs.st.Message = msg
	jobs.st.Log = tailString(string(b), 40000)
	jobs.st.BlocksExit = false
	jobs.cancel = nil
	jobs.done = nil
	jobs.mu.Unlock()
}

func cancelActiveJob() {
	jobs.mu.Lock()
	cancel := jobs.cancel
	done := jobs.done
	blocksExit := jobs.st.Running && jobs.st.BlocksExit
	jobs.mu.Unlock()
	if blocksExit {
		// Administrator maintenance may be in the middle of a transactional Emby
		// index replacement. Never abandon its elevated child or kill it halfway;
		// shutdown waits until the exact owned operation has reported completion.
		if done != nil {
			<-done
		}
		return
	}
	if cancel != nil {
		cancel()
	}
	if done != nil {
		select {
		case <-done:
		case <-time.After(8 * time.Second):
			appendManagerLog("job cancellation timed out")
		}
	}
}

func waitForBlockingJob(timeout time.Duration) error {
	jobs.mu.Lock()
	done := jobs.done
	blocksExit := jobs.st.Running && jobs.st.BlocksExit
	action := jobs.st.Action
	jobs.mu.Unlock()
	if !blocksExit || done == nil {
		return nil
	}
	timer := time.NewTimer(timeout)
	defer timer.Stop()
	select {
	case <-done:
		return nil
	case <-timer.C:
		return fmt.Errorf("%w：%s；事务仍由当前程序持有，没有遗留到后台", errMaintenanceExitTimeout, actionLabel(action))
	}
}

func performImmediateAction(action, arg string) error {
	_ = os.MkdirAll(logDir(), 0755)
	f, err := os.OpenFile(managerLogPath(), os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0644)
	if err != nil {
		return err
	}
	defer f.Close()
	return performActionWithWriter(context.Background(), action, arg, f)
}

func performActionWithWriter(ctx context.Context, action, arg string, w io.Writer) error {
	switch action {
	case "install":
		return platformInstall(ctx, w)
	case "migrate-legacy":
		return platformMigrateLegacy(ctx, w)
	case "disable-integration":
		return platformDisableIntegration(ctx, w)
	case "start":
		return services.requestStart()
	case "stop":
		return services.stop("服务已停止；Emby 技术规格卡片已撤下")
	case "cleanup-legacy":
		_, err := cleanupLegacyArtifacts(w)
		return err
	case "auto":
		return platformRunEngine(ctx, "auto", arg, w)
	case "run", "scan-root":
		return platformRunEngine(ctx, "run", arg, w)
	case "scan-space":
		space := strings.ToLower(strings.TrimSpace(arg))
		settings := loadSettings()
		roots, mixed := enabledRootsForSpace(settings.LibraryRoots, space)
		for _, root := range roots {
			if err := platformRunEngine(ctx, "run", root.Path, w); err != nil {
				return fmt.Errorf("刷新 %s 失败（%s）：%w", space, root.Path, err)
			}
		}
		if len(roots) == 0 {
			if mixed > 0 {
				return fmt.Errorf("当前媒体库没有独立分类目录；请先在设置中把混合目录拆分或标记为电影/电视剧，已避免误扫另一类媒体")
			}
			return fmt.Errorf("当前媒体库没有已启用且已分类的目录")
		}
		return nil
	case "diagnose", "repair-web", "rebuild-index", "discover-roots":
		return platformRunEngine(ctx, action, arg, w)
	default:
		return fmt.Errorf("未知操作：%s", action)
	}
}

func enabledRootsForSpace(roots []LibraryRoot, space string) ([]LibraryRoot, int) {
	space = strings.ToLower(strings.TrimSpace(space))
	matched := make([]LibraryRoot, 0, len(roots))
	mixed := 0
	for _, root := range roots {
		if !root.Enabled {
			continue
		}
		kind := strings.ToLower(strings.TrimSpace(root.Kind))
		if kind == "mixed" {
			mixed++
			continue
		}
		if kind == space {
			matched = append(matched, root)
		}
	}
	return matched, mixed
}

func ensureAssets() error {
	if err := os.MkdirAll(engineDir(), 0755); err != nil {
		return err
	}
	if runtime.GOOS != "windows" {
		return fmt.Errorf("Windows Server 版本不支持平台：%s", runtime.GOOS)
	}
	name := "engine/windows-engine.ps1"
	b, err := assets.ReadFile(name)
	if err != nil {
		return err
	}
	b = withUTF8BOM(b)
	path := enginePath()
	old, _ := os.ReadFile(path)
	if string(old) != string(b) {
		if err := atomicWrite(path, b, 0755); err != nil {
			return err
		}
	}
	if runtime.GOOS == "windows" {
		if js, err := assets.ReadFile("engine/technical-specs-card.js"); err == nil {
			jsPath := filepath.Join(engineDir(), "technical-specs-card.js")
			oldJS, _ := os.ReadFile(jsPath)
			if string(oldJS) != string(js) {
				if err := atomicWrite(jsPath, js, 0644); err != nil {
					return err
				}
			}
		}
	}
	_ = os.MkdirAll(logDir(), 0755)
	if source, err := platformEnsureSettingsContinuity(); err != nil {
		appendManagerLog("settings continuity: " + err.Error())
	} else if source != "" {
		appendManagerLog("restored media settings from " + source)
	}
	if _, err := os.Stat(settingsPath()); errors.Is(err, os.ErrNotExist) {
		if err := saveSettings(Settings{IntervalSeconds: 60, Language: defaultLanguage}); err != nil {
			return err
		}
	}
	return nil
}

func loadSettings() Settings {
	s := Settings{IntervalSeconds: 60, Language: defaultLanguage}
	b, err := os.ReadFile(settingsPath())
	if err == nil {
		_ = json.Unmarshal(b, &s)
	}
	if s.IntervalSeconds < 30 {
		s.IntervalSeconds = 60
	}
	s.Language = configuredLanguage(s.Language)
	return s
}

func sanitizeLibraryRoots(roots []LibraryRoot) ([]LibraryRoot, error) {
	if len(roots) > 256 {
		return nil, fmt.Errorf("媒体目录不能超过 256 个")
	}
	cleaned := make([]LibraryRoot, 0, len(roots))
	seen := map[string]string{}
	for _, root := range roots {
		path := strings.Trim(strings.TrimSpace(root.Path), "\"")
		if path == "" {
			return nil, fmt.Errorf("媒体目录路径不能为空")
		}
		path = filepath.Clean(path)
		if !filepath.IsAbs(path) {
			return nil, fmt.Errorf("媒体目录必须使用绝对路径：%s", path)
		}
		volume := filepath.VolumeName(path)
		remainder := strings.TrimPrefix(path, volume)
		isUNCShare := strings.HasPrefix(volume, `\\`)
		if !isUNCShare && (remainder == string(filepath.Separator) || remainder == "") {
			return nil, fmt.Errorf("不能直接扫描整个磁盘根目录：%s", path)
		}
		key := strings.ToLower(strings.TrimRight(path, `\/`))
		if _, exists := seen[key]; exists {
			return nil, fmt.Errorf("媒体目录重复：%s", path)
		}
		for previousKey, previousPath := range seen {
			if strings.HasPrefix(key, previousKey+`\`) || strings.HasPrefix(previousKey, key+`\`) {
				return nil, fmt.Errorf("媒体目录相互包含，会造成重复扫描：%s 与 %s", previousPath, path)
			}
		}
		seen[key] = path
		source := strings.ToLower(strings.TrimSpace(root.Source))
		if source != "auto" && source != "manual" {
			source = "manual"
		}
		kind := strings.ToLower(strings.TrimSpace(root.Kind))
		if kind != "movies" && kind != "tv" && kind != "mixed" {
			kind = "auto"
		}
		name := strings.TrimSpace(root.Name)
		if name == "" {
			name = filepath.Base(path)
		}
		cleaned = append(cleaned, LibraryRoot{
			Path: path, Name: name, Kind: kind, Source: source, Enabled: root.Enabled,
		})
	}
	return cleaned, nil
}

func withUTF8BOM(b []byte) []byte {
	bom := []byte{0xEF, 0xBB, 0xBF}
	if bytes.HasPrefix(b, bom) {
		return b
	}
	out := make([]byte, 0, len(b)+len(bom))
	out = append(out, bom...)
	return append(out, b...)
}

func saveSettings(s Settings) error {
	if s.IntervalSeconds < 30 {
		s.IntervalSeconds = 60
	}
	s.Language = configuredLanguage(s.Language)
	b, _ := json.MarshalIndent(s, "", "  ")
	return atomicWrite(settingsPath(), b, 0644)
}

func atomicWrite(path string, b []byte, mode fs.FileMode) error {
	if err := os.MkdirAll(filepath.Dir(path), 0755); err != nil {
		return err
	}
	f, err := os.CreateTemp(filepath.Dir(path), "."+filepath.Base(path)+".*.tmp")
	if err != nil {
		return err
	}
	tmp := f.Name()
	defer os.Remove(tmp)
	if err := f.Chmod(mode); err != nil {
		_ = f.Close()
		return err
	}
	if _, err := f.Write(b); err != nil {
		_ = f.Close()
		return err
	}
	if err := f.Sync(); err != nil {
		_ = f.Close()
		return err
	}
	if err := f.Close(); err != nil {
		return err
	}
	return platformAtomicReplace(tmp, path)
}

func writeJSON(w http.ResponseWriter, v interface{}) {
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.Header().Set("Cache-Control", "no-store")
	_ = json.NewEncoder(w).Encode(localizeResponsePayload(currentLanguage(), v))
}

func writeJSONStatus(w http.ResponseWriter, code int, v interface{}) {
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.Header().Set("Cache-Control", "no-store")
	w.WriteHeader(code)
	_ = json.NewEncoder(w).Encode(localizeResponsePayload(currentLanguage(), v))
}

func tailString(s string, max int) string {
	if len(s) <= max {
		return s
	}
	return "…\n" + s[len(s)-max:]
}

func appendManagerLog(s string) {
	_ = os.MkdirAll(logDir(), 0755)
	f, err := os.OpenFile(managerLogPath(), os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0644)
	if err == nil {
		fmt.Fprintf(f, "[%s] %s\n", time.Now().Format(time.RFC3339), s)
		f.Close()
	}
}

func cleanupLegacyArtifacts(w io.Writer) ([]string, error) {
	removed, err := platformCleanupLegacy(w)
	report := map[string]interface{}{
		"version": appVersion,
		"time":    time.Now().Format(time.RFC3339),
		"removed": removed,
	}
	b, _ := json.MarshalIndent(report, "", "  ")
	_ = atomicWrite(cleanupReportPath(), b, 0644)
	return removed, err
}

func exportDiagnostics() (string, error) {
	_ = os.MkdirAll(logDir(), 0755)
	out := filepath.Join(logDir(), "IMDb-Tech-Diagnostics-"+time.Now().Format("20060102-150405")+".zip")
	f, err := os.Create(out)
	if err != nil {
		return "", err
	}
	zw := zip.NewWriter(f)
	paths := []string{
		settingsPath(), managerLogPath(), agentLogPath(), jobLogPath(),
		agentHeartbeatPath(), agentCyclePath(), cleanupReportPath(), enginePath(),
	}
	paths = append(paths, platformDiagnosticPaths()...)
	seen := map[string]bool{}
	for _, p := range paths {
		if p == "" || seen[p] {
			continue
		}
		seen[p] = true
		info, err := os.Stat(p)
		if err != nil || info.IsDir() {
			continue
		}
		r, err := os.Open(p)
		if err != nil {
			continue
		}
		h := &zip.FileHeader{Name: filepath.Base(p), Method: zip.Deflate}
		h.SetModTime(info.ModTime())
		zf, err := zw.CreateHeader(h)
		if err == nil {
			_, _ = io.Copy(zf, io.LimitReader(r, 12<<20))
		}
		r.Close()
	}
	if st, err := collectStatus(); err == nil {
		decorateCommonStatus(&st)
		zf, _ := zw.Create("status.json")
		enc := json.NewEncoder(zf)
		enc.SetIndent("", "  ")
		_ = enc.Encode(st)
	}
	_ = zw.Close()
	_ = f.Close()
	return out, nil
}

func runCommandToWriter(w io.Writer, name string, args ...string) error {
	return runCommandToWriterContext(context.Background(), w, name, args...)
}

func runCommandToWriterContext(ctx context.Context, w io.Writer, name string, args ...string) error {
	cmd := hiddenCommandContext(context.Background(), name, args...)
	cmd.Stdout = w
	cmd.Stderr = w
	if err := cmd.Start(); err != nil {
		return err
	}
	done := make(chan error, 1)
	go func() { done <- cmd.Wait() }()
	select {
	case err := <-done:
		return err
	case <-ctx.Done():
		// Kill only the exact process tree launched for this job. This avoids a
		// PowerShell grandchild surviving after the visual Manager exits.
		_, _ = commandOutput("taskkill.exe", "/PID", strconv.Itoa(cmd.Process.Pid), "/T", "/F")
		select {
		case <-done:
		case <-time.After(3 * time.Second):
		}
		return fmt.Errorf("任务已取消：%w", ctx.Err())
	}
}

func commandOutput(name string, args ...string) (string, error) {
	cmd := hiddenCommand(name, args...)
	b, err := cmd.CombinedOutput()
	return strings.TrimSpace(string(b)), err
}

func readAgentPID() int {
	b, err := os.ReadFile(agentPIDPath())
	if err != nil {
		return 0
	}
	n, _ := strconv.Atoi(strings.TrimSpace(string(b)))
	return n
}

func saveAgentCycle(s AgentCycleState) error {
	b, _ := json.MarshalIndent(s, "", "  ")
	return atomicWrite(agentCyclePath(), b, 0644)
}

func loadAgentCycle() AgentCycleState {
	var s AgentCycleState
	b, err := os.ReadFile(agentCyclePath())
	if err == nil {
		_ = json.Unmarshal(b, &s)
	}
	return s
}

func engineDir() string          { return filepath.Join(portableRootDir(), "runtime", "engine") }
func logDir() string             { return filepath.Join(portableRootDir(), "logs") }
func settingsPath() string       { return filepath.Join(baseDir(), "settings.json") }
func managerLogPath() string     { return filepath.Join(logDir(), "manager.log") }
func agentLogPath() string       { return filepath.Join(logDir(), "agent.log") }
func jobLogPath() string         { return filepath.Join(logDir(), "job.log") }
func agentPIDPath() string       { return filepath.Join(baseDir(), "agent.pid") }
func agentHeartbeatPath() string { return filepath.Join(baseDir(), "agent-heartbeat.txt") }
func agentCyclePath() string     { return filepath.Join(baseDir(), "agent-cycle.json") }
func cleanupReportPath() string  { return filepath.Join(baseDir(), "cleanup-report.json") }

func init() {
	log.SetOutput(io.Discard)
}
