//go:build windows

package main

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"sync"
	"syscall"
	"time"
	"unsafe"
)

const oldWinTaskName = "Emby Technical Specs Web Card"
const winRunValueName = "Tech Card Manager"
const legacyWinRunValueName = "IMDb Tech Manager Agent"
const expectedWebCardVersion = "4.1.0"

func baseDir() string {
	return filepath.Join(portableRootDir(), "data")
}

func portableRootDir() string {
	exe, err := os.Executable()
	if err != nil {
		return "."
	}
	exe, _ = filepath.EvalSymlinks(exe)
	return filepath.Dir(exe)
}

func ensurePortableWorkspace() error {
	root := portableRootDir()
	probe, err := os.CreateTemp(root, ".imdb-tech-write-check-*")
	if err != nil {
		return fmt.Errorf("Portable 程序目录不可写，请把整个软件文件夹移动到可写位置后重试：%s：%w", root, err)
	}
	probePath := probe.Name()
	if err := probe.Close(); err != nil {
		_ = os.Remove(probePath)
		return err
	}
	if err := os.Remove(probePath); err != nil {
		return fmt.Errorf("Portable 程序目录无法清理临时文件：%s：%w", root, err)
	}
	for _, name := range []string{"runtime", "data", "logs", "backup", "updates"} {
		if err := os.MkdirAll(filepath.Join(root, name), 0755); err != nil {
			return fmt.Errorf("无法创建 Portable 目录 %s：%w", name, err)
		}
	}
	cleanupStaleEdgeProfiles()
	return nil
}

func cleanupStaleEdgeProfiles() {
	runtimeRoot := filepath.Join(portableRootDir(), "runtime")
	// The single-instance mutex is acquired before this startup cleanup. These
	// paths are therefore stale Manager-owned profiles, never another live
	// Manager session or the user's normal Edge profile.
	for _, path := range []string{filepath.Join(runtimeRoot, "edge-profile")} {
		if err := os.RemoveAll(path); err != nil {
			appendManagerLog("stale Edge profile cleanup: " + err.Error())
		}
	}
	sessionsRoot := filepath.Join(runtimeRoot, "edge-sessions")
	entries, err := os.ReadDir(sessionsRoot)
	if err != nil && !errors.Is(err, os.ErrNotExist) {
		appendManagerLog("stale Edge session list: " + err.Error())
		return
	}
	for _, entry := range entries {
		if !entry.IsDir() || !strings.HasPrefix(entry.Name(), "session-") {
			continue
		}
		if err := os.RemoveAll(filepath.Join(sessionsRoot, entry.Name())); err != nil {
			appendManagerLog("stale Edge session cleanup: " + err.Error())
		}
	}
}

func enginePath() string { return filepath.Join(engineDir(), "windows-engine.ps1") }
func installedExePath() string {
	exe, _ := os.Executable()
	exe, _ = filepath.EvalSymlinks(exe)
	return exe
}
func platformDataPath() string {
	return filepath.Join(os.Getenv("APPDATA"), "Emby-Server", "programdata", "custom-tech-specs")
}
func embyRoot() string { return filepath.Join(os.Getenv("APPDATA"), "Emby-Server") }
func techDataFile() string {
	return filepath.Join(embyRoot(), "system", "dashboard-ui", "technical-specs-data.json")
}
func cardRuntimeFile() string {
	return filepath.Join(embyRoot(), "system", "dashboard-ui", "technical-specs-runtime.json")
}
func xmlErrorFile() string       { return filepath.Join(platformDataPath(), "manager-xml-errors.json") }
func managerCatalogFile() string { return filepath.Join(platformDataPath(), "manager-catalog.json") }
func rootDiscoveryFile() string {
	return filepath.Join(platformDataPath(), "manager-root-discovery.json")
}
func indexHTML() string { return filepath.Join(embyRoot(), "system", "dashboard-ui", "index.html") }
func liveWebCardJS() string {
	return filepath.Join(embyRoot(), "system", "dashboard-ui", "technical-specs-card.js")
}
func webCardLanguageFile() string {
	return filepath.Join(embyRoot(), "system", "dashboard-ui", "technical-specs-languages.json")
}
func webPatchBackupRoot() string { return filepath.Join(portableRootDir(), "backup", "web-patch") }
func indexSummaryFile() string {
	return filepath.Join(platformDataPath(), "manager-index-summary.json")
}

func validatedConfiguredSettings(settings Settings) (Settings, error) {
	if settings.IntervalSeconds < 30 || settings.IntervalSeconds > 86400 {
		settings.IntervalSeconds = 60
	}
	if !settings.RootsConfigured {
		return Settings{}, fmt.Errorf("媒体目录尚未确认")
	}
	cleaned, err := sanitizeLibraryRoots(settings.LibraryRoots)
	if err != nil {
		return Settings{}, err
	}
	enabled := 0
	for _, root := range cleaned {
		if root.Enabled {
			enabled++
		}
	}
	if enabled == 0 {
		return Settings{}, fmt.Errorf("媒体目录中没有启用项")
	}
	settings.LibraryRoots = cleaned
	settings.RootsConfigured = true
	return settings, nil
}

func recoveredKind(value string) string {
	value = strings.ToLower(strings.TrimSpace(value))
	switch {
	case strings.Contains(value, "mixed"):
		return "mixed"
	case strings.Contains(value, "tv"), strings.Contains(value, "series"):
		return "tv"
	case strings.Contains(value, "movie"):
		return "movies"
	default:
		return "auto"
	}
}

func recoverSettingsFromIndexSummary() (Settings, error) {
	b, err := os.ReadFile(indexSummaryFile())
	if err != nil {
		return Settings{}, err
	}
	var summary struct {
		GeneratedAt string                     `json:"generatedAt"`
		ScanStats   map[string]json.RawMessage `json:"scanStats"`
		Items       map[string]json.RawMessage `json:"items"`
		Libraries   []struct {
			Name string `json:"name"`
			Path string `json:"path"`
			Kind string `json:"kind"`
		} `json:"libraries"`
		LibraryRoots []string `json:"libraryRoots"`
	}
	if err := json.Unmarshal(b, &summary); err != nil {
		return Settings{}, err
	}
	if strings.TrimSpace(summary.GeneratedAt) == "" || summary.ScanStats == nil || summary.Items == nil {
		return Settings{}, fmt.Errorf("索引摘要不完整，不能作为目录恢复依据")
	}
	settings := Settings{IntervalSeconds: 60, Language: defaultLanguage, RootsConfigured: true}
	for _, library := range summary.Libraries {
		if strings.TrimSpace(library.Path) == "" {
			continue
		}
		settings.LibraryRoots = append(settings.LibraryRoots, LibraryRoot{
			Path: library.Path, Name: library.Name, Kind: recoveredKind(library.Kind),
			Source: "auto", Enabled: true,
		})
	}
	if len(settings.LibraryRoots) == 0 {
		for _, path := range summary.LibraryRoots {
			if strings.TrimSpace(path) != "" {
				settings.LibraryRoots = append(settings.LibraryRoots, LibraryRoot{
					Path: path, Kind: "auto", Source: "auto", Enabled: true,
				})
			}
		}
	}
	return validatedConfiguredSettings(settings)
}

func platformEnsureSettingsContinuity() (string, error) {
	if b, err := os.ReadFile(settingsPath()); err == nil {
		var current Settings
		if json.Unmarshal(b, &current) == nil {
			if _, validErr := validatedConfiguredSettings(current); validErr == nil {
				return "", nil
			}
		}
	}
	settings, err := recoverSettingsFromIndexSummary()
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return "", nil
		}
		return "", fmt.Errorf("上一版有效索引摘要：%w", err)
	}
	b, err := json.MarshalIndent(settings, "", "  ")
	if err != nil {
		return "", err
	}
	if err := atomicWrite(settingsPath(), b, 0644); err != nil {
		return "", err
	}
	return "上一版有效索引摘要", nil
}

// platformAtomicReplace replaces a Manager-owned state file without the old
// remove-then-rename gap. Host-owned index.html uses the stricter PowerShell
// transaction writer, including its own backup, CAS and rollback checks.
func platformAtomicReplace(source, destination string) error {
	src, err := syscall.UTF16PtrFromString(source)
	if err != nil {
		return err
	}
	dst, err := syscall.UTF16PtrFromString(destination)
	if err != nil {
		return err
	}
	moveFileEx := syscall.NewLazyDLL("kernel32.dll").NewProc("MoveFileExW")
	const moveFileReplaceExisting = 0x1
	const moveFileWriteThrough = 0x8
	ret, _, callErr := moveFileEx.Call(
		uintptr(unsafe.Pointer(src)),
		uintptr(unsafe.Pointer(dst)),
		moveFileReplaceExisting|moveFileWriteThrough,
	)
	if ret == 0 {
		return fmt.Errorf("MoveFileExW(%s -> %s): %v", source, destination, callErr)
	}
	return nil
}

