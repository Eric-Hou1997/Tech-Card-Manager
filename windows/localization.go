package main

import (
	"bytes"
	"io"
	"regexp"
	"sort"
	"strings"
	"sync"
)

const defaultLanguage = "zh-CN"

type LanguageOption struct {
	Code         string `json:"code"`
	NativeName   string `json:"native_name"`
	EnglishName  string `json:"english_name"`
	Flag         string `json:"flag"`
	BuiltIn      bool   `json:"built_in"`
	Installed    bool   `json:"installed"`
	Downloadable bool   `json:"downloadable"`
	State        string `json:"state"`
	Revision     int    `json:"revision,omitempty"`
	ReleasedWith string `json:"released_with,omitempty"`
	Error        string `json:"error,omitempty"`
}

var languageOptions = []LanguageOption{
	{Code: "zh-CN", NativeName: "简体中文", EnglishName: "Simplified Chinese", Flag: "cn", BuiltIn: true, Installed: true, State: "built-in"},
	{Code: "zh-Hant", NativeName: "繁體中文", EnglishName: "Traditional Chinese", Flag: "cn", BuiltIn: true, Installed: true, State: "built-in"},
	{Code: "en-US", NativeName: "English (United States)", EnglishName: "English (United States)", Flag: "us", BuiltIn: true, Installed: true, State: "built-in"},
	{Code: "fr-FR", NativeName: "Français", EnglishName: "French", Flag: "fr", Downloadable: true, State: "not-installed"},
	{Code: "ru-RU", NativeName: "Русский", EnglishName: "Russian", Flag: "ru", Downloadable: true, State: "not-installed"},
	{Code: "ja-JP", NativeName: "日本語", EnglishName: "Japanese", Flag: "jp", Downloadable: true, State: "not-installed"},
	{Code: "es-ES", NativeName: "Español", EnglishName: "Spanish", Flag: "es", Downloadable: true, State: "not-installed"},
	{Code: "th-TH", NativeName: "ไทย", EnglishName: "Thai", Flag: "th", Downloadable: true, State: "not-installed"},
}

var languageByCode = func() map[string]LanguageOption {
	result := make(map[string]LanguageOption, len(languageOptions))
	for _, option := range languageOptions {
		result[option.Code] = option
	}
	return result
}()

func supportedLanguage(language string) bool {
	option, ok := languageByCode[strings.TrimSpace(language)]
	return ok && (option.BuiltIn || (languageCatalogActive() && languagePackInstalled(option.Code)))
}

func normalizedLanguageAlias(language string) string {
	language = strings.TrimSpace(language)
	if language == "zh-TW" || language == "zh-HK" || language == "zh-MO" {
		return "zh-Hant"
	}
	return language
}

func normalizedLanguage(language string) string {
	language = configuredLanguage(language)
	if supportedLanguage(language) {
		return language
	}
	return defaultLanguage
}

func configuredLanguage(language string) string {
	language = normalizedLanguageAlias(language)
	if _, ok := languageByCode[language]; ok {
		return language
	}
	return defaultLanguage
}

func supportedLanguages() []LanguageOption {
	result := make([]LanguageOption, len(languageOptions))
	copy(result, languageOptions)
	decorateLanguageOptions(result)
	return result
}

func localized(language, chinese, english string) string {
	language = normalizedLanguage(language)
	if language == "en-US" {
		return english
	}
	if language == "zh-Hant" {
		return traditionalChinese(chinese)
	}
	if value, ok := languagePackMessage(language, "core", stableMessageID(english)); ok {
		return value
	}
	if language != "zh-CN" {
		return english
	}
	return chinese
}

func currentLanguage() string { return normalizedLanguage(loadSettings().Language) }

func currentLocalized(chinese, english string) string {
	return localized(currentLanguage(), chinese, english)
}

func localizedNative(language, chinese, english string) string {
	language = normalizedLanguage(language)
	if language == "en-US" {
		return english
	}
	if language == "zh-Hant" {
		return traditionalChinese(chinese)
	}
	if value, ok := languagePackMessage(language, "native", stableMessageID(english)); ok {
		return value
	}
	if language != "zh-CN" {
		return english
	}
	return chinese
}

func currentNativeLocalized(chinese, english string) string {
	return localizedNative(currentLanguage(), chinese, english)
}

