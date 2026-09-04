package main

import (
	"archive/zip"
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"sync"
	"time"
)

const (
	languageCatalogSchema = 1
	languagePackMaxBytes  = 8 << 20
	languagePackProduct   = "tcm"
	languagePackRepoPath  = "Eric-Hou1997/Tech-Card-Manager"
)

type LanguagePackDescriptor struct {
	Revision       int    `json:"revision"`
	ReleasedWith   string `json:"released_with"`
	Asset          string `json:"asset"`
	SHA256         string `json:"sha256"`
	CatalogSchema  int    `json:"catalog_schema"`
	MessageSetHash string `json:"message_set_hash"`
}

type LanguageCatalog struct {
	Schema     int                               `json:"schema"`
	Product    string                            `json:"product"`
	AppVersion string                            `json:"app_version"`
	Languages  map[string]LanguagePackDescriptor `json:"languages"`
}

type installedLanguageManifest struct {
	Schema         int               `json:"schema"`
	Product        string            `json:"product"`
	Locale         string            `json:"locale"`
	Revision       int               `json:"revision"`
	ReleasedWith   string            `json:"released_with"`
	CatalogSchema  int               `json:"catalog_schema"`
	MessageSetHash string            `json:"message_set_hash"`
	Files          map[string]string `json:"files"`
}

var embeddedLanguageCatalog = mustReadLanguageCatalog()
var languageCatalog = mustLoadLanguageCatalog(embeddedLanguageCatalog)
var languageCatalogHash = fmt.Sprintf("%x", sha256.Sum256(embeddedLanguageCatalog))

func mustReadLanguageCatalog() []byte {
	b, err := assets.ReadFile("language_catalog.json")
	if err != nil {
		panic("embedded language catalog is missing: " + err.Error())
	}
	return b
}

func mustLoadLanguageCatalog(b []byte) LanguageCatalog {
	var catalog LanguageCatalog
	versionPattern := regexp.MustCompile(`^v[0-9]+\.[0-9]+\.[0-9]+$`)
	if err := json.Unmarshal(b, &catalog); err != nil || catalog.Schema != languageCatalogSchema || catalog.Product != languagePackProduct || !versionPattern.MatchString(catalog.AppVersion) || len(catalog.Languages) == 0 {
		panic("embedded language catalog is invalid")
	}
	for language, descriptor := range catalog.Languages {
		expectedAsset := fmt.Sprintf("TCM-Language-%s-r%d.zip", language, descriptor.Revision)
		if _, known := languageByCode[language]; !known || descriptor.Revision < 1 || descriptor.CatalogSchema != catalog.Schema || !isSHA256(descriptor.SHA256) || !isSHA256(descriptor.MessageSetHash) || descriptor.Asset != expectedAsset || !versionPattern.MatchString(descriptor.ReleasedWith) {
			panic("embedded language catalog contains an invalid descriptor")
		}
	}
	return catalog
}

func isSHA256(value string) bool {
	decoded, err := hex.DecodeString(value)
	return err == nil && len(decoded) == sha256.Size
}

var (
	languagePackHTTPClient   = newLanguagePackHTTPClient()
	languagePackRootOverride string
	languagePackMu           sync.RWMutex
	languagePackErrors       = map[string]string{}
	languagePackCache        = map[string]map[string]map[string]string{}
	languagePackInstalling   = map[string]bool{}
	publishWebCardLanguages  = platformPublishWebCardLanguagePacks
)

func newLanguagePackHTTPClient() *http.Client {
	return &http.Client{
		Timeout: 90 * time.Second,
		CheckRedirect: func(request *http.Request, via []*http.Request) error {
			if len(via) >= 10 {
				return errors.New("语言包下载重定向次数过多")
			}
			host := strings.ToLower(request.URL.Hostname())
			if host != "github.com" && !strings.HasSuffix(host, ".githubusercontent.com") {
				return fmt.Errorf("语言包下载重定向到非官方主机 %s，已拒绝", host)
			}
			return nil
		},
	}
}

func languagePackRoot() string {
	if languagePackRootOverride != "" {
		return languagePackRootOverride
	}
	return filepath.Join(baseDir(), "language-packs")
}

func languageCatalogActive() bool {
	return languageCatalog.AppVersion == "v"+appVersion
}

func stableMessageID(english string) string {
	hash := uint64(14695981039346656037)
	for _, value := range []byte(strings.TrimSpace(english)) {
		hash ^= uint64(value)
		hash *= 1099511628211
	}
	return fmt.Sprintf("legacy.%016x", hash)
}