func hiddenCommand(name string, args ...string) *exec.Cmd {
	return hiddenCommandContext(context.Background(), name, args...)
}

func hiddenCommandContext(ctx context.Context, name string, args ...string) *exec.Cmd {
	cmd := exec.CommandContext(ctx, name, args...)
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true, CreationFlags: 0x08000000}
	return cmd
}

func platformInstall(ctx context.Context, w io.Writer) error {
	if err := ensurePortableWorkspace(); err != nil {
		return err
	}
	if err := ensureAssets(); err != nil {
		return err
	}

	fmt.Fprintln(w, "事务化安装/修复 Emby 技术规格网页卡片…")
	var details strings.Builder
	output := io.MultiWriter(w, &details)
	if err := runCommandToWriterContext(
		ctx,
		output,
		"powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass",
		"-File", enginePath(), "-RepairWebOnly", "-BackupRootPath", webPatchBackupRoot(),
		"-OutputLanguage", engineOutputLanguageForWriter(w),
	); err != nil {
		message := compactPowerShellFailure(details.String())
		if message == "" {
			message = err.Error()
		}
		return fmt.Errorf("Emby 网页卡片安装失败：%s", message)
	}
	fmt.Fprintln(w, "✅ 网页卡片集成已就绪；运行状态仍由可视化 Manager 统一控制。")
	return nil
}

func compactPowerShellFailure(output string) string {
	lines := strings.Split(strings.ReplaceAll(output, "\r\n", "\n"), "\n")
	meaningful := make([]string, 0, 4)
	for _, line := range lines {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "+ ") || strings.HasPrefix(line, "At ") ||
			strings.HasPrefix(line, "CategoryInfo") || strings.HasPrefix(line, "FullyQualifiedErrorId") {
			continue
		}
		meaningful = append(meaningful, line)
	}
	if len(meaningful) > 4 {
		meaningful = meaningful[len(meaningful)-4:]
	}
	message := strings.Join(meaningful, "；")
	if len(message) > 1200 {
		message = message[len(message)-1200:]
	}
	return message
}

func platformAutoStartEnabled() bool {
	out, err := commandOutput(
		"reg.exe", "query",
		`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`,
		"/v", winRunValueName,
	)
	if err != nil {
		return false
	}
	exe := installedExePath()
	if exe == "" {
		return false
	}
	expected := strings.ToLower(fmt.Sprintf(`"%s" --login-startup`, exe))
	return strings.Contains(strings.ToLower(out), expected)
}