var englishBackendPhrases = []struct{ zh, en string }{
	{"语言包目录记录不完整", "The language-pack catalog entry is incomplete"},
	{"语言包文件名无效", "The language-pack filename is invalid"},
	{"该语言不需要下载", "This language does not need to be downloaded"},
	{"当前版本没有为该语言指定语言包", "This app version does not specify a pack for that language"},
	{"该语言包尚未随正式版本发布", "This language pack has not been published with a stable release"},
	{"该语言包正在下载", "This language pack is already downloading"},
	{"语言包下载失败", "Language-pack download failed"},
	{"语言包下载重定向次数过多", "Too many language-pack download redirects"},
	{"语言包下载重定向到非官方主机", "The language-pack download redirected to an unofficial host"},
	{"语言包过大，已拒绝安装", "The language pack is too large and was rejected"},
	{"语言包摘要验证失败", "The language-pack checksum verification failed"},
	{"语言包 ZIP 无效", "The language-pack ZIP is invalid"},
	{"语言包文件集合无效", "The language-pack file set is invalid"},
	{"语言包包含不允许的文件", "The language pack contains a disallowed file"},
	{"语言包内容过大", "The language-pack content is too large"},
	{"语言包内容读取失败", "The language-pack content could not be read"},
	{"语言包清单与当前应用目录不匹配", "The language-pack manifest does not match this app catalog"},
	{"语言包目录无效", "The language-pack catalog is invalid"},
	{"语言包目录不安全", "The language-pack directory is unsafe"},
	{"语言包目录不属于当前应用版本", "The language-pack catalog does not belong to this app version"},
	{"语言包已安装，但网页卡片语言文件发布失败", "The language pack was installed, but publishing the Web Card language file failed"},
	{"所选语言无效", "The selected language is invalid"},
	{"所选语言尚未安装或不受当前版本支持", "The selected language is not installed or is unsupported by this app version"},
	{"服务已停止；请先确认媒体目录", "Service stopped; confirm media folders first"},
	{"服务调度已停止，但撤卡状态写入失败；请重试停止", "Scheduling stopped, but the card-removal state could not be written. Retry stopping"},
	{"全部服务已停止；正在退出", "All services stopped; exiting"},
	{"Emby 网页卡片安装失败", "Emby Web Card installation failed"},
	{"旧版后台 Agent 正在运行", "Legacy background Agent is running"},
	{"旧版 Agent 状态文件", "Legacy Agent state file"},
	{"旧版程序 PID", "Legacy app PID"},
	{"已停止旧版程序 PID", "Stopped legacy app PID"},
	{"旧版登录启动项", "Legacy sign-in startup entry"},
	{"旧版计划任务", "Legacy scheduled task"},
	{"无法确认所有权的网页补丁（标记、脚本数量或块内容异常）", "Web Patch ownership cannot be verified (invalid markers, script count, or block content)"},
	{"旧版网页卡片", "Legacy Web Card"},
	{"计划任务", "Scheduled task"},
	{"旧计划任务清理提示", "Legacy scheduled-task cleanup note"},
	{"服务由可视化 Manager 主进程拥有；最小化继续运行，关闭窗口停止全部服务", "The service is owned by the visual Manager process; it continues while minimized, and closing the window stops all services"},
	{"停止服务会失效运行许可，Emby 页面随后撤下本程序拥有的技术规格卡片", "Stopping the service invalidates the runtime lease, after which Emby removes the Technical Specs card owned by this app"},
	{"网页卡片 v4.1.0：后台索引永不修改 Emby index.html，网页维护仍需用户明确确认", "Web Card v4.1.0: background indexing never changes Emby index.html, and Web maintenance still requires explicit user approval"},
	{"Manager 索引摘要损坏", "The Manager index summary is damaged"},
	{"Manager 索引摘要读取失败", "Could not read the Manager index summary"},
	{"读取管理员维护进程退出状态失败", "Could not read the administrator maintenance process exit status"},
	{"管理员维护进程返回未知等待状态", "The administrator maintenance process returned an unknown wait state"},
	{"无法创建管理员维护请求", "Could not create the administrator maintenance request"},
	{"管理员维护进程已退出（代码", "The administrator maintenance process exited (code"},
	{"），但没有返回可验证结果", ") without returning a verifiable result"},
	{"管理员维护进程退出代码", "Administrator maintenance process exit code"},
	{"目录配置状态无效", "The folder configuration state is invalid"},
	{"不允许使用管理员任务执行未知操作", "An administrator task cannot run an unknown operation"},
	{"无法确认当前管理员权限状态，操作未开始", "The current administrator state could not be verified; the operation did not start"},
	{"管理员操作已结束，但网页集成文件没有通过完整性复核", "The administrator operation finished, but the Web integration files failed integrity verification"},
	{"运行诊断", "Run diagnostics"},
	{"刷新", "Refresh"},
	{"失败", "failed"},
	{"媒体目录路径不能为空", "The media-folder path cannot be empty"},
	{"媒体目录相互包含，会造成重复扫描", "Media folders overlap and would be scanned twice"},
	{" 与 ", " and "},
	{"本轮只扫描此目录", "Scanning only this folder"},
	{"没有识别出任何电影/电视剧媒体库物理路径", "No physical Movie or TV Show library path was identified"},
	{"Tech Card Manager 4.1.0 支持 Portable 数据目录、用户选择媒体目录、目录级扫描与只读 NFO 目录", "Tech Card Manager 4.1.0 supports portable data folders, user-selected media folders, folder-level scans, and a read-only NFO catalog"},
	{"当前媒体库没有独立分类目录；请先在设置中把混合目录拆分或标记为电影/电视剧，已避免误扫另一类媒体", "The current library has no separately categorized folder. Split mixed folders or mark them as Movies or TV Shows in Settings to avoid scanning the other media type"},
	{"服务调度已停止，但未能确认 Emby 撤卡；请点击按钮重试", "Scheduling stopped, but removal of the card from Emby could not be confirmed. Use the button to retry"},
	{"服务调度已停止，但未能确认 Emby 撤卡；退出已暂停", "Scheduling stopped, but removal of the card from Emby could not be confirmed. Exit has been paused"},
	{"正在处理旧版迁移；如 Windows 需要确认，将显示管理员权限窗口。迁移完成前新版服务保持停止。", "Migrating legacy components. Windows may display an administrator prompt. The new service remains stopped until migration finishes."},
	{"检测到无法确认所有权的 Emby 网页修改，已安全停止；不会覆盖 index.html", "Unverifiable changes were found in the Emby Web UI. The operation stopped safely and will not overwrite index.html"},
	{"网页卡片集成已就绪；运行状态仍由可视化 Manager 统一控制", "Web Card integration is ready; runtime state remains controlled by the visual Manager"},
	{"已彻底停用技术规格集成并恢复原生 Emby；媒体 NFO 未被修改", "Technical Specs integration was fully disabled and native Emby was restored; media NFO files were not changed"},
	{"索引文件未通过版本、目录与一致性复核", "The index files failed version, folder, or consistency verification"},
	{"服务运行中，但 Emby 卡片运行许可不可用", "The service is running, but the Emby card runtime lease is unavailable"},
	{"服务已停止；Emby 技术规格卡片已撤下", "Service stopped; the Emby Technical Specs card was removed"},
	{"服务已停止；正在恢复原生 Emby", "Service stopped; restoring native Emby"},
	{"服务已停止；旧版组件保持不变", "Service stopped; legacy components remain unchanged"},
	{"管理员维护操作尚未开始", "The administrator maintenance operation has not started"},
	{"管理员维护进程已退出", "The administrator maintenance process exited"},
	{"管理员维护结果与当前请求不匹配，已拒绝采用", "The administrator result does not match the current request and was rejected"},
	{"管理员维护结果损坏", "The administrator maintenance result is damaged"},
	{"无法许可管理员维护进程，操作没有执行", "The administrator maintenance process could not be authorized; no operation was performed"},
	{"管理员维护进程在建立安全握手前退出", "The administrator maintenance process exited before establishing a secure handshake"},
	{"管理员维护进程启动超时，操作没有执行", "The administrator maintenance process timed out during startup; no operation was performed"},
	{"等待管理员维护进程失败", "Waiting for the administrator maintenance process failed"},
	{"无法启动管理员维护进程", "Could not start the administrator maintenance process"},
	{"Windows 没有返回管理员维护进程句柄，操作未执行", "Windows did not return a handle for the administrator maintenance process; no operation was performed"},
	{"用户取消了管理员权限请求，操作未执行", "The user cancelled the administrator request; no operation was performed"},
	{"无法确认主程序仍在运行，管理员操作没有执行", "The main app could not be confirmed as running; the administrator operation was not performed"},
	{"主程序已经退出，管理员操作没有执行", "The main app exited; the administrator operation was not performed"},
	{"等待主程序许可超时，管理员操作没有执行", "Timed out waiting for permission from the main app; the administrator operation was not performed"},
	{"管理员进程已启动，等待主程序许可", "The administrator process started and is waiting for permission from the main app"},
	{"Windows 已启动受控管理员维护进程；正在建立安全握手", "Windows started the controlled administrator maintenance process; establishing a secure handshake"},
	{"管理员权限已获得，正在执行并等待结果复核", "Administrator access was granted; running the operation and waiting for result verification"},
	{"主程序已许可执行", "The main app authorized execution"},
	{"管理员维护操作未获主程序许可", "The administrator maintenance operation was not authorized by the main app"},
	{"管理员维护操作已完成", "The administrator maintenance operation completed"},
	{"无效的管理员维护请求", "Invalid administrator maintenance request"},
	{"无效的管理员维护握手", "Invalid administrator maintenance handshake"},
	{"管理员维护尚未结束，退出已暂停", "Administrator maintenance has not finished; exit is paused"},
	{"事务仍由当前程序持有，没有遗留到后台", "the transaction is still owned by the current app and was not left running in the background"},
	{"Portable 程序目录不可写，请把整个软件文件夹移动到可写位置后重试", "The portable app folder is not writable. Move the entire app folder to a writable location and retry"},
	{"Portable 程序目录无法清理临时文件", "The portable app folder could not clean up its temporary file"},
	{"无法创建 Portable 目录", "Could not create portable folder"},
	{"无法创建 Portable 界面运行目录", "Could not create the portable UI runtime folder"},
	{"无法创建独立界面会话", "Could not create an isolated UI session"},
	{"无法建立界面进程所有权", "Could not establish ownership of the UI process"},
	{"无法接管 Edge 界面进程树", "Could not take ownership of the Edge UI process tree"},
	{"无法启动已纳管的 Edge 界面", "Could not start the managed Edge UI"},
	{"找不到 Microsoft Edge，无法创建受 Manager 生命周期控制的可视化窗口", "Microsoft Edge was not found, so a window controlled by the Manager lifecycle cannot be created"},
	{"Edge 管理界面仍未关闭", "The Edge Manager window is still open"},
	{"索引摘要不完整，不能作为目录恢复依据", "The index summary is incomplete and cannot be used to restore media folders"},
	{"上一版有效索引摘要", "the previous version's valid index summary"},
	{"无法确定当前程序路径", "Could not determine the current app path"},
	{"启用 Windows 登录自启动失败", "Could not enable start at Windows sign-in"},
	{"Windows 登录自启动写入后校验失败", "Start at Windows sign-in failed post-write verification"},
	{"关闭 Windows 登录自启动后校验失败", "Disabling start at Windows sign-in failed verification"},
	{"已启用 Windows 登录后启动应用", "Enabled app startup after Windows sign-in"},
	{"已关闭 Windows 登录后启动应用", "Disabled app startup after Windows sign-in"},
	{"已停止旧版 Agent", "Stopped the legacy Agent"},
	{"未发现需要迁移的旧版组件", "No legacy components need migration"},
	{"旧版组件已迁移；新版服务可以安全启动", "Legacy components were migrated; the new service can start safely"},
	{"旧版程序未能退出", "The legacy app did not exit"},
	{"已停止旧版程序", "Stopped legacy app"},
	{"不是目录", "Not a folder"},
	{"Emby dashboard-ui 不可用，无法更新卡片运行许可", "Emby dashboard-ui is unavailable, so the card runtime lease cannot be updated"},
	{"未发现需要清理的 Windows 旧版残留", "No legacy Windows remnants need cleanup"},
	{"已清理", "Cleaned"},
	{"项 Windows 旧版残留", "legacy Windows remnants"},
	{"未知操作", "Unknown operation"},
	{"Windows Server 版本不支持平台", "The Windows Server build does not support this platform"},
	{"媒体目录必须使用绝对路径", "Media folders must use absolute paths"},
	{"不能直接扫描整个磁盘根目录", "A whole drive root cannot be scanned directly"},
	{"服务当前处于", "The service is currently in"},
	{"状态，请稍候", "state; please wait"},
	{"正在建立卡片索引", "Building the card index"},
	{"新版服务未启动；正在退出", "The new service did not start; exiting"},
	{"当前没有等待迁移的旧版组件", "No legacy components are waiting for migration"},
	{"正在申请管理员权限以迁移旧版组件", "Requesting administrator access to migrate legacy components"},
	{"旧版迁移未完成", "Legacy migration did not complete"},
	{"旧版迁移没有开始", "Legacy migration did not start"},
	{"启用或修复 Emby 集成", "Set up or repair Emby integration"},
	{"启动后台服务", "Start background service"},
	{"停止后台服务", "Stop background service"},
	{"立即增量检查", "Run incremental check now"},
	{"修复网页卡片", "Repair Web Card"},
	{"迁移所列旧版组件", "Migrate listed legacy components"},
	{"只扫描此目录", "Scan only this folder"},
	{"刷新当前媒体库", "Refresh current library"},
	{"完整重建索引", "Fully rebuild index"},
	{"从 Emby 发现媒体目录", "Discover media folders from Emby"},
	{"清理旧版残留", "Clean up legacy remnants"},
	{"彻底停用并恢复原生 Emby", "Fully disable integration and restore native Emby"},
	{"技术规格前端脚本版本不匹配：期望", "The Technical Specs frontend script version does not match. Expected"},
	{"，文件：", ", file: "},
	{"找不到 Emby library.db", "Emby library.db was not found"},
	{"读取媒体目录设置失败", "Could not read media-folder settings"},
	{"用户已选择", "selected by the user"},
	{"用户手动选择", "manually selected by the user"},
	{"用户选择", "user-selected"},
	{"媒体目录设置中没有启用的目录", "No folder is enabled in media-folder settings"},
	{"WEB_PATCH_BACKUP_ROOT_REQUIRED：缺少 Portable backup 路径", "WEB_PATCH_BACKUP_ROOT_REQUIRED: the portable backup path is missing"},
	{"WEB_PATCH_BACKUP_INSIDE_EMBY：长期备份不能放在 Emby 管理的目录树中", "WEB_PATCH_BACKUP_INSIDE_EMBY: the durable backup cannot be stored in the Emby-managed tree"},
	{"INVALID_EMBY_INDEX：文件过小", "INVALID_EMBY_INDEX: the file is too small"},
	{"INVALID_EMBY_INDEX：缺少", "INVALID_EMBY_INDEX: missing"},
	{"INVALID_EMBY_INDEX：包含 NUL", "INVALID_EMBY_INDEX: contains NUL"},
	{"INVALID_WEB_PATCH_MARKERS：受管标记不完整或重复", "INVALID_WEB_PATCH_MARKERS: managed markers are incomplete or duplicated"},
	{"BASELINE_CONTAINS_WEB_PATCH：原版备份不得包含 Technical Specs 注入", "BASELINE_CONTAINS_WEB_PATCH: the clean baseline must not contain a Technical Specs injection"},
	{"INVALID_WEB_PATCH_COUNT：Technical Specs script 数量异常", "INVALID_WEB_PATCH_COUNT: the number of Technical Specs scripts is invalid"},
	{"UNSAFE_WEB_PATCH_OWNERSHIP：检测到不完整、重复或夹带其它内容的网页补丁，拒绝自动删除或覆盖", "UNSAFE_WEB_PATCH_OWNERSHIP: an incomplete, duplicate, or contaminated Web Patch was detected; automatic deletion or overwrite was refused"},
	{"BASELINE_HASH_MISMATCH：不可变备份校验失败", "BASELINE_HASH_MISMATCH: immutable backup verification failed"},
	{"WEB_PATCH_STATE_BASELINE_OUTSIDE_BACKUP：状态文件中的原版备份路径越界", "WEB_PATCH_STATE_BASELINE_OUTSIDE_BACKUP: the baseline path in state escapes the backup root"},
	{"NO_TRUSTED_BASELINE：当前 index 已注入，但没有与其干净内容匹配的可靠原版备份。请先用同版本 Emby 官方安装包修复 Web UI", "NO_TRUSTED_BASELINE: the current index is injected, but no trusted baseline matches its clean content. Repair the Web UI with the official package for the same Emby version first"},
	{"WEB_PATCH_JOURNAL_PATH_INVALID：事务文件不在 dashboard-ui", "WEB_PATCH_JOURNAL_PATH_INVALID: the transaction file is outside dashboard-ui"},
	{"WEB_PATCH_JOURNAL_INDEX_INVALID：事务日志不属于当前 Emby index.html", "WEB_PATCH_JOURNAL_INDEX_INVALID: the transaction journal does not belong to the current Emby index.html"},
	{"WEB_PATCH_JOURNAL_HASH_INVALID：事务日志 Hash 无效", "WEB_PATCH_JOURNAL_HASH_INVALID: the transaction-journal hash is invalid"},
	{"WEB_PATCH_RECOVERY_FAILED：已提交候选与日志声明的 Patch 状态不一致", "WEB_PATCH_RECOVERY_FAILED: the committed candidate does not match the Patch state declared by the journal"},
	{"WEB_PATCH_RECOVERY_FAILED：回滚后 Hash 不一致", "WEB_PATCH_RECOVERY_FAILED: the hash does not match after rollback"},
	{"WEB_PATCH_RECOVERY_REQUIRED：候选已替换但可靠回滚文件缺失", "WEB_PATCH_RECOVERY_REQUIRED: the candidate was replaced but the trusted rollback file is missing"},
	{"WEB_PATCH_RECOVERY_FAILED：缺失入口恢复后 Hash 不一致", "WEB_PATCH_RECOVERY_FAILED: the hash does not match after restoring the missing entry file"},
	{"WEB_PATCH_RECOVERY_REQUIRED：检测到未完成事务和第三方文件变化，已停止自动处理", "WEB_PATCH_RECOVERY_REQUIRED: an incomplete transaction and third-party file changes were detected; automatic processing stopped"},
	{"WEB_PATCH_BUSY：另一个 Web Patch 事务仍在运行", "WEB_PATCH_BUSY: another Web Patch transaction is still running"},
	{"WEB_CARD_ASSET_HASH_MISMATCH：前端脚本替换校验失败", "WEB_CARD_ASSET_HASH_MISMATCH: frontend script replacement verification failed"},
	{"CONCURRENT_MODIFICATION：Emby index.html 在读取后被其它程序修改，事务已中止", "CONCURRENT_MODIFICATION: another program changed Emby index.html after it was read; the transaction was aborted"},
	{"WEB_PATCH_POST_VERIFY_FAILED：正式文件 Hash 不一致", "WEB_PATCH_POST_VERIFY_FAILED: the installed-file hash does not match"},
	{"WEB_PATCH_POST_VERIFY_FAILED：受管块或目标 script 缺失", "WEB_PATCH_POST_VERIFY_FAILED: the managed block or target script is missing"},
	{"WEB_PATCH_POST_VERIFY_FAILED：恢复结果仍含受管 Web Patch", "WEB_PATCH_POST_VERIFY_FAILED: the restored file still contains the managed Web Patch"},
	{"条件回滚后的 Hash 不一致", "The hash does not match after conditional rollback"},
	{"EMBY_INDEX_MISSING：请先用当前版本 Emby 官方安装包修复 Web UI；本工具不会用旧版备份盲目覆盖", "EMBY_INDEX_MISSING: repair the Web UI with the official package for the current Emby version first; this tool will not blindly overwrite it with an older backup"},
	{"INVALID_EMBY_INDEX：无法定位 </body>，拒绝修改", "INVALID_EMBY_INDEX: </body> could not be located; modification was refused"},
	{"已通过事务校验安装", "was installed with transactional verification"},
	{"已健康，NO_CHANGE", "is healthy, NO_CHANGE"},
	{"EMBY_INDEX_MISSING：不会自动使用可能过期的备份恢复", "EMBY_INDEX_MISSING: a potentially stale backup will not be restored automatically"},
	{"RESTORE_VERIFY_FAILED：恢复结果与原版备份不是字节级一致", "RESTORE_VERIFY_FAILED: the restored result is not byte-identical to the clean baseline"},
	{"已恢复可信原版 index.html 并移除 Web Card/派生索引；长期备份已保留", "Restored the trusted clean index.html and removed the Web Card and derived index; durable backups were preserved"},
	{"FULL_REBUILD_OFFLINE_ROOT：完整重建已中止，不会清空旧缓存。请先恢复离线目录", "FULL_REBUILD_OFFLINE_ROOT: the full rebuild was aborted without clearing the old cache. Restore offline folders first"},
	{"Manager 索引摘要尚未生成", "The Manager index summary has not been generated"},
	{"当前技术规格索引包含", "The current Technical Specs index contains"},
	{"个 IMDb 条目", "IMDb items"},
	{"扫描统计", "Scan statistics"},
	{"自动发现的 Emby 媒体库", "Automatically discovered Emby media libraries"},
	{"Web 前端", "Web frontend"},
	{"dashboard-ui 注入与 v", "dashboard-ui injection and v"},
	{"JS 均正常", "JS are both healthy"},
	{"dashboard-ui 注入或前端 JS 版本不正确", "dashboard-ui injection or the frontend JS version is incorrect"},
	{"XML 错误明细", "XML error details"},
	{"读取 technical-specs-data.json 失败", "Could not read technical-specs-data.json"},
	{"Emby 电影 / 电视剧物理路径自动发现测试（只读）", "Emby Movie / TV Show physical-path discovery test (read-only)"},
	{"原则：按 library.db 真实父子关系逐个物理根判断；绝不合并公共父目录", "Rule: evaluate each physical root using the real library.db hierarchy; never merge a shared parent folder"},
	{"没有识别出电影/电视剧物理路径", "No Movie or TV Show physical path was identified"},
	{"识别出", "Identified"},
	{"个电影/电视剧物理路径", "Movie / TV Show physical paths"},
	{"离线/不可访问", "Offline / inaccessible"},
	{"在线", "Online"},
	{"Emby 节点", "Emby node"},
	{"状态", "Status"},
	{"证据", "Evidence"},
	{"以下物理根已识别，但会排除", "The following physical roots were identified but excluded"},
	{"支持：多个电影库、多个电视剧库、同一虚拟库多个物理路径、电影/电视剧混合库", "Supports multiple Movie libraries, multiple TV Show libraries, multiple physical paths per virtual library, and mixed Movie / TV Show libraries"},
	{"离线路径不会导致旧索引被清空；音乐库和普通视频库不会进入技术规格扫描", "Offline paths do not clear the old index; Music and generic Video libraries are excluded from Technical Specs scans"},
	{"确认这里列出的路径正确后，再不带 -DiscoverOnly 运行同一文件正式安装", "After confirming these paths, run the same file without -DiscoverOnly to install"},
	{"Web Card 已通过锁、CAS、日志与原子替换完成显式修复", "Web Card repair completed with locking, CAS, journaling, and atomic replacement"},
	{"只读 NFO 索引已完整重建，目录", "The read-only NFO index was fully rebuilt with"},
	{"项；Emby index.html 未修改", "items; Emby index.html was not changed"},
	{"找不到 Emby Web", "Emby Web was not found"},
	{"Worker 语法错误", "Worker syntax error"},
	{"复制后的 Manager Worker 未通过 PowerShell Parser 校验", "The copied Manager Worker failed PowerShell parser validation"},
	{"Worker 复制后 SHA256 不一致", "The Worker SHA256 does not match after copying"},
	{"首次建立技术规格索引", "Build the initial Technical Specs index"},
	{"首次索引完成，共", "Initial indexing completed with"},
	{"首次索引输出无效：technical-specs-data.json 缺少 items", "Initial index output is invalid: technical-specs-data.json is missing items"},
	{"验证索引", "Verify the index"},
	{"后台调度由 Tech Card Manager 接管", "Tech Card Manager takes ownership of background scheduling"},
	{"已确保旧的每分钟 PowerShell 计划任务不存在", "Confirmed that the legacy one-minute PowerShell scheduled task does not exist"},
	{"索引引擎启用完成", "index engine setup completed"},
	{"支持 Portable 数据目录、用户选择媒体目录、目录级扫描与只读 NFO 目录", "supports portable data folders, user-selected media folders, folder-level scans, and a read-only NFO catalog"},
	{"安全边界：后台增量检查永不改动 Emby index.html；Web Card 只在用户点击安装/修复时执行事务化注入", "Safety boundary: background incremental checks never change Emby index.html; Web Card injection is transactional and runs only after the user selects install or repair"},
	{"支持多个电影库/电视剧库、同一虚拟库多个物理路径和混合库；离线根保留缓存，恢复后自动续扫", "Supports multiple Movie and TV Show libraries, multiple physical paths per virtual library, and mixed libraries; offline roots retain cache and resume automatically when restored"},
	{"网页卡片：电影优先沿用真实视频卡片，ISO/BDMV 使用独立卡片；电视剧只在节目首页展示，季和单集页面不显示；页面切换后自动校验条目身份", "Web Card: Movies prefer the real Video card, ISO/BDMV uses a standalone card, TV Shows appear only on the Series page, and Season and Episode pages remain suppressed; item identity is verified after navigation"},
	{"下一步：浏览器 Ctrl+F5，然后分别打开一个普通视频文件和一个 ISO 电影确认技术规格卡。iOS 原生 Emby App 不加载服务器 dashboard-ui 自定义 JS，属于客户端限制；标准 Tag 仍正常可用", "Next: press Ctrl+F5 in the browser, then open a regular video file and an ISO Movie to verify the Technical Specs card. The native iOS Emby app does not load custom server dashboard-ui JavaScript; standard Tags remain available"},
	{"语言只能选择简体中文或 English (United States)", "Language must be Simplified Chinese or English (United States)"},
	{"后台检查周期必须在 30 秒到 86400 秒之间", "The background interval must be between 30 and 86400 seconds"},
	{"媒体目录格式无效", "The media-folder configuration is invalid"},
	{"目录配置状态无效", "The folder configuration state is invalid"},
	{"至少启用一个媒体目录", "Enable at least one media folder"},
	{"媒体目录尚未确认", "Media folders have not been confirmed"},
	{"媒体目录中没有启用项", "No media folder is enabled"},
	{"媒体目录不能超过 256 个", "No more than 256 media folders are allowed"},
	{"媒体目录路径为空", "The media-folder path is empty"},
	{"媒体目录重复", "Duplicate media folder"},
	{"媒体目录相互包含，会造成重复扫描", "Media folders overlap and would be scanned twice"},
	{"服务已停止，请先启动服务", "The service is stopped. Start it first"},
	{"当前媒体库只能是 movies 或 tv", "The current library must be movies or tv"},
	{"当前媒体库没有独立分类目录", "The current library has no separately categorized folder"},
	{"当前媒体库没有已启用且已分类的目录", "The current library has no enabled categorized folder"},
	{"已有任务正在运行", "Another task is already running"},
	{"任务没有可执行入口", "The task has no executable entry point"},
	{"任务已取消", "The task was cancelled"},
	{"不允许使用管理员任务执行未知操作", "An unknown operation cannot run as an administrator task"},
	{"无法确认当前管理员权限状态，操作未开始", "The current administrator state could not be verified; the operation did not start"},
	{"正在申请管理员权限；如 Windows 需要确认，将显示 UAC 窗口。", "Requesting administrator access. Windows may display a UAC prompt."},
	{"当前程序已具备管理员权限，操作已开始。", "The app already has administrator access. The operation has started."},
	{"管理员操作已结束，但网页集成复核失败", "The administrator operation finished, but Web integration verification failed"},
	{"网页集成文件没有通过完整性复核", "The Web integration files did not pass integrity verification"},
	{"恢复操作已结束，但结果复核失败", "The restore operation finished, but result verification failed"},
	{"恢复操作未完全移除本程序拥有的 Emby 网页集成", "The restore operation did not completely remove the Emby Web integration owned by this app"},
	{"迁移操作已结束，但仍检测到旧版组件；新版服务保持停止", "Migration finished, but legacy components are still detected; the new service remains stopped"},
	{"检测到旧版组件，需要用户确认迁移", "Legacy components were detected and require confirmation"},
	{"路径为空", "The path is empty"},
	{"未选择目录", "No folder was selected"},
	{"打开目录选择窗口失败", "Could not open the folder picker"},
	{"Manager 引擎文件不存在，请重新启动应用或点击“启用 / 修复集成”", "The Manager engine is missing. Restart the app or choose Set Up / Repair Integration"},
	{"Windows 不支持操作", "Windows does not support this operation"},
	{"刷新 movies 失败", "Failed to refresh Movies"},
	{"刷新 tv 失败", "Failed to refresh TV Shows"},
	{"版本号必须为 vX.Y.Z", "The version must use the vX.Y.Z format"},
	{"该正式发布缺少指定更新包", "The official release is missing the required update package"},
	{"仅支持 GET", "Only GET is supported"},
	{"无法连接 GitHub", "Could not connect to GitHub"},
	{"GitHub 尚未发布正式版本", "No official GitHub release is available"},
	{"GitHub 更新检查失败", "GitHub update check failed"},
	{"GitHub 更新信息无效", "The GitHub update response is invalid"},
	{"GitHub 最新发布不是可用的正式 vX.Y.Z 版本", "The latest GitHub release is not a valid stable vX.Y.Z release"},
	{"GitHub 更新重定向次数过多", "Too many GitHub update redirects"},
	{"GitHub 更新重定向到非官方主机 %s，已拒绝", "The GitHub update redirected to the unofficial host %s and was rejected"},
	{"GitHub 匿名 API 的出口 IP 额度已用完", "The GitHub anonymous API quota for this public IP has been exhausted"},
	{"GitHub 触发了次级限流，请按提示时间后再检查", "GitHub applied a secondary rate limit; check again after the indicated time"},
	{"代理或中间网络拒绝了更新请求", "A proxy or intermediary network rejected the update request"},
	{"GitHub 拒绝了更新请求，但未标明为额度耗尽", "GitHub rejected the update request without identifying an exhausted quota"},
	{"GitHub 更新服务暂时不可用", "The GitHub update service is temporarily unavailable"},
	{"GitHub 更新请求失败（HTTP %d）", "The GitHub update request failed (HTTP %d)"},
	{"无法连接代理服务器", "Could not connect to the proxy server"},
	{"正式发布返回了非官方或不匹配的更新地址", "The official release returned an unofficial or mismatched update URL"},
	{"GitHub 最新发布页面没有返回有效的正式版本", "The GitHub latest-release page did not return a valid stable version"},
	{"GitHub 返回了无法使用的未修改状态", "GitHub returned an unusable not-modified response"},
	{"已检查到更新，但无法保存更新状态", "The update was checked, but its state could not be saved"},
	{"NFO 管理目录损坏，请运行增量检查重建", "The NFO Manager catalog is damaged. Run an incremental check to rebuild it"},
	{"服务已启动", "Service started"},
	{"服务已停止，Emby 将不再显示技术规格卡片", "Service stopped. Emby will no longer show the Technical Specs card"},
	{"已取消；旧版保持不变，新版服务未启动", "Cancelled. Legacy components were left unchanged and the new service was not started"},
	{"服务已停止", "Service stopped"},
	{"服务运行中", "Service running"},
	{"正在检查运行环境", "Checking the runtime environment"},
	{"正在重新检查旧版组件", "Checking legacy components again"},
	{"检测到旧版组件；新版服务尚未启动", "Legacy components detected; the new service has not started"},
	{"服务已停止；请先确认媒体目录", "Service stopped; confirm media folders first"},
	{"正在恢复媒体目录并建立卡片索引", "Restoring media folders and building the card index"},
	{"服务启动失败", "Service startup failed"},
	{"无法建立 Emby 卡片运行许可", "Could not establish the Emby card runtime lease"},
	{"卡片索引没有建立成功", "The card index could not be built"},
	{"正在停止服务并撤下 Emby 技术规格卡片", "Stopping the service and removing the Emby Technical Specs card"},
	{"服务调度已停止，但撤卡状态写入失败；请重试停止", "Scheduling stopped, but the card-removal state could not be written. Retry stopping"},
	{"全部服务已停止；正在退出", "All services stopped; exiting"},
	{"旧版组件保持不变", "legacy components remain unchanged"},
	{"完整重建已中止，不会清空旧缓存", "The full rebuild was cancelled and the old cache was preserved"},
	{"没有已启用的媒体目录", "There are no enabled media folders"},
	{"等待技术规格索引锁超时", "Timed out waiting for the Technical Specs index lock"},
	{"目录级扫描目标不在已启用媒体目录中", "The folder scan target is not within an enabled media folder"},
	{"本轮只扫描此目录", "Scanning only this folder"},
	{"没有识别出任何电影/电视剧媒体库物理路径", "No physical Movie or TV Show library path was identified"},
	{"NFO XML 解析失败，已跳过", "NFO XML parsing failed and was skipped"},
	{"找不到技术规格前端脚本", "The Technical Specs frontend script was not found"},
	{"技术规格前端脚本版本不匹配", "The Technical Specs frontend script version does not match"},
	{"安装包缺少 technical-specs-card.js", "The package is missing technical-specs-card.js"},
	{"找不到 Emby dashboard-ui", "The Emby dashboard-ui folder was not found"},
	{"事务化安装/修复 Emby 技术规格网页卡片", "Installing or repairing the Emby Technical Specs Web Card transactionally"},
	{"错误", "Error"},
	{"完成", "Completed"},
	{"运行中", "Running"},
	{"任务", "Task"},
	{"开始时间", "Started"},
}