func recognizedLanguage(language string) (LanguageOption, bool) {
	option, ok := languageByCode[strings.TrimSpace(language)]
	return option, ok
}

func languagePackPath(language string, descriptor LanguagePackDescriptor) string {
	return filepath.Join(languagePackRoot(), language, fmt.Sprintf("r%d", descriptor.Revision))
}

func installedLanguagePack(language string) (installedLanguageManifest, string, bool) {
	descriptor, ok := languageCatalog.Languages[language]
	if !ok {
		return installedLanguageManifest{}, "", false
	}
	dir := languagePackPath(language, descriptor)
	dirInfo, err := os.Lstat(dir)
	if err != nil || !dirInfo.IsDir() || dirInfo.Mode()&os.ModeSymlink != 0 {
		return installedLanguageManifest{}, "", false
	}
	b, err := os.ReadFile(filepath.Join(dir, "manifest.json"))
	if err != nil {
		return installedLanguageManifest{}, "", false
	}
	var manifest installedLanguageManifest
	if json.Unmarshal(b, &manifest) != nil || manifest.Schema != languageCatalogSchema || manifest.Product != languageCatalog.Product || manifest.Locale != language || manifest.Revision != descriptor.Revision || manifest.ReleasedWith != descriptor.ReleasedWith || manifest.CatalogSchema != descriptor.CatalogSchema || manifest.MessageSetHash != descriptor.MessageSetHash || len(manifest.Files) != 5 {
		return installedLanguageManifest{}, "", false
	}
	for _, name := range []string{"web.json", "core.json", "engine.json", "native.json", "web-card.json"} {
		path := filepath.Join(dir, name)
		info, err := os.Lstat(path)
		if err != nil || !info.Mode().IsRegular() || info.Mode()&os.ModeSymlink != 0 || !validLanguagePackFileHash(path, manifest.Files[name]) {
			return installedLanguageManifest{}, "", false
		}
	}
	return manifest, dir, true
}

func validLanguagePackFileHash(path, expected string) bool {
	if len(expected) != 64 {
		return false
	}
	payload, err := os.ReadFile(path)
	if err != nil {
		return false
	}
	sum := sha256.Sum256(payload)
	return strings.EqualFold(hex.EncodeToString(sum[:]), expected)
}

func languagePackInstalled(language string) bool {
	_, _, ok := installedLanguagePack(language)
	return ok
}

func languagePackHistoryPresent(language string) bool {
	entries, err := os.ReadDir(filepath.Join(languagePackRoot(), language))
	if err != nil {
		return false
	}
	revisionDir := regexp.MustCompile(`^r[1-9][0-9]*$`)
	for _, entry := range entries {
		if entry.IsDir() && revisionDir.MatchString(entry.Name()) {
			return true
		}
	}
	return false
}

func languagePackIdentity(language string) (revision int, messageSetHash string) {
	option, ok := recognizedLanguage(language)
	if !ok || option.BuiltIn {
		return 0, "builtin-v1"
	}
	descriptor := languageCatalog.Languages[language]
	return descriptor.Revision, languageCatalogHash
}

func decorateLanguageOptions(options []LanguageOption) {
	languagePackMu.RLock()
	defer languagePackMu.RUnlock()
	for index := range options {
		option := &options[index]
		if option.BuiltIn {
			option.Installed, option.State = true, "built-in"
			continue
		}
		descriptor, published := languageCatalog.Languages[option.Code]
		published = published && languageCatalogActive()
		option.Downloadable, option.Revision, option.ReleasedWith = published, descriptor.Revision, descriptor.ReleasedWith
		option.Installed = languagePackInstalled(option.Code)
		if languagePackInstalling[option.Code] {
			option.State = "downloading"
		} else if option.Installed {
			option.State = "installed"
		} else if message := languagePackErrors[option.Code]; message != "" {
			option.State, option.Error = "failed", message
		} else {
			option.State = "not-installed"
		}
	}
}

func languagePackMessage(language, section, id string) (string, bool) {
	if _, _, ok := installedLanguagePack(language); !ok {
		return "", false
	}
	languagePackMu.RLock()
	sections, cached := languagePackCache[language]
	languagePackMu.RUnlock()
	if !cached {
		loaded := map[string]map[string]string{}
		_, dir, _ := installedLanguagePack(language)
		for _, name := range []string{"web", "core", "engine", "native", "web-card"} {
			var messages map[string]string
			b, err := os.ReadFile(filepath.Join(dir, name+".json"))
			if err != nil || json.Unmarshal(b, &messages) != nil {
				return "", false
			}
			loaded[name] = messages
		}
		languagePackMu.Lock()
		languagePackCache[language] = loaded
		sections = loaded
		languagePackMu.Unlock()
	}
	value, ok := sections[section][id]
	return value, ok && strings.TrimSpace(value) != ""
}

