Tech Card Manager Windows v4.0.3
=====================================

- Windows x64 Portable GUI；完整解压 ZIP 后运行其中 EXE。
- Windows 只读索引 Emby NFO 并维护技术规格卡片；不抓取 IMDb、不写 NFO、不生成标签、不运行 AI。
- 正式发布包仅为 `TCM-v4.0.3-Windows-x64-EXE.zip`，不提供裸 EXE。
- ZIP 内的程序名固定为 `Tech-Card-Manager.exe`，升级时用它替换旧程序，不要保留多个版本化 EXE。

本版重点
--------

- 设置中加入登录后启动，并可选择在登录启动时静默最小化至系统托盘。
- 设置中加入简体中文 / English (United States) 语言选择。4.0.3 暂保持中文界面，为后续语言包保留设置；它不改变 NFO、技术规格或卡片数据。
- “关于”区域显示正式发布日期，并在每次打开设置页面时自动检查 GitHub 正式更新。
- “刷新当前媒体库”按当前电影或电视剧空间分别刷新对应目录，避免不必要地扫描另一类媒体库。
- Windows 端继续保持只读 NFO 边界；所有设置和刷新均不写入 NFO。

使用与验收
----------

- 打开 Tech Card Manager 即启动服务；最小化后继续运行；关闭或托盘退出会撤下 Emby 卡片并停止服务。
- 在实际 Windows x64、PowerShell 5.1、UAC、托盘和 Emby Server 上验收登录启动、静默启动、电影/电视剧分区刷新及 Web Card。
- 当前源码基线已完成源码回归、JavaScript 语法和 Windows x64 交叉构建验证。
- 当前仓库尚未据此声明 ZIP 内容、SHA256 或正式发布验收完成；这些结果只能在实际运行发布构建并核对产物后记录。
- 真实 Windows / Emby 页面仍需目标环境验收。