var englishBackendPhrasesByLength = func() []struct{ zh, en string } {
	result := append([]struct{ zh, en string }(nil), englishBackendPhrases...)
	sort.SliceStable(result, func(i, j int) bool { return len(result[i].zh) > len(result[j].zh) })
	return result
}()

var windowsPathPattern = regexp.MustCompile(`(?i)(?:[a-z]:\\|\\\\)[^\r\n]*`)

func localizeBackendText(language, value string) string {
	language = normalizedLanguage(language)
	if language == "zh-CN" {
		return value
	}
	// Task output can append user-supplied Windows paths to product messages.
	// Shield those path spans before phrase replacement so a folder such as
	// D:\电影\完成 is never rewritten merely because its components resemble UI
	// words. Protocol and catalog fields are already excluded at their callers.
	protectedPaths := windowsPathPattern.FindAllString(value, -1)
	for index, path := range protectedPaths {
		value = strings.Replace(value, path, backendPathPlaceholder(index), 1)
	}
	for _, phrase := range englishBackendPhrasesByLength {
		translated := phrase.en
		if language == "zh-Hant" {
			translated = traditionalChinese(phrase.zh)
		} else if language != "en-US" {
			if packed, ok := languagePackMessage(language, "core", stableMessageID(phrase.en)); ok {
				translated = packed
			} else if packed, ok := languagePackMessage(language, "engine", stableMessageID(phrase.en)); ok {
				translated = packed
			}
		}
		value = strings.ReplaceAll(value, phrase.zh, translated)
		if language != "en-US" {
			value = strings.ReplaceAll(value, phrase.en, translated)
		}
	}
	for index, path := range protectedPaths {
		value = strings.Replace(value, backendPathPlaceholder(index), path, 1)
	}
	return value
}