func languagePackDownloadURL(descriptor LanguagePackDescriptor) (string, error) {
	if descriptor.Revision < 1 || descriptor.ReleasedWith == "" || descriptor.Asset == "" {
		return "", errors.New("语言包目录记录不完整")
	}
	if !strings.HasPrefix(descriptor.Asset, "TCM-Language-") || !strings.HasSuffix(descriptor.Asset, ".zip") || strings.Contains(descriptor.Asset, "/") {
		return "", errors.New("语言包文件名无效")
	}
	return "https://github.com/" + languagePackRepoPath + "/releases/download/" + url.PathEscape(descriptor.ReleasedWith) + "/" + url.PathEscape(descriptor.Asset), nil
}

func installLanguagePack(language string) error {
	option, known := recognizedLanguage(language)
	if !known || option.BuiltIn {
		return errors.New("该语言不需要下载")
	}
	if !languageCatalogActive() {
		return errors.New("语言包目录不属于当前应用版本")
	}
	descriptor, ok := languageCatalog.Languages[language]
	if !ok {
		return errors.New("当前版本没有为该语言指定语言包")
	}
	if !isSHA256(descriptor.SHA256) || !isSHA256(descriptor.MessageSetHash) {
		return errors.New("该语言包尚未随正式版本发布")
	}
	languagePackMu.Lock()
	if languagePackInstalling[language] {
		languagePackMu.Unlock()
		return errors.New("该语言包正在下载")
	}
	languagePackInstalling[language] = true
	languagePackMu.Unlock()
	defer func() {
		languagePackMu.Lock()
		delete(languagePackInstalling, language)
		languagePackMu.Unlock()
	}()
	downloadURL, err := languagePackDownloadURL(descriptor)
	if err != nil {
		return err
	}
	request, _ := http.NewRequest(http.MethodGet, downloadURL, nil)
	request.Header.Set("User-Agent", "Tech-Card-Manager/"+appVersion)
	response, err := languagePackHTTPClient.Do(request)
	if err != nil {
		return fmt.Errorf("语言包下载失败：%w", err)
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		return fmt.Errorf("语言包下载失败（HTTP %d）", response.StatusCode)
	}
	payload, err := io.ReadAll(io.LimitReader(response.Body, languagePackMaxBytes+1))
	if err != nil {
		return fmt.Errorf("语言包下载失败：%w", err)
	}
	if len(payload) > languagePackMaxBytes {
		return errors.New("语言包过大，已拒绝安装")
	}
	sum := sha256.Sum256(payload)
	if !strings.EqualFold(hex.EncodeToString(sum[:]), descriptor.SHA256) {
		return errors.New("语言包摘要验证失败")
	}
	if err := extractLanguagePack(payload, language, descriptor); err != nil {
		return err
	}
	languagePackMu.Lock()
	delete(languagePackCache, language)
	delete(languagePackErrors, language)
	languagePackMu.Unlock()
	return nil
}

func ensureConfiguredLanguagePack() {
	for _, language := range languagePacksNeedingRestore(loadSettings().Language) {
		if err := installLanguagePack(language); err != nil {
			languagePackMu.Lock()
			languagePackErrors[language] = err.Error()
			languagePackMu.Unlock()
			appendManagerLog("language pack restore " + language + ": " + err.Error())
		}
	}
	if languageCatalogActive() {
		if err := publishWebCardLanguages(); err != nil {
			appendManagerLog("web-card language publication: " + err.Error())
		}
	}
}

func languagePacksNeedingRestore(configured string) []string {
	if !languageCatalogActive() {
		return nil
	}
	configured = configuredLanguage(configured)
	result := make([]string, 0, len(languageOptions))
	for _, option := range languageOptions {
		language := option.Code
		if option.BuiltIn || languagePackInstalled(language) || (language != configured && !languagePackHistoryPresent(language)) {
			continue
		}
		result = append(result, language)
	}
	return result
}

func webCardLanguagePackPayload() map[string]interface{} {
	languages := map[string]map[string]string{}
	for language := range languageCatalog.Languages {
		if messages, ok := languagePackSection(language, "web-card"); ok {
			languages[language] = messages
		}
	}
	return map[string]interface{}{
		"schema":              1,
		"catalog_app_version": languageCatalog.AppVersion,
		"languages":           languages,
	}
}

