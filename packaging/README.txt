Tech Card Manager v4.1.0 / 使用说明
===================================

中文
----

- Windows x64 Portable GUI；完整解压 `TCM-v4.1.0-Windows-x64-EXE.zip` 后运行其中的 `Tech-Card-Manager.exe`。
- Windows 只读索引 Emby NFO 并维护技术规格卡片；不抓取 IMDb、不写 NFO、不生成标签、不运行 AI。
- 简体中文、繁體中文与 English (United States) 内置于主程序；法语、俄语、日语、西班牙语和泰语可在设置中从本次 GitHub Release 下载。每个包由 v4.1.0 目录唯一绑定并校验，不需要单独更新。
- 老用户无需转换 NFO、索引缓存、卡片数据、日志或备份。缺少语言字段的旧设置默认使用简体中文；旧日志原样保留，新任务按启动时的语言生成日志。
- 未安装或不支持的 Emby 界面语言会让 Web Card 回退简体中文。Technical Specs 字段键与缓存结构保持不变，只翻译显示名称、aria-label 与无数据提示。
- 升级时先从系统托盘完全退出旧版，用 ZIP 中的同名 EXE 替换当前程序；保留 `data`、`logs`、`backup`、`runtime`、`updates` 及其他用户文件夹。

English
-------

- This is a Windows x64 portable GUI. Fully extract `TCM-v4.1.0-Windows-x64-EXE.zip`, then run `Tech-Card-Manager.exe`.
- Windows indexes Emby NFO files in read-only mode and maintains the Technical Specs Web Card. It does not scrape IMDb, write NFO files, generate tags, or run AI.
- Simplified Chinese, Traditional Chinese, and English (United States) are built in. French, Russian, Japanese, Spanish, and Thai can be downloaded from this GitHub Release in Settings. Each pack is bound to and verified by the v4.1.0 catalog, with no separate update operation.
- Existing users do not need to convert NFO files, index caches, card data, logs, or backups. Old settings without a language use Simplified Chinese; historical logs remain unchanged, while each new task uses the language captured when it starts.
- Uninstalled or unsupported Emby UI languages make the Web Card fall back to Simplified Chinese. Technical Specs field keys and cache schemas remain unchanged; only display labels, aria-label text, and empty states are translated.
- Before upgrading, fully exit the old version from the system tray, replace the current executable with the same-named EXE from the ZIP, and preserve `data`, `logs`, `backup`, `runtime`, `updates`, and all other user folders.

Validation boundary / 验收边界
------------------------------

- Source contracts, JavaScript syntax, Windows x64 Go test compilation, Go vet, and Windows x64 GUI cross-build are checked before packaging.
- Real Windows, PowerShell 5.1, UAC, tray behavior, Edge Web UI, and a live Emby DOM require target-machine acceptance.
- The repository does not claim that a ZIP, checksum, tag, or GitHub Release exists until the maintainer explicitly authorizes and completes release packaging.