func backendPathPlaceholder(index int) string {
	return "\x00TCM_USER_PATH_" + string(rune(index+1)) + "\x00"
}

func localizeResponsePayload(language string, value interface{}) interface{} {
	switch typed := value.(type) {
	case map[string]string:
		copy := make(map[string]string, len(typed))
		for key, item := range typed {
			if key == "error" || key == "message" {
				item = localizeBackendText(language, item)
			}
			copy[key] = item
		}
		return copy
	case map[string]interface{}:
		copy := make(map[string]interface{}, len(typed))
		for key, item := range typed {
			if text, ok := item.(string); ok && (key == "error" || key == "message") {
				item = localizeBackendText(language, text)
			}
			copy[key] = item
		}
		return copy
	default:
		return value
	}
}

// localizeStatusForPresentation translates only product-owned explanatory text.
// Paths, library names, catalog data, protocol keys, and historical job logs stay
// byte-for-byte as produced or supplied by the user.
func localizeStatusForPresentation(status *Status) {
	if status == nil {
		return
	}
	language := status.Language
	status.Service.Message = localizeBackendText(language, status.Service.Message)
	for index := range status.Service.Legacy.Items {
		status.Service.Legacy.Items[index] = localizeBackendText(language, status.Service.Legacy.Items[index])
	}
	status.LastCycle.Error = localizeBackendText(language, status.LastCycle.Error)
	for index := range status.Notes {
		status.Notes[index] = localizeBackendText(language, status.Notes[index])
	}
	for index := range status.Libraries {
		status.Libraries[index].Evidence = localizeBackendText(language, status.Libraries[index].Evidence)
		status.Libraries[index].AccessError = localizeBackendText(language, status.Libraries[index].AccessError)
	}
	for index := range status.DiscoveredRoots {
		status.DiscoveredRoots[index].Evidence = localizeBackendText(language, status.DiscoveredRoots[index].Evidence)
		status.DiscoveredRoots[index].AccessError = localizeBackendText(language, status.DiscoveredRoots[index].AccessError)
	}
	for index := range status.XmlErrorDetails {
		status.XmlErrorDetails[index].Error = localizeBackendText(language, status.XmlErrorDetails[index].Error)
	}
	if status.Extra != nil {
		for _, key := range []string{"index_summary_error", "error", "message"} {
			if value, ok := status.Extra[key].(string); ok {
				status.Extra[key] = localizeBackendText(language, value)
			}
		}
	}
}