func platformSetAutoStart(enabled bool, w io.Writer) error {
	if enabled {
		exe := installedExePath()
		if exe == "" {
			return fmt.Errorf("无法确定当前程序路径")
		}
		value := fmt.Sprintf(`"%s" --login-startup`, exe)
		out, err := commandOutput(
			"reg.exe", "add",
			`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`,
			"/v", winRunValueName, "/t", "REG_SZ", "/d", value, "/f",
		)
		if err != nil {
			return fmt.Errorf("启用 Windows 登录自启动失败：%v %s", err, strings.TrimSpace(out))
		}
		if !platformAutoStartEnabled() {
			return fmt.Errorf("Windows 登录自启动写入后校验失败")
		}
		_, _ = commandOutput("reg.exe", "delete", `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, "/v", legacyWinRunValueName, "/f")
		if w != nil {
			fmt.Fprintln(w, "✅ 已启用 Windows 登录后启动应用。")
		}
		return nil
	}

	_, _ = commandOutput(
		"reg.exe", "delete",
		`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`,
		"/v", winRunValueName, "/f",
	)
	_, _ = commandOutput(
		"reg.exe", "delete",
		`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`,
		"/v", legacyWinRunValueName, "/f",
	)
	if platformAutoStartEnabled() {
		return fmt.Errorf("关闭 Windows 登录自启动后校验失败")
	}
	if w != nil {
		fmt.Fprintln(w, "✅ 已关闭 Windows 登录后启动应用。")
	}
	return nil
}

func platformStopAgentProcessOnly(w io.Writer) error {
	pid := readAgentPID()
	if pid > 0 {
		// PID files can outlive their process and Windows may recycle the PID.
		// Only terminate it after re-resolving the process as an owned legacy
		// Manager executable. Otherwise the file is treated as stale metadata.
		for _, process := range platformFindLegacyManagerProcesses() {
			if process.PID == pid {
				if err := platformStopLegacyProcesses([]LegacyProcess{process}, w); err != nil {
					return err
				}
				break
			}
		}
	}
	_ = os.Remove(agentHeartbeatPath())
	_ = os.Remove(agentPIDPath())
	if w != nil {
		fmt.Fprintln(w, "✅ 已停止旧版 Agent。")
	}
	return nil
}

func platformDisableIntegration(ctx context.Context, w io.Writer) error {
	_ = platformStopAgentProcessOnly(w)
	if err := platformSetAutoStart(false, w); err != nil {
		return err
	}
	set := loadSettings()
	set.AutoStart = false
	set.AutoStartConfigured = true
	if err := saveSettings(set); err != nil {
		return err
	}
	if err := platformRunEngine(ctx, "disable-integration", "", w); err != nil {
		return err
	}
	fmt.Fprintln(w, "✅ 已彻底停用技术规格集成并恢复原生 Emby；媒体 NFO 未被修改。")
	return nil
}

func platformMigrateLegacy(ctx context.Context, w io.Writer) error {
	report := platformDetectLegacy()
	if report.UnsafePatch {
		return fmt.Errorf("检测到无法确认所有权的 Emby 网页修改，已安全停止；不会覆盖 index.html")
	}
	if !report.Required {
		fmt.Fprintln(w, "✅ 未发现需要迁移的旧版组件。")
		return nil
	}
	// The confirmation dialog lists these exact process paths and PIDs. Stop
	// every listed legacy Manager instance before removing its launch entries or
	// replacing the Web Patch; each PID/path pair is revalidated immediately
	// before termination by platformStopLegacyProcesses.
	if err := platformStopLegacyProcesses(report.Processes, w); err != nil {
		return err
	}
	if err := platformStopAgentProcessOnly(w); err != nil {
		return err
	}
	if err := platformSetAutoStart(false, w); err != nil {
		return err
	}
	if _, err := platformCleanupLegacy(w); err != nil {
		return err
	}
	if err := platformInstall(ctx, w); err != nil {
		return err
	}
	fmt.Fprintln(w, "✅ 旧版组件已迁移；新版服务可以安全启动。")
	return nil
}

func platformAgentAlreadyRunning() bool {
	pid := readAgentPID()
	if pid <= 0 {
		return false
	}
	out, err := commandOutput("tasklist.exe", "/FI", fmt.Sprintf("PID eq %d", pid), "/FO", "CSV", "/NH")
	return err == nil && strings.Contains(out, fmt.Sprintf(`"%d"`, pid))
}

func platformDetectLegacy() LegacyReport {
	report := LegacyReport{}
	add := func(value string) {
		for _, existing := range report.Items {
			if existing == value {
				return
			}
		}
		report.Items = append(report.Items, value)
	}

	if platformAgentAlreadyRunning() {
		report.AgentRunning = true
		add("旧版后台 Agent 正在运行")
	} else if _, err := os.Stat(agentPIDPath()); err == nil {
		report.Artifacts = true
		add("旧版 Agent 状态文件")
	}
	for _, process := range platformFindLegacyManagerProcesses() {
		report.Processes = append(report.Processes, process)
		add(fmt.Sprintf("旧版程序 PID %d：%s", process.PID, process.Path))
	}
	if platformAutoStartEnabled() {
		report.AutoStart = true
		add("旧版登录启动项")
	}
	if out, err := commandOutput("schtasks.exe", "/Query", "/TN", oldWinTaskName); err == nil && strings.TrimSpace(out) != "" {
		report.ScheduledTask = true
		add("旧版计划任务: " + oldWinTaskName)
	}

	if b, err := os.ReadFile(indexHTML()); err == nil {
		html := string(b)
		const patchBegin = "<!-- IMDbTechManager WebPatch BEGIN -->"
		const patchEnd = "<!-- IMDbTechManager WebPatch END -->"
		beginCount := strings.Count(html, patchBegin)
		endCount := strings.Count(html, patchEnd)
		ownedScript := regexp.MustCompile(`(?is)<script\b[^>]*\bsrc=["']technical-specs-card\.js\?v=([0-9.]+)["'][^>]*>\s*</script>`)
		scriptMatches := ownedScript.FindAllStringSubmatch(html, -1)
		hasReference := len(scriptMatches) > 0 || strings.Contains(strings.ToLower(html), "technical-specs-card.js")
		if hasReference || beginCount > 0 || endCount > 0 {
			strictOwned := false
			if beginCount == 1 && endCount == 1 && len(scriptMatches) == 1 {
				beginAt := strings.Index(html, patchBegin)
				endAt := strings.Index(html, patchEnd)
				if beginAt >= 0 && endAt > beginAt+len(patchBegin) {
					inside := html[beginAt+len(patchBegin) : endAt]
					insideScripts := ownedScript.FindAllStringSubmatch(inside, -1)
					withoutOwnedScript := ownedScript.ReplaceAllString(inside, "")
					strictOwned = len(insideScripts) == 1 && strings.TrimSpace(withoutOwnedScript) == ""
				}
			}
			if !strictOwned {
				report.UnsafePatch = true
				report.WebPatch = true
				add("无法确认所有权的网页补丁（标记、脚本数量或块内容异常）")
			} else if scriptMatches[0][1] != expectedWebCardVersion {
				report.WebPatch = true
				add("旧版网页卡片 v" + scriptMatches[0][1])
			}
		}
	}

	report.Required = report.AgentRunning || len(report.Processes) > 0 || report.AutoStart || report.ScheduledTask || report.Artifacts || report.WebPatch || report.UnsafePatch
	return report
}

func platformFindLegacyManagerProcesses() []LegacyProcess {
	script := `$self = ` + fmt.Sprintf("%d", os.Getpid()) + `; Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object { $_.ProcessId -ne $self -and $_.Name -like 'IMDbTechManager*.exe' } | Select-Object ProcessId,ExecutablePath,CommandLine | ConvertTo-Json -Compress`
	out, err := commandOutput("powershell.exe", "-NoProfile", "-Command", script)
	if err != nil || strings.TrimSpace(out) == "" {
		return nil
	}
	var rows []struct {
		ProcessID      int    `json:"ProcessId"`
		ExecutablePath string `json:"ExecutablePath"`
		CommandLine    string `json:"CommandLine"`
	}
	trimmed := strings.TrimSpace(out)
	if strings.HasPrefix(trimmed, "{") {
		trimmed = "[" + trimmed + "]"
	}
	if json.Unmarshal([]byte(trimmed), &rows) != nil {
		return nil
	}
	selfPath := strings.ToLower(filepath.Clean(installedExePath()))
	result := make([]LegacyProcess, 0, len(rows))
	for _, row := range rows {
		if row.ProcessID <= 0 || strings.TrimSpace(row.ExecutablePath) == "" {
			continue
		}
		path := filepath.Clean(row.ExecutablePath)
		commandLine := strings.ToLower(row.CommandLine)
		if strings.ToLower(path) == selfPath && !strings.Contains(commandLine, "--agent") {
			// This is the current visual Manager process that owns the migration dialog.
			continue
		}
		result = append(result, LegacyProcess{PID: row.ProcessID, Path: path, CommandLine: row.CommandLine})
	}
	return result
}

func platformStopLegacyProcesses(processes []LegacyProcess, w io.Writer) error {
	for _, process := range processes {
		if process.PID <= 0 || strings.TrimSpace(process.Path) == "" {
			continue
		}
		// Re-resolve the exact PID before touching it. A recycled PID or a process
		// whose executable path changed is never considered Manager-owned.
		current := platformFindLegacyManagerProcesses()
		owned := false
		for _, candidate := range current {
			if candidate.PID == process.PID && strings.EqualFold(filepath.Clean(candidate.Path), filepath.Clean(process.Path)) {
				owned = true
				break
			}
		}
		if !owned {
			continue
		}
		_, _ = commandOutput("taskkill.exe", "/PID", fmt.Sprintf("%d", process.PID), "/T")
		deadline := time.Now().Add(2 * time.Second)
		for time.Now().Before(deadline) {
			if !processPIDExists(process.PID) {
				break
			}
			time.Sleep(100 * time.Millisecond)
		}
		if processPIDExists(process.PID) {
			_, _ = commandOutput("taskkill.exe", "/PID", fmt.Sprintf("%d", process.PID), "/T", "/F")
		}
		if processPIDExists(process.PID) {
			return fmt.Errorf("旧版程序未能退出：PID %d，%s", process.PID, process.Path)
		}
		if w != nil {
			fmt.Fprintf(w, "✅ 已停止旧版程序 PID %d：%s\n", process.PID, process.Path)
		}
	}
	return nil
}

func processPIDExists(pid int) bool {
	if pid <= 0 {
		return false
	}
	out, err := commandOutput("tasklist.exe", "/FI", fmt.Sprintf("PID eq %d", pid), "/FO", "CSV", "/NH")
	return err == nil && strings.Contains(out, fmt.Sprintf(`"%d"`, pid))
}

func platformWriteCardRuntime(state CardRuntimeState) error {
	dashboard := filepath.Dir(cardRuntimeFile())
	if info, err := os.Stat(dashboard); err != nil || !info.IsDir() {
		if err == nil {
			err = fmt.Errorf("不是目录")
		}
		return fmt.Errorf("Emby dashboard-ui 不可用，无法更新卡片运行许可：%s：%w", dashboard, err)
	}
	b, err := json.MarshalIndent(state, "", "  ")
	if err != nil {
		return err
	}
	b = append(b, '\n')
	if err := atomicWrite(cardRuntimeFile(), b, 0644); err != nil {
		return err
	}
	if err := platformPublishWebCardLanguagePacks(); err != nil {
		appendManagerLog("web-card language publication: " + err.Error())
	}
	return nil
}

func platformPublishWebCardLanguagePacks() error {
	if info, err := os.Stat(liveWebCardJS()); errors.Is(err, os.ErrNotExist) || (err == nil && !info.Mode().IsRegular()) {
		return nil
	} else if err != nil {
		return err
	}
	b, err := json.MarshalIndent(webCardLanguagePackPayload(), "", "  ")
	if err != nil {
		return err
	}
	return atomicWrite(webCardLanguageFile(), append(b, '\n'), 0644)
}

func platformCleanupLegacy(w io.Writer) ([]string, error) {
	removed := []string{}

	// Legacy scheduled task: best-effort because deleting a task created with
	// Highest privileges can require elevation. ManagedInstall repeats this
	// cleanup when elevated.
	_, _ = commandOutput("schtasks.exe", "/End", "/TN", oldWinTaskName)
	if out, err := commandOutput("schtasks.exe", "/Delete", "/TN", oldWinTaskName, "/F"); err == nil {
		removed = append(removed, "计划任务: "+oldWinTaskName)
	} else if w != nil && out != "" && !strings.Contains(strings.ToLower(out), "cannot find") && !strings.Contains(out, "找不到") {
		fmt.Fprintln(w, "旧计划任务清理提示：", out)
	}

	patterns := []string{
		"technical-specs-worker-v*.ps1",
		"state-v*.json",
		"items-cache-v*.json",
		"root-discovery-v*.json",
		"root-state-v*.json",
		"xml-errors-v*.json",
		"library-roots-v*.json",
		"tech-specs-indexer.ps1",
	}
	for _, pattern := range patterns {
		matches, _ := filepath.Glob(filepath.Join(platformDataPath(), pattern))
		for _, p := range matches {
			if err := os.Remove(p); err == nil {
				removed = append(removed, p)
			}
		}
	}

	for _, name := range []string{"state.json", "items-cache.json", "root-discovery.json", "root-state.json", "xml-errors.json"} {
		p := filepath.Join(platformDataPath(), name)
		if err := os.Remove(p); err == nil {
			removed = append(removed, p)
		}
	}

	if w != nil {
		if len(removed) == 0 {
			fmt.Fprintln(w, "✅ 未发现需要清理的 Windows 旧版残留。")
		} else {
			fmt.Fprintf(w, "✅ 已清理 %d 项 Windows 旧版残留。\n", len(removed))
		}
	}
	return removed, nil
}

func platformRunEngine(ctx context.Context, action, arg string, w io.Writer) error {
	if _, err := os.Stat(enginePath()); err != nil {
		return fmt.Errorf("Manager 引擎文件不存在，请重新启动应用或点击“启用 / 修复集成”")
	}

	switch action {
	case "auto", "run":
		args := []string{
			"-NoProfile", "-ExecutionPolicy", "Bypass", "-File", enginePath(),
			"-IndexOnly", "-RootConfigPath", settingsPath(), "-BackupRootPath", webPatchBackupRoot(),
			"-OutputLanguage", engineOutputLanguageForWriter(w),
		}
		if strings.TrimSpace(arg) != "" {
			args = append(args, "-OnlyRoot", arg)
		}
		return runCommandToWriterContext(ctx, w, "powershell.exe", args...)
	case "repair-web":
		return runCommandToWriterContext(
			ctx,
			w,
			"powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass",
			"-File", enginePath(), "-RepairWebOnly", "-BackupRootPath", webPatchBackupRoot(),
			"-OutputLanguage", engineOutputLanguageForWriter(w),
		)
	case "disable-integration":
		return runCommandToWriterContext(
			ctx,
			w,
			"powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass",
			"-File", enginePath(), "-DisableIntegration", "-BackupRootPath", webPatchBackupRoot(),
			"-OutputLanguage", engineOutputLanguageForWriter(w),
		)
	case "diagnose":
		return runCommandToWriterContext(
			ctx,
			w,
			"powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass",
			"-File", enginePath(), "-CheckOnly", "-RootConfigPath", settingsPath(),
			"-BackupRootPath", webPatchBackupRoot(), "-OutputLanguage", engineOutputLanguageForWriter(w),
		)
	case "rebuild-index":
		return runCommandToWriterContext(
			ctx,
			w,
			"powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass",
			"-File", enginePath(), "-RebuildIndexOnly", "-RootConfigPath", settingsPath(),
			"-BackupRootPath", webPatchBackupRoot(), "-OutputLanguage", engineOutputLanguageForWriter(w),
		)
	case "discover-roots":
		return runCommandToWriterContext(
			ctx,
			w,
			"powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass",
			"-File", enginePath(), "-DiscoverOnly", "-RootConfigPath", settingsPath(),
			"-BackupRootPath", webPatchBackupRoot(), "-OutputLanguage", engineOutputLanguageForWriter(w),
		)
	default:
		return fmt.Errorf("Windows 不支持操作：%s", action)
	}
}

func platformChooseFolder() (string, error) {
	description := currentNativeLocalized("选择电影或电视剧媒体目录", "Select a Movie or TV Show media folder")
	description = strings.ReplaceAll(description, "'", "''")
	script := `[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false); Add-Type -AssemblyName System.Windows.Forms; $dialog = New-Object System.Windows.Forms.FolderBrowserDialog; $dialog.Description = '` + description + `'; $dialog.ShowNewFolderButton = $false; if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) { [Console]::Write($dialog.SelectedPath) }`
	out, err := commandOutput("powershell.exe", "-NoProfile", "-STA", "-Command", script)
	if err != nil {
		return "", fmt.Errorf("打开目录选择窗口失败：%w", err)
	}
	path := strings.TrimSpace(out)
	if path == "" {
		return "", fmt.Errorf("未选择目录")
	}
	return path, nil
}

func loadDiscoveredLibraries() []LibraryInfo {
	b, err := os.ReadFile(rootDiscoveryFile())
	if err != nil {
		return nil
	}
	var snapshot struct {
		Libraries []struct {
			Name, Path, Kind, Evidence string
			Included, Online           bool
		} `json:"libraries"`
	}
	if json.Unmarshal(b, &snapshot) != nil {
		return nil
	}
	result := make([]LibraryInfo, 0, len(snapshot.Libraries))
	for _, root := range snapshot.Libraries {
		if !root.Included || strings.TrimSpace(root.Path) == "" {
			continue
		}
		result = append(result, LibraryInfo{
			Name: root.Name, Path: root.Path, Kind: root.Kind, Online: root.Online,
			State: "discovered", Evidence: root.Evidence,
		})
	}
	return result
}

func collectStatus() (Status, error) {
	s := loadSettings()
	st := Status{
		AppVersion: appVersion, Platform: "windows", PlatformLabel: "Windows / Emby Server",
		IntervalSeconds: s.IntervalSeconds,
		RootsConfigured: s.RootsConfigured,
		ConfiguredRoots: append([]LibraryRoot(nil), s.LibraryRoots...),
		Capabilities: map[string]bool{
			"service_start": true, "service_stop": true, "run": true,
			"repair_web": true, "rebuild_index": true, "diagnose": true,
			"export": true, "settings": true, "migrate_legacy": true,
			"disable_integration": true,
		},
		Notes: []string{
			"服务由可视化 Manager 主进程拥有；最小化继续运行，关闭窗口停止全部服务。",
			"停止服务会失效运行许可，Emby 页面随后撤下本程序拥有的技术规格卡片。",
			"网页卡片 v4.1.0：后台索引永不修改 Emby index.html，网页维护仍需用户明确确认。",
		},
		Paths: map[string]string{
			"emby_root":       embyRoot(),
			"tech_data":       techDataFile(),
			"index_summary":   indexSummaryFile(),
			"manager_catalog": managerCatalogFile(),
			"xml_errors":      xmlErrorFile(),
			"dashboard_index": indexHTML(),
		},
		Extra: map[string]interface{}{},
	}
	_, st.Installed = statOK(installedExePath())
	_, st.EngineReady = statOK(enginePath())
	service := services.snapshot()
	st.Service = service
	st.AgentRunning = service.Running

	if b, err := os.ReadFile(indexSummaryFile()); err == nil {
		var data struct {
			GeneratedAt   string `json:"generatedAt"`
			IndexedTitles int    `json:"indexedTitles"`
			CatalogCount  int    `json:"catalogCount"`
			Libraries     []struct {
				Name, Path, Kind, Evidence string
				Online                     bool
			} `json:"libraries"`
			ScanStats struct {
				OnlineRootsScanned          int `json:"onlineRootsScanned"`
				NFOSeen                     int `json:"nfoSeen"`
				NFOReparsed                 int `json:"nfoReparsed"`
				TechnicalSpecsFound         int `json:"technicalSpecsFound"`
				WebEligibleSpecsFound       int `json:"webEligibleSpecsFound"`
				EpisodeSpecsExcludedFromWeb int `json:"episodeSpecsExcludedFromWeb"`
				XmlReadErrors               int `json:"xmlReadErrors"`
			} `json:"scanStats"`
		}
		if err := json.Unmarshal(b, &data); err == nil {
			st.IndexedTitles = data.IndexedTitles
			st.NFOTotal = data.CatalogCount
			st.XmlErrors = data.ScanStats.XmlReadErrors
			for _, l := range data.Libraries {
				st.Libraries = append(st.Libraries, LibraryInfo{
					Name: l.Name, Path: l.Path, Kind: l.Kind, Online: l.Online, Evidence: l.Evidence,
				})
			}
			st.Extra["generated_at"] = data.GeneratedAt
			st.Extra["scan_stats"] = map[string]int{
				"online_roots":                    data.ScanStats.OnlineRootsScanned,
				"nfo_seen":                        data.ScanStats.NFOSeen,
				"nfo_reparsed":                    data.ScanStats.NFOReparsed,
				"technical_specs_found":           data.ScanStats.TechnicalSpecsFound,
				"web_eligible_specs_found":        data.ScanStats.WebEligibleSpecsFound,
				"episode_specs_excluded_from_web": data.ScanStats.EpisodeSpecsExcludedFromWeb,
				"xml_read_errors":                 data.ScanStats.XmlReadErrors,
			}
		} else {
			st.Extra["index_summary_error"] = "Manager 索引摘要损坏：" + err.Error()
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		st.Extra["index_summary_error"] = "Manager 索引摘要读取失败：" + err.Error()
	} else {
		st.Extra["index_summary_error"] = "Manager 索引摘要尚未生成"
	}

	if b, err := os.ReadFile(xmlErrorFile()); err == nil {
		var v struct {
			GeneratedAt string         `json:"generatedAt"`
			Count       int            `json:"count"`
			Errors      []XmlErrorInfo `json:"errors"`
		}
		if json.Unmarshal(b, &v) == nil {
			st.XmlErrors = v.Count
			if len(v.Errors) > 200 {
				st.XmlErrorDetails = v.Errors[:200]
			} else {
				st.XmlErrorDetails = v.Errors
			}
			st.Extra["xml_errors_generated_at"] = v.GeneratedAt
		}
	}

	_, embyIndexExists := statOK(indexHTML())
	st.Extra["emby_detected"] = embyIndexExists
	indexInjected := false
	if b, err := os.ReadFile(indexHTML()); err == nil {
		html := string(b)
		re := regexp.MustCompile(`technical-specs-card\.js\?v=([0-9.]+)`)
		if m := re.FindStringSubmatch(html); len(m) == 2 {
			st.WebVersion = m[1]
			indexInjected = m[1] == expectedWebCardVersion &&
				strings.Count(html, "<!-- IMDbTechManager WebPatch BEGIN -->") == 1 &&
				strings.Count(html, "<!-- IMDbTechManager WebPatch END -->") == 1
		}
	}
	st.WebPatch = indexInjected

	liveJSExists := false
	liveJSVersion := ""
	liveJSMatches := false
	if b, err := os.ReadFile(liveWebCardJS()); err == nil {
		liveJSExists = true
		re := regexp.MustCompile(`WEB_CARD_VERSION\s*=\s*["']([0-9.]+)["']`)
		if m := re.FindStringSubmatch(string(b)); len(m) == 2 {
			liveJSVersion = m[1]
		}
		if bundled, readErr := assets.ReadFile("engine/technical-specs-card.js"); readErr == nil {
			liveJSMatches = bytes.Equal(b, bundled)
		}
	}
	dataExists := false
	dataValid := false
	if b, err := os.ReadFile(techDataFile()); err == nil {
		dataExists = true
		var data struct {
			Version   int                        `json:"version"`
			Items     map[string]json.RawMessage `json:"items"`
			ItemTypes map[string]string          `json:"itemTypes"`
		}
		dataValid = json.Unmarshal(b, &data) == nil && data.Version == 7 &&
			data.Items != nil && data.ItemTypes != nil
	}
	dataCurrent := dataValid && derivedIndexesValidForSettings(s)
	st.Extra["expected_web_card_version"] = expectedWebCardVersion
	st.Extra["web_index_injected"] = indexInjected
	st.Extra["web_js_exists"] = liveJSExists
	st.Extra["web_js_version"] = liveJSVersion
	st.Extra["web_js_matches"] = liveJSVersion == expectedWebCardVersion && liveJSMatches
	st.Extra["web_setup_complete"] = indexInjected && liveJSExists && liveJSVersion == expectedWebCardVersion && liveJSMatches
	st.Extra["web_data_exists"] = dataExists
	st.Extra["web_data_valid"] = dataValid
	st.Extra["web_data_current"] = dataCurrent
	st.Extra["web_healthy"] = indexInjected && liveJSExists && liveJSVersion == expectedWebCardVersion && liveJSMatches && dataCurrent
	runtimeValid := false
	if b, err := os.ReadFile(cardRuntimeFile()); err == nil {
		var runtimeState CardRuntimeState
		if json.Unmarshal(b, &runtimeState) == nil {
			expiresAt, parseErr := time.Parse(time.RFC3339Nano, runtimeState.ExpiresAt)
			runtimeValid = parseErr == nil && service.Running && runtimeState.Enabled &&
				runtimeState.WebCardVersion == expectedWebCardVersion && expiresAt.After(time.Now().UTC())
		}
	}
	st.Extra["card_runtime_valid"] = runtimeValid
	st.Paths["card_runtime"] = cardRuntimeFile()
	st.Paths["web_card_js"] = liveWebCardJS()
	st.Paths["web_card_languages"] = webCardLanguageFile()
	return st, nil
}

func statOK(p string) (os.FileInfo, bool) { i, e := os.Stat(p); return i, e == nil }

func heartbeatStatus() (bool, string) {
	b, err := os.ReadFile(agentHeartbeatPath())
	if err != nil {
		return false, ""
	}
	t, err := time.Parse(time.RFC3339, strings.TrimSpace(string(b)))
	if err != nil {
		return false, ""
	}
	return time.Since(t) < 150*time.Second, t.Local().Format("2006-01-02 15:04:05")
}

type jobObjectBasicLimitInformation struct {
	PerProcessUserTimeLimit int64
	PerJobUserTimeLimit     int64
	LimitFlags              uint32
	MinimumWorkingSetSize   uintptr
	MaximumWorkingSetSize   uintptr
	ActiveProcessLimit      uint32
	Affinity                uintptr
	PriorityClass           uint32
	SchedulingClass         uint32
}

type ioCounters struct {
	ReadOperationCount  uint64
	WriteOperationCount uint64
	OtherOperationCount uint64
	ReadTransferCount   uint64
	WriteTransferCount  uint64
	OtherTransferCount  uint64
}

type jobObjectExtendedLimitInformation struct {
	BasicLimitInformation jobObjectBasicLimitInformation
	IoInfo                ioCounters
	ProcessMemoryLimit    uintptr
	JobMemoryLimit        uintptr
	PeakProcessMemoryUsed uintptr
	PeakJobMemoryUsed     uintptr
}

type jobObjectBasicAccountingInformation struct {
	TotalUserTime             int64
	TotalKernelTime           int64
	ThisPeriodTotalUserTime   int64
	ThisPeriodTotalKernelTime int64
	TotalPageFaultCount       uint32
	TotalProcesses            uint32
	ActiveProcesses           uint32
	TotalTerminatedProcesses  uint32
}

var appJobAPI = struct {
	createJob   *syscall.LazyProc
	setInfo     *syscall.LazyProc
	assign      *syscall.LazyProc
	terminate   *syscall.LazyProc
	openProcess *syscall.LazyProc
	isInJob     *syscall.LazyProc
	queryJob    *syscall.LazyProc
	resume      *syscall.LazyProc
	closeHandle *syscall.LazyProc
}{
	createJob:   syscall.NewLazyDLL("kernel32.dll").NewProc("CreateJobObjectW"),
	setInfo:     syscall.NewLazyDLL("kernel32.dll").NewProc("SetInformationJobObject"),
	assign:      syscall.NewLazyDLL("kernel32.dll").NewProc("AssignProcessToJobObject"),
	terminate:   syscall.NewLazyDLL("kernel32.dll").NewProc("TerminateJobObject"),
	openProcess: syscall.NewLazyDLL("kernel32.dll").NewProc("OpenProcess"),
	isInJob:     syscall.NewLazyDLL("kernel32.dll").NewProc("IsProcessInJob"),
	queryJob:    syscall.NewLazyDLL("kernel32.dll").NewProc("QueryInformationJobObject"),
	resume:      syscall.NewLazyDLL("ntdll.dll").NewProc("NtResumeProcess"),
	closeHandle: syscall.NewLazyDLL("kernel32.dll").NewProc("CloseHandle"),
}

const (
	jobObjectBasicAccountingInfo = 1
	jobObjectExtendedLimitInfo   = 9
	jobObjectLimitKillOnJobClose = 0x00002000
	processTerminate             = 0x0001
	processSuspendResume         = 0x0800
	processSetQuota              = 0x0100
	processQueryLimitedInfo      = 0x1000
	createSuspended              = 0x00000004
)

var appWindowProcess struct {
	sync.Mutex
	launcherPID int
	windowPID   int
	windowsHwnd uintptr
	done        chan struct{}
	job         uintptr
	profile     string
}

func createKillOnCloseJob() (uintptr, error) {
	job, _, callErr := appJobAPI.createJob.Call(0, 0)
	if job == 0 {
		return 0, fmt.Errorf("CreateJobObjectW: %v", callErr)
	}
	info := jobObjectExtendedLimitInformation{}
	info.BasicLimitInformation.LimitFlags = jobObjectLimitKillOnJobClose
	result, _, setErr := appJobAPI.setInfo.Call(
		job,
		jobObjectExtendedLimitInfo,
		uintptr(unsafe.Pointer(&info)),
		unsafe.Sizeof(info),
	)
	if result == 0 {
		appJobAPI.closeHandle.Call(job)
		return 0, fmt.Errorf("SetInformationJobObject: %v", setErr)
	}
	return job, nil
}

func assignProcessToAppJob(job uintptr, pid int) error {
	process, _, openErr := appJobAPI.openProcess.Call(
		processTerminate|processSetQuota|processQueryLimitedInfo,
		0,
		uintptr(pid),
	)
	if process == 0 {
		return fmt.Errorf("OpenProcess(%d): %v", pid, openErr)
	}
	defer appJobAPI.closeHandle.Call(process)
	if result, _, assignErr := appJobAPI.assign.Call(job, process); result == 0 {
		return fmt.Errorf("AssignProcessToJobObject(%d): %v", pid, assignErr)
	}
	return nil
}

func resumeOwnedProcess(pid int) error {
	process, _, openErr := appJobAPI.openProcess.Call(
		processSuspendResume|processQueryLimitedInfo,
		0,
		uintptr(pid),
	)
	if process == 0 {
		return fmt.Errorf("OpenProcess(%d) for resume: %v", pid, openErr)
	}
	defer appJobAPI.closeHandle.Call(process)
	// NtResumeProcess returns an NTSTATUS; zero is STATUS_SUCCESS. Starting
	// suspended closes the child-spawn gap between CreateProcess and Job
	// assignment, so every Edge process belongs to this Manager before it runs.
	if status, _, _ := appJobAPI.resume.Call(process); status != 0 {
		return fmt.Errorf("NtResumeProcess(%d): NTSTATUS 0x%x", pid, status)
	}
	return nil
}

func platformOwnsAppWindowPID(pid int) bool {
	if pid <= 0 {
		return false
	}
	appWindowProcess.Lock()
	job := appWindowProcess.job
	appWindowProcess.Unlock()
	if job == 0 {
		return false
	}
	process, _, _ := appJobAPI.openProcess.Call(processQueryLimitedInfo, 0, uintptr(pid))
	if process == 0 {
		return false
	}
	defer appJobAPI.closeHandle.Call(process)
	var belongs int32
	result, _, _ := appJobAPI.isInJob.Call(process, job, uintptr(unsafe.Pointer(&belongs)))
	return result != 0 && belongs != 0
}

func platformRecordAppWindow(hwnd uintptr, pid int) {
	if hwnd == 0 || !platformOwnsAppWindowPID(pid) {
		return
	}
	appWindowProcess.Lock()
	appWindowProcess.windowsHwnd = hwnd
	appWindowProcess.windowPID = pid
	appWindowProcess.Unlock()
}

func platformOwnedAppWindowHWND() uintptr {
	appWindowProcess.Lock()
	defer appWindowProcess.Unlock()
	return appWindowProcess.windowsHwnd
}

func openAppWindow(url string) error {
	candidates := []string{
		filepath.Join(os.Getenv("ProgramFiles(x86)"), "Microsoft", "Edge", "Application", "msedge.exe"),
		filepath.Join(os.Getenv("ProgramFiles"), "Microsoft", "Edge", "Application", "msedge.exe"),
		filepath.Join(os.Getenv("LOCALAPPDATA"), "Microsoft", "Edge", "Application", "msedge.exe"),
	}
	for _, p := range candidates {
		if _, err := os.Stat(p); err == nil {
			sessionsRoot := filepath.Join(portableRootDir(), "runtime", "edge-sessions")
			if err := os.MkdirAll(sessionsRoot, 0755); err != nil {
				return fmt.Errorf("无法创建 Portable 界面运行目录：%w", err)
			}
			profile, err := os.MkdirTemp(sessionsRoot, "session-")
			if err != nil {
				return fmt.Errorf("无法创建独立界面会话：%w", err)
			}
			job, err := createKillOnCloseJob()
			if err != nil {
				_ = os.RemoveAll(profile)
				return fmt.Errorf("无法建立界面进程所有权：%w", err)
			}
			cmd := hiddenCommand(
				p,
				"--app="+url,
				"--start-maximized",
				"--no-first-run",
				"--disable-background-mode",
				"--user-data-dir="+profile,
			)
			cmd.SysProcAttr.CreationFlags |= createSuspended
			if err := cmd.Start(); err != nil {
				appJobAPI.closeHandle.Call(job)
				_ = os.RemoveAll(profile)
				return err
			}
			if err := assignProcessToAppJob(job, cmd.Process.Pid); err != nil {
				_ = cmd.Process.Kill()
				_ = cmd.Wait()
				appJobAPI.closeHandle.Call(job)
				_ = os.RemoveAll(profile)
				return fmt.Errorf("无法接管 Edge 界面进程树：%w", err)
			}
			if err := resumeOwnedProcess(cmd.Process.Pid); err != nil {
				appJobAPI.terminate.Call(job, 1)
				appJobAPI.closeHandle.Call(job)
				_ = cmd.Wait()
				_ = os.RemoveAll(profile)
				return fmt.Errorf("无法启动已纳管的 Edge 界面：%w", err)
			}
			done := make(chan struct{})
			pid := cmd.Process.Pid
			appWindowProcess.Lock()
			appWindowProcess.launcherPID = pid
			appWindowProcess.windowPID = 0
			appWindowProcess.windowsHwnd = 0
			appWindowProcess.done = done
			appWindowProcess.job = job
			appWindowProcess.profile = profile
			appWindowProcess.Unlock()
			go func() {
				_ = cmd.Wait()
				close(done)
				// The launcher ending is not proof that Edge's app window ended.  The
				// Job, real HWND and real window PID remain authoritative until the
				// Manager's verified shutdown path clears the whole UI session.
			}()
			return nil
		}
	}
	return fmt.Errorf("找不到 Microsoft Edge，无法创建受 Manager 生命周期控制的可视化窗口")
}

func platformStopAppWindowProcess() error {
	appWindowProcess.Lock()
	launcherPID := appWindowProcess.launcherPID
	windowPID := appWindowProcess.windowPID
	hwnd := appWindowProcess.windowsHwnd
	done := appWindowProcess.done
	job := appWindowProcess.job
	profile := appWindowProcess.profile
	appWindowProcess.Unlock()
	if launcherPID <= 0 && windowPID <= 0 && hwnd == 0 && job == 0 {
		return nil
	}
	if hwnd != 0 && isOwnedManagerWindow(hwnd) {
		procShowWindow.Call(hwnd, swHide)
		procPostMessage.Call(hwnd, wmClose, 0, 0)
		deadline := time.Now().Add(1500 * time.Millisecond)
		for isWindow(hwnd) && time.Now().Before(deadline) {
			time.Sleep(50 * time.Millisecond)
		}
	}
	if job != 0 {
		// Terminate the exact owned Job, then close its KILL_ON_JOB_CLOSE handle.
		// This remains authoritative even when Edge's launcher PID already ended.
		if result, _, terminateErr := appJobAPI.terminate.Call(job, 0); result == 0 {
			return fmt.Errorf("TerminateJobObject: %v", terminateErr)
		}
		if result, _, closeErr := appJobAPI.closeHandle.Call(job); result == 0 {
			return fmt.Errorf("CloseHandle(Edge Job): %v", closeErr)
		}
		appWindowProcess.Lock()
		if appWindowProcess.job == job {
			appWindowProcess.job = 0
		}
		appWindowProcess.Unlock()
	}
	if done != nil {
		select {
		case <-done:
		case <-time.After(2 * time.Second):
		}
	}
	deadline := time.Now().Add(2 * time.Second)
	for hwnd != 0 && isWindow(hwnd) && time.Now().Before(deadline) {
		time.Sleep(50 * time.Millisecond)
	}
	if hwnd != 0 && isWindow(hwnd) {
		var currentPID uint32
		procGetWindowPID.Call(hwnd, uintptr(unsafe.Pointer(&currentPID)))
		// An HWND can be recycled after the owned Edge exits. Never treat a
		// different process now using that numeric handle as ours.
		if int(currentPID) == windowPID {
			return fmt.Errorf("Edge 管理界面仍未关闭，HWND=%d，PID=%d", hwnd, currentPID)
		}
	}
	appWindowProcess.Lock()
	if appWindowProcess.launcherPID == launcherPID && appWindowProcess.profile == profile {
		appWindowProcess.launcherPID = 0
		appWindowProcess.windowPID = 0
		appWindowProcess.windowsHwnd = 0
		appWindowProcess.done = nil
		appWindowProcess.job = 0
		appWindowProcess.profile = ""
	}
	appWindowProcess.Unlock()
	cleanupOwnedEdgeProfile(profile)
	return nil
}

func cleanupOwnedEdgeProfile(profile string) {
	if strings.TrimSpace(profile) == "" {
		return
	}
	root := filepath.Join(portableRootDir(), "runtime", "edge-sessions")
	rel, err := filepath.Rel(root, profile)
	if err != nil || rel == "." || strings.HasPrefix(rel, ".."+string(os.PathSeparator)) || filepath.IsAbs(rel) {
		appendManagerLog("refused unsafe Edge profile cleanup: " + profile)
		return
	}
	if err := os.RemoveAll(profile); err != nil {
		appendManagerLog("Edge profile cleanup: " + err.Error())
	}
}

func platformOwnedAppWindowPID() int {
	appWindowProcess.Lock()
	defer appWindowProcess.Unlock()
	if appWindowProcess.windowPID > 0 {
		return appWindowProcess.windowPID
	}
	return appWindowProcess.launcherPID
}

func platformAppWindowSessionActive() bool {
	appWindowProcess.Lock()
	defer appWindowProcess.Unlock()
	return appWindowProcess.job != 0 || appWindowProcess.launcherPID > 0 || appWindowProcess.windowsHwnd != 0
}

func platformAppWindowProcessesActive() bool {
	appWindowProcess.Lock()
	job := appWindowProcess.job
	appWindowProcess.Unlock()
	if job == 0 {
		return false
	}
	var info jobObjectBasicAccountingInformation
	result, _, _ := appJobAPI.queryJob.Call(
		job,
		jobObjectBasicAccountingInfo,
		uintptr(unsafe.Pointer(&info)),
		unsafe.Sizeof(info),
		0,
	)
	// A query failure is uncertain, not proof of exit. Keep the session alive
	// and let the explicit shutdown path report/handle the underlying failure.
	return result == 0 || info.ActiveProcesses > 0
}

func platformShowShutdownError(err error) {
	if err == nil {
		return
	}
	user32 := syscall.NewLazyDLL("user32.dll")
	messageBox := user32.NewProc("MessageBoxW")
	message, _ := syscall.UTF16PtrFromString(currentNativeLocalized("服务、界面或所属进程没有完全停止，程序将保持运行以便重试。", "The service, window, or owned processes did not stop completely. The app will remain open so you can retry.") + "\n\n" + localizeBackendText(currentLanguage(), err.Error()))
	title, _ := syscall.UTF16PtrFromString(currentNativeLocalized("Tech Card Manager 退出未完成", "Tech Card Manager did not exit completely"))
	const mbOKIconError = 0x00000010
	messageBox.Call(0, uintptr(unsafe.Pointer(message)), uintptr(unsafe.Pointer(title)), mbOKIconError)
}

func platformShowStartupError(err error) {
	if err == nil {
		return
	}
	user32 := syscall.NewLazyDLL("user32.dll")
	messageBox := user32.NewProc("MessageBoxW")
	message, _ := syscall.UTF16PtrFromString(currentNativeLocalized("无法打开 Tech Card Manager 可视化界面，程序不会在后台继续运行。", "Tech Card Manager could not open its window and will not continue running in the background.") + "\n\n" + localizeBackendText(currentLanguage(), err.Error()))
	title, _ := syscall.UTF16PtrFromString(currentNativeLocalized("Tech Card Manager 启动失败", "Tech Card Manager startup failed"))
	const mbOKIconError = 0x00000010
	messageBox.Call(0, uintptr(unsafe.Pointer(message)), uintptr(unsafe.Pointer(title)), mbOKIconError)
}

func platformConfirmExit(service ServiceSnapshot, jobRunning, maintenanceRunning, windowAlreadyClosed bool) bool {
	language := currentLanguage()
	message := localizedNative(language, "即将退出 Tech Card Manager。退出后将停止全部服务，并从当前打开的 Emby 页面撤下技术规格卡片。媒体文件和 NFO 不会被修改。", "Tech Card Manager is about to exit. All services will stop and the Technical Specs card will be removed from currently open Emby pages. Media files and NFO files will not be changed.")
	if service.State == serviceLegacyBlocked {
		message = localizedNative(language, "当前新版服务尚未启动。退出不会修改检测到的旧版组件；旧版仍可能继续控制 Emby 技术规格卡片。", "The new service has not started. Exiting will not change detected legacy components; a legacy component may continue to control the Emby Technical Specs card.")
	} else if service.State == serviceStopError {
		message = localizedNative(language, "上一次停止未能确认 Emby 撤卡状态。程序会再次尝试停止；如果仍失败，将保留界面并显示错误，不会伪装成已经退出。", "The last stop attempt could not confirm that the card was removed from Emby. The app will try again; if it still fails, the window will remain open and show the error.")
	} else if !service.Running {
		message = localizedNative(language, "Tech Card Manager 服务已经停止。退出后将关闭管理界面，Emby 不会显示由新版服务提供的技术规格卡片。", "The Tech Card Manager service is already stopped. Exiting will close the Manager, and Emby will not show a Technical Specs card supplied by the new service.")
	}
	if maintenanceRunning {
		message += localizedNative(language, "\n\n管理员维护事务正在执行。确认退出后，程序会先等待该事务安全结束，不会把提权进程留在后台。", "\n\nAn administrator maintenance transaction is running. After you confirm, the app will wait for it to finish safely and will not leave an elevated process behind.")
	} else if jobRunning {
		message += localizedNative(language, "\n\n当前媒体库检查将被安全取消。", "\n\nThe current media-library check will be cancelled safely.")
	}
	message += localizedNative(language, "\n\n是否退出？", "\n\nExit now?")
	user32 := syscall.NewLazyDLL("user32.dll")
	messageBox := user32.NewProc("MessageBoxW")
	text, _ := syscall.UTF16PtrFromString(message)
	title, _ := syscall.UTF16PtrFromString(localizedNative(language, "退出 Tech Card Manager？", "Exit Tech Card Manager?"))
	const (
		mbOKCancel      = 0x00000001
		mbIconWarn      = 0x00000030
		mbDefButton2    = 0x00000100
		mbSetForeground = 0x00010000
		mbTopmost       = 0x00040000
		idOK            = 1
	)
	owner := platformExitPromptOwner(windowAlreadyClosed)
	if owner != 0 {
		procSetForeground.Call(owner)
	}
	result, _, _ := messageBox.Call(
		owner,
		uintptr(unsafe.Pointer(text)),
		uintptr(unsafe.Pointer(title)),
		mbOKCancel|mbIconWarn|mbDefButton2|mbSetForeground|mbTopmost,
	)
	return result == idOK
}

func openPath(p string) error { return hiddenCommand("explorer.exe", p).Start() }

func platformDiagnosticPaths() []string {
	return []string{
		techDataFile(), webCardLanguageFile(), indexSummaryFile(), xmlErrorFile(), managerCatalogFile(), indexHTML(),
		filepath.Join(platformDataPath(), "manager-root-discovery.json"),
		filepath.Join(platformDataPath(), "manager-root-state.json"),
		filepath.Join(platformDataPath(), "manager-state.json"),
	}
}

type shellExecuteInfo struct {
	CbSize       uint32
	FMask        uint32
	Hwnd         uintptr
	LpVerb       *uint16
	LpFile       *uint16
	LpParameters *uint16
	LpDirectory  *uint16
	NShow        int32
	HInstApp     uintptr
	LpIDList     uintptr
	LpClass      *uint16
	HkeyClass    uintptr
	DwHotKey     uint32
	HMonitor     uintptr
	HProcess     uintptr
}

type elevatedActionEnvelope struct {
	RequestID string `json:"request_id"`
	Action    string `json:"action"`
	OK        bool   `json:"ok"`
	Message   string `json:"message,omitempty"`
	Time      string `json:"time"`
}

type elevatedPermit struct {
	RequestID string `json:"request_id"`
	Action    string `json:"action"`
	Allow     bool   `json:"allow"`
	Message   string `json:"message,omitempty"`
	Time      string `json:"time"`
}

func platformProcessIsElevated() (bool, error) {
	kernel32 := syscall.NewLazyDLL("kernel32.dll")
	advapi32 := syscall.NewLazyDLL("advapi32.dll")
	getCurrentProcess := kernel32.NewProc("GetCurrentProcess")
	closeHandle := kernel32.NewProc("CloseHandle")
	openProcessToken := advapi32.NewProc("OpenProcessToken")
	getTokenInformation := advapi32.NewProc("GetTokenInformation")

	const tokenQuery = 0x0008
	const tokenElevationClass = 20
	process, _, _ := getCurrentProcess.Call()
	var token uintptr
	ret, _, callErr := openProcessToken.Call(process, tokenQuery, uintptr(unsafe.Pointer(&token)))
	if ret == 0 {
		return false, fmt.Errorf("OpenProcessToken: %v", callErr)
	}
	defer closeHandle.Call(token)

	var elevation uint32
	var returned uint32
	ret, _, callErr = getTokenInformation.Call(
		token,
		tokenElevationClass,
		uintptr(unsafe.Pointer(&elevation)),
		unsafe.Sizeof(elevation),
		uintptr(unsafe.Pointer(&returned)),
	)
	if ret == 0 {
		return false, fmt.Errorf("GetTokenInformation(TokenElevation): %v", callErr)
	}
	return elevation != 0, nil
}

func requestElevatedAction(action, requestID string) (uintptr, error) {
	if !isPrivilegedAction(action) || !validElevatedRequestID(requestID) {
		return 0, fmt.Errorf("无效的管理员维护请求")
	}
	exe, err := os.Executable()
	if err != nil {
		return 0, err
	}
	shell32 := syscall.NewLazyDLL("shell32.dll")
	proc := shell32.NewProc("ShellExecuteExW")
	verb, _ := syscall.UTF16PtrFromString("runas")
	file, _ := syscall.UTF16PtrFromString(exe)
	params, _ := syscall.UTF16PtrFromString(
		"--headless-action " + action +
			" --elevated-request " + requestID +
			" --manager-pid " + strconv.Itoa(os.Getpid()),
	)
	directory, _ := syscall.UTF16PtrFromString(portableRootDir())
	info := shellExecuteInfo{
		FMask:        0x00000040, // SEE_MASK_NOCLOSEPROCESS
		LpVerb:       verb,
		LpFile:       file,
		LpParameters: params,
		LpDirectory:  directory,
		NShow:        0, // the owned helper has no user interface
	}
	info.CbSize = uint32(unsafe.Sizeof(info))
	ret, _, callErr := proc.Call(uintptr(unsafe.Pointer(&info)))
	if ret == 0 {
		if errno, ok := callErr.(syscall.Errno); ok && errno == 1223 {
			return 0, fmt.Errorf("用户取消了管理员权限请求，操作未执行")
		}
		return 0, fmt.Errorf("无法启动管理员维护进程：%v", callErr)
	}
	if info.HProcess == 0 {
		return 0, fmt.Errorf("Windows 没有返回管理员维护进程句柄，操作未执行")
	}
	return info.HProcess, nil
}

func validElevatedRequestID(requestID string) bool {
	if len(requestID) != 32 {
		return false
	}
	for _, ch := range requestID {
		if (ch < '0' || ch > '9') && (ch < 'a' || ch > 'f') {
			return false
		}
	}
	return true
}

func newElevatedRequestID() (string, error) {
	b := make([]byte, 16)
	if _, err := rand.Read(b); err != nil {
		return "", err
	}
	return hex.EncodeToString(b), nil
}

func elevatedRequestDir() string {
	return filepath.Join(baseDir(), "elevated-requests")
}

func elevatedReadyPath(requestID string) string {
	return filepath.Join(elevatedRequestDir(), requestID+".ready.json")
}

func elevatedPermitPath(requestID string) string {
	return filepath.Join(elevatedRequestDir(), requestID+".permit.json")
}

func elevatedResultPath(requestID string) string {
	return filepath.Join(elevatedRequestDir(), requestID+".result.json")
}

func writeElevatedJSON(path string, value interface{}) error {
	if err := os.MkdirAll(elevatedRequestDir(), 0755); err != nil {
		return err
	}
	b, err := json.MarshalIndent(value, "", "  ")
	if err != nil {
		return err
	}
	return atomicWrite(path, b, 0600)
}

func writeElevatedReady(requestID, action string) error {
	if !validElevatedRequestID(requestID) || !isPrivilegedAction(action) {
		return fmt.Errorf("无效的管理员维护握手")
	}
	return writeElevatedJSON(elevatedReadyPath(requestID), elevatedActionEnvelope{
		RequestID: requestID,
		Action:    action,
		OK:        true,
		Message:   "管理员进程已启动，等待主程序许可",
		Time:      time.Now().Format(time.RFC3339Nano),
	})
}

func writeElevatedPermit(requestID, action string, allow bool, message string) error {
	return writeElevatedJSON(elevatedPermitPath(requestID), elevatedPermit{
		RequestID: requestID,
		Action:    action,
		Allow:     allow,
		Message:   message,
		Time:      time.Now().Format(time.RFC3339Nano),
	})
}

func waitForElevatedPermit(requestID, action string, managerPID int, timeout time.Duration) (elevatedPermit, error) {
	kernel32 := syscall.NewLazyDLL("kernel32.dll")
	openProcess := kernel32.NewProc("OpenProcess")
	waitForSingleObject := kernel32.NewProc("WaitForSingleObject")
	closeHandle := kernel32.NewProc("CloseHandle")
	const synchronize = 0x00100000
	manager, _, callErr := openProcess.Call(synchronize, 0, uintptr(managerPID))
	if manager == 0 {
		return elevatedPermit{}, fmt.Errorf("无法确认主程序仍在运行，管理员操作没有执行：%v", callErr)
	}
	defer closeHandle.Call(manager)

	deadline := time.Now().Add(timeout)
	for time.Now().Before(deadline) {
		if wait, _, _ := waitForSingleObject.Call(manager, 0); wait == 0 {
			return elevatedPermit{}, fmt.Errorf("主程序已经退出，管理员操作没有执行")
		}
		b, err := os.ReadFile(elevatedPermitPath(requestID))
		if err == nil {
			var permit elevatedPermit
			if json.Unmarshal(b, &permit) == nil && permit.RequestID == requestID && permit.Action == action {
				_ = os.Remove(elevatedPermitPath(requestID))
				return permit, nil
			}
		}
		time.Sleep(100 * time.Millisecond)
	}
	return elevatedPermit{}, fmt.Errorf("等待主程序许可超时，管理员操作没有执行")
}

func writeElevatedResult(requestID, action string, ok bool, message string) error {
	return writeElevatedJSON(elevatedResultPath(requestID), elevatedActionEnvelope{
		RequestID: requestID,
		Action:    action,
		OK:        ok,
		Message:   message,
		Time:      time.Now().Format(time.RFC3339Nano),
	})
}

func waitForElevatedReady(process uintptr, requestID, action string) error {
	kernel32 := syscall.NewLazyDLL("kernel32.dll")
	waitForSingleObject := kernel32.NewProc("WaitForSingleObject")
	deadline := time.Now().Add(30 * time.Second)
	for time.Now().Before(deadline) {
		if b, err := os.ReadFile(elevatedReadyPath(requestID)); err == nil {
			var ready elevatedActionEnvelope
			if json.Unmarshal(b, &ready) == nil && ready.RequestID == requestID && ready.Action == action && ready.OK {
				return nil
			}
		}
		wait, _, callErr := waitForSingleObject.Call(process, 100)
		if wait == 0 {
			return fmt.Errorf("管理员维护进程在建立安全握手前退出")
		}
		if wait == 0xffffffff {
			return fmt.Errorf("等待管理员维护进程失败：%v", callErr)
		}
	}
	return fmt.Errorf("管理员维护进程启动超时，操作没有执行")
}

func waitForElevatedProcess(process uintptr) (uint32, error) {
	kernel32 := syscall.NewLazyDLL("kernel32.dll")
	waitForSingleObject := kernel32.NewProc("WaitForSingleObject")
	getExitCodeProcess := kernel32.NewProc("GetExitCodeProcess")
	for {
		wait, _, callErr := waitForSingleObject.Call(process, 250)
		switch wait {
		case 0:
			var exitCode uint32
			ret, _, exitErr := getExitCodeProcess.Call(process, uintptr(unsafe.Pointer(&exitCode)))
			if ret == 0 {
				return 0, fmt.Errorf("读取管理员维护进程退出状态失败：%v", exitErr)
			}
			return exitCode, nil
		case 0x00000102:
			continue
		case 0xffffffff:
			return 0, fmt.Errorf("等待管理员维护进程失败：%v", callErr)
		default:
			return 0, fmt.Errorf("管理员维护进程返回未知等待状态：0x%x", wait)
		}
	}
}

func platformRunElevatedAction(ctx context.Context, action string, beforeRun func() error, w io.Writer) error {
	select {
	case <-ctx.Done():
		return fmt.Errorf("管理员维护操作尚未开始：%w", ctx.Err())
	default:
	}
	requestID, err := newElevatedRequestID()
	if err != nil {
		return fmt.Errorf("无法创建管理员维护请求：%w", err)
	}
	paths := []string{elevatedReadyPath(requestID), elevatedPermitPath(requestID), elevatedResultPath(requestID)}
	defer func() {
		for _, path := range paths {
			_ = os.Remove(path)
		}
	}()

	process, err := requestElevatedAction(action, requestID)
	if err != nil {
		return err
	}
	kernel32 := syscall.NewLazyDLL("kernel32.dll")
	closeHandle := kernel32.NewProc("CloseHandle")
	terminateProcess := kernel32.NewProc("TerminateProcess")
	defer closeHandle.Call(process)

	if w != nil {
		fmt.Fprintln(w, "Windows 已启动受控管理员维护进程；正在建立安全握手…")
	}
	if err := waitForElevatedReady(process, requestID, action); err != nil {
		// No permit has been written, so the helper cannot have started an Emby
		// mutation. Reap the exact waiting process instead of leaving it behind.
		terminateProcess.Call(process, 5)
		_, _ = waitForElevatedProcess(process)
		return err
	}
	_ = os.Remove(elevatedReadyPath(requestID))

	if beforeRun != nil {
		if err := beforeRun(); err != nil {
			if permitErr := writeElevatedPermit(requestID, action, false, err.Error()); permitErr != nil {
				terminateProcess.Call(process, 5)
			}
			_, _ = waitForElevatedProcess(process)
			return err
		}
	}
	if err := writeElevatedPermit(requestID, action, true, "主程序已许可执行"); err != nil {
		// The helper has not been permitted to mutate anything yet, so terminating
		// this exact waiting process is safe and cannot interrupt an Emby write.
		terminateProcess.Call(process, 5)
		_, _ = waitForElevatedProcess(process)
		return fmt.Errorf("无法许可管理员维护进程，操作没有执行：%w", err)
	}
	if w != nil {
		fmt.Fprintln(w, "管理员权限已获得，正在执行并等待结果复核…")
	}
	exitCode, waitErr := waitForElevatedProcess(process)
	if waitErr != nil {
		return waitErr
	}
	b, readErr := os.ReadFile(elevatedResultPath(requestID))
	if readErr != nil {
		return fmt.Errorf("管理员维护进程已退出（代码 %d），但没有返回可验证结果：%w", exitCode, readErr)
	}
	var result elevatedActionEnvelope
	if err := json.Unmarshal(b, &result); err != nil {
		return fmt.Errorf("管理员维护结果损坏：%w", err)
	}
	if result.RequestID != requestID || result.Action != action {
		return fmt.Errorf("管理员维护结果与当前请求不匹配，已拒绝采用")
	}
	if !result.OK || exitCode != 0 {
		message := strings.TrimSpace(result.Message)
		if message == "" {
			message = fmt.Sprintf("管理员维护进程退出代码 %d", exitCode)
		}
		return errors.New(message)
	}
	return nil
}