func extractLanguagePack(payload []byte, language string, descriptor LanguagePackDescriptor) error {
	reader, err := zip.NewReader(bytes.NewReader(payload), int64(len(payload)))
	if err != nil {
		return fmt.Errorf("语言包 ZIP 无效：%w", err)
	}
	allowed := map[string]bool{"manifest.json": true, "web.json": true, "core.json": true, "engine.json": true, "native.json": true, "web-card.json": true}
	if len(reader.File) != len(allowed) {
		return errors.New("语言包文件集合无效")
	}
	root, localeRoot, err := prepareLanguagePackDirectory(language)
	if err != nil {
		return err
	}
	temp, err := os.MkdirTemp(root, ".install-"+language+"-")
	if err != nil {
		return err
	}
	committed := false
	defer func() {
		if !committed {
			_ = os.RemoveAll(temp)
		}
	}()
	seen := map[string]bool{}
	var totalUncompressed uint64
	for _, file := range reader.File {
		name := filepath.ToSlash(file.Name)
		if !allowed[name] || seen[name] || file.FileInfo().Mode()&os.ModeSymlink != 0 || file.FileInfo().IsDir() {
			return errors.New("语言包包含不允许的文件")
		}
		seen[name] = true
		totalUncompressed += file.UncompressedSize64
		if file.UncompressedSize64 > 4<<20 || totalUncompressed > languagePackMaxBytes {
			return errors.New("语言包内容过大")
		}
		source, err := file.Open()
		if err != nil {
			return err
		}
		data, readErr := io.ReadAll(io.LimitReader(source, 4<<20+1))
		closeErr := source.Close()
		if readErr != nil || closeErr != nil || len(data) > 4<<20 {
			return errors.New("语言包内容读取失败")
		}
		if err := writeLanguagePackFile(filepath.Join(temp, name), data); err != nil {
			return err
		}
	}
	var manifest installedLanguageManifest
	b, err := os.ReadFile(filepath.Join(temp, "manifest.json"))
	if err != nil || json.Unmarshal(b, &manifest) != nil || manifest.Schema != languageCatalogSchema || manifest.Product != languageCatalog.Product || manifest.Locale != language || manifest.Revision != descriptor.Revision || manifest.ReleasedWith != descriptor.ReleasedWith || manifest.CatalogSchema != descriptor.CatalogSchema || manifest.MessageSetHash != descriptor.MessageSetHash || len(manifest.Files) != 5 {
		return errors.New("语言包清单与当前应用目录不匹配")
	}
	for _, section := range []string{"web", "core", "engine", "native", "web-card"} {
		var messages map[string]string
		name := section + ".json"
		b, err := os.ReadFile(filepath.Join(temp, name))
		if err != nil || json.Unmarshal(b, &messages) != nil || messages == nil || !validLanguagePackFileHash(filepath.Join(temp, name), manifest.Files[name]) {
			return fmt.Errorf("语言包 %s 目录无效", section)
		}
	}
	destination := filepath.Join(localeRoot, fmt.Sprintf("r%d", descriptor.Revision))
	if _, _, valid := installedLanguagePack(language); valid {
		committed = true
		_ = os.RemoveAll(temp)
		return nil
	}
	if err := os.MkdirAll(filepath.Dir(destination), 0755); err != nil {
		return err
	}
	backup := ""
	if _, err := os.Stat(destination); err == nil {
		backup = fmt.Sprintf("%s.invalid-%d", destination, time.Now().UnixNano())
		if err := os.Rename(destination, backup); err != nil {
			return err
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return err
	}
	if err := os.Rename(temp, destination); err != nil {
		if backup != "" {
			_ = os.Rename(backup, destination)
		}
		return err
	}
	if parent, err := os.Open(filepath.Dir(destination)); err == nil {
		_ = parent.Sync()
		_ = parent.Close()
	}
	if backup != "" {
		_ = os.RemoveAll(backup)
	}
	committed = true
	return nil
}

func prepareLanguagePackDirectory(language string) (string, string, error) {
	root := languagePackRoot()
	if err := os.MkdirAll(root, 0755); err != nil {
		return "", "", err
	}
	rootInfo, err := os.Lstat(root)
	if err != nil || !rootInfo.IsDir() || rootInfo.Mode()&os.ModeSymlink != 0 {
		return "", "", errors.New("语言包目录不安全")
	}
	localeRoot := filepath.Join(root, language)
	if err := os.MkdirAll(localeRoot, 0755); err != nil {
		return "", "", err
	}
	localeInfo, err := os.Lstat(localeRoot)
	if err != nil || !localeInfo.IsDir() || localeInfo.Mode()&os.ModeSymlink != 0 {
		return "", "", errors.New("语言包目录不安全")
	}
	return root, localeRoot, nil
}

func writeLanguagePackFile(path string, data []byte) error {
	file, err := os.OpenFile(path, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0644)
	if err != nil {
		return err
	}
	if _, err = file.Write(data); err == nil {
		err = file.Sync()
	}
	if closeErr := file.Close(); err == nil {
		err = closeErr
	}
	return err
}

func handleLanguagePacks(w http.ResponseWriter, r *http.Request) {
	languageForErrors := currentLanguage()
	if r.Method == http.MethodGet {
		response := map[string]interface{}{"app_version": appVersion, "catalog_app_version": languageCatalog.AppVersion, "catalog_schema": languageCatalog.Schema, "catalog_hash": languageCatalogHash, "languages": supportedLanguages()}
		language := strings.TrimSpace(r.URL.Query().Get("language"))
		if messages, ok := languagePackSection(language, "web"); ok {
			response["web_messages"] = messages
		}
		if messages, ok := languagePackSection(language, "native"); ok {
			response["native_messages"] = messages
		}
		writeJSON(w, response)
		return
	}
	if r.Method != http.MethodPost {
		writeJSONStatus(w, http.StatusMethodNotAllowed, map[string]string{"error": localizeBackendText(languageForErrors, "仅支持 GET 或 POST")})
		return
	}
	r.Body = http.MaxBytesReader(w, r.Body, 4096)
	var request struct {
		Language string `json:"language"`
	}
	if err := json.NewDecoder(r.Body).Decode(&request); err != nil {
		writeJSONStatus(w, http.StatusBadRequest, map[string]string{"error": localizeBackendText(languageForErrors, "请求 JSON 无效")})
		return
	}
	language := strings.TrimSpace(request.Language)
	if err := installLanguagePack(language); err != nil {
		if option, known := recognizedLanguage(language); known && !option.BuiltIn {
			languagePackMu.Lock()
			languagePackErrors[language] = err.Error()
			languagePackMu.Unlock()
		}
		writeJSONStatus(w, http.StatusBadGateway, map[string]string{"error": localizeBackendText(languageForErrors, err.Error())})
		return
	}
	response := map[string]interface{}{"ok": true, "language": language, "languages": supportedLanguages()}
	if err := publishWebCardLanguages(); err != nil {
		warning := localizeBackendText(languageForErrors, "语言包已安装，但网页卡片语言文件发布失败："+err.Error())
		response["warning"] = warning
		appendManagerLog("web-card language publication: " + err.Error())
	}
	writeJSON(w, response)
}

func languagePackSection(language, section string) (map[string]string, bool) {
	if _, _, ok := installedLanguagePack(language); !ok {
		return nil, false
	}
	_, _ = languagePackMessage(language, section, "__load__")
	languagePackMu.RLock()
	defer languagePackMu.RUnlock()
	values, ok := languagePackCache[language][section]
	if !ok {
		return nil, false
	}
	copyValues := make(map[string]string, len(values))
	for key, value := range values {
		copyValues[key] = value
	}
	return copyValues, true
}

var traditionalChineseReplacer = strings.NewReplacer(
	"简体中文", "簡體中文", "语言", "語言", "设置", "設定", "当前", "目前", "媒体库", "媒體庫", "电影", "電影", "电视剧", "電視劇", "标题", "標題", "标签", "標籤", "状态", "狀態", "选择", "選擇", "显示", "顯示", "检查", "檢查", "错误", "錯誤", "启动", "啟動", "关闭", "關閉", "下载", "下載", "应用", "應用程式", "目录", "目錄", "路径", "路徑", "任务", "任務", "失败", "失敗", "刷新", "重新整理", "确认", "確認", "删除", "刪除", "恢复", "復原", "缓存", "快取", "文件", "檔案", "后台", "背景", "自动", "自動", "运行", "執行", "连接", "連線", "支持", "支援", "网络", "網路", "用户", "使用者", "数据", "資料", "读取", "讀取", "写入", "寫入", "总数", "總數",
)

func traditionalChinese(value string) string { return traditionalChineseReplacer.Replace(value) }