type localizedLineWriter struct {
	mu       sync.Mutex
	dst      io.Writer
	language string
	pending  bytes.Buffer
}

func newLocalizedLineWriter(dst io.Writer, language string) *localizedLineWriter {
	return &localizedLineWriter{dst: dst, language: normalizedLanguage(language)}
}

func outputLanguageForWriter(writer io.Writer) string {
	if localizedWriter, ok := writer.(*localizedLineWriter); ok {
		return localizedWriter.language
	}
	return currentLanguage()
}

// The bundled PowerShell engine intentionally remains bilingual. External UI
// languages consume its stable English presentation strings and translate
// them in localizedLineWriter, while structured values remain untouched.
func engineOutputLanguageForWriter(writer io.Writer) string {
	if outputLanguageForWriter(writer) == "zh-CN" {
		return "zh-CN"
	}
	return "en-US"
}

func (w *localizedLineWriter) Write(p []byte) (int, error) {
	w.mu.Lock()
	defer w.mu.Unlock()
	_, _ = w.pending.Write(p)
	for {
		line, err := w.pending.ReadString('\n')
		if err != nil {
			_, _ = w.pending.WriteString(line)
			break
		}
		if _, err := io.WriteString(w.dst, localizeBackendText(w.language, line)); err != nil {
			return 0, err
		}
	}
	return len(p), nil
}

func (w *localizedLineWriter) Flush() error {
	w.mu.Lock()
	defer w.mu.Unlock()
	if w.pending.Len() == 0 {
		return nil
	}
	_, err := io.WriteString(w.dst, localizeBackendText(w.language, w.pending.String()))
	w.pending.Reset()
	return err
}
