Tech Card Manager v4.0.4 / 使用说明
===================================

中文
----

- Windows x64 Portable GUI；完整解压 `TCM-v4.0.4-Windows-x64-EXE.zip` 后运行其中的 `Tech-Card-Manager.exe`。
- Windows 只读索引 Emby NFO 并维护技术规格卡片；不抓取 IMDb、不写 NFO、不生成标签、不运行 AI。
- 可在设置中即时切换简体中文与 English (United States)。切换不会重载页面，也不会丢失筛选、选择、滚动位置、弹窗或未保存表单。
- 老用户无需转换 NFO、索引缓存、卡片数据、日志或备份。缺少语言字段的旧设置默认使用简体中文；旧日志原样保留，新任务按启动时的语言生成日志。
- 未提供翻译的 Emby 界面语言会让 Web Card 回退简体中文。Technical Specs 字段键与缓存结构保持不变。
- 升级时先从系统托盘完全退出旧版，用 ZIP 中的同名 EXE 替换当前程序；保留 `data`、`logs`、`backup`、`runtime`、`updates` 及其他用户文件夹。

English
-------

- This is a Windows x64 portable GUI. Fully extract `TCM-v4.0.4-Windows-x64-EXE.zip`, then run `Tech-Card-Manager.exe`.
- Windows indexes Emby NFO files in read-only mode and maintains the Technical Specs Web Card. It does not scrape IMDb, write NFO files, generate tags, or run AI.
- Settings can switch instantly between Simplified Chinese and English (United States). Switching does not reload the page or lose filters, selection, scroll position, dialogs, or unsaved form state.
- Existing users do not need to convert NFO files, index caches, card data, logs, or backups. Old settings without a language use Simplified Chinese; historical logs remain unchanged, while each new task uses the language captured when it starts.
- Unsupported Emby UI languages make the Web Card fall back to Simplified Chinese. Technical Specs field keys and cache schemas remain unchanged.
- Before upgrading, fully exit the old version from the system tray, replace the current executable with the same-named EXE from the ZIP, and preserve `data`, `logs`, `backup`, `runtime`, `updates`, and all other user folders.

Validation boundary / 验收边界
------------------------------

- Source contracts, JavaScript syntax, Windows x64 Go test compilation, Go vet, and Windows x64 GUI cross-build are checked before packaging.
- Real Windows, PowerShell 5.1, UAC, tray behavior, Edge Web UI, and a live Emby DOM require target-machine acceptance.
- The repository does not claim that a ZIP, checksum, tag, or GitHub Release exists until the maintainer explicitly authorizes and completes release packaging.
