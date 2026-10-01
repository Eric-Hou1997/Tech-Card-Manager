# Tech Card Manager 重写范围与规则

状态：产品目标和平台矩阵冻结，完整产品与平台验收未完成。当前执行入口见 [重写规划入口](README.md)；旧 12 步授权不得覆盖最新约束。只迁移各自 v4.1.0 功能、界面和操作，不扩展产品。

## 首发矩阵

| 系统 | 架构 | 格式 | 数量 |
| --- | --- | --- | --- |
| macOS | ARM64 | DMG | 1 |
| Windows | x64、ARM64 | Setup.exe (NSIS) | 2 |
| Linux | x64、ARM64 | AppImage、DEB、RPM | 6 |

每产品 5 个构建目标、9 个安装包；两产品共 10 个目标、18 个包。macOS Intel 不支持。ARM 明确为 ARM64。完整机器可读配置见 [scope.json](scope.json)。

## 支持环境的验收目标

- macOS 12 为保留旧版兼容性的最低目标，并验证本机系统；最低系统未实测之前不可宣称支持。
- Windows 11 24H2，x64 与 ARM64 分别验收。旧版 Windows 其他版本列为兼容调查项，不能静默承诺或删除已证明的兼容性。
- Linux：Ubuntu 22.04/24.04 LTS、Debian 12/13、Fedora 43/44；每种架构分别验收。Ubuntu/Debian 验证 DEB 与 AppImage，Fedora 验证 RPM 与 AppImage。
- Linux 基础构建环境为 Ubuntu 22.04；验证 glibc、WebKitGTK 4.1、GTK、托盘依赖及 RPM 包名映射。X11/Wayland 均有检查项。系统版本是待验收目标，不是已通过声明。
- 需要真实 Emby 部署的能力按现有兼容版本建立用例；Docker、远程和 NAS 的现有实际能力先入台账，再决定适配，不能当作已支持或擅自删除。

## 架构与行为

Rust + Tauri 2 + TypeScript + Vue 3/Vite；以前端实际运行原版对应界面为目标，不交付验证工作台。每产品一套 Rust 核心，平台差异集中适配。前端不直接管理媒体写入与凭据。两个产品独立构建、安装和运行。TCM 保持 NFO 只读；ITM 保留全部 NFO 安全、归属、AI 费用与任务范围约束。

业务迁移阶段退出 Go/Python/PowerShell 正式运行链；目前旧实现仅作基线，不能先删除。行为差异必须有对应编号、原因和测试。已知新增缺陷清零才可发布，不以测试通过宣称绝对零缺陷。

## 安装、更新与隔离

TCM 保留原版检查更新、提示确认和手动下载/安装流程，按已安装产品、系统、架构及渠道选择正确产物。macOS DMG、Windows NSIS、Linux AppImage/DEB/RPM 的包目标不变；不增加 TCM 应用内自动安装器，不把 OTA 签名信任根作为 TCM 必备条件。包管理器拥有的文件仍由系统包管理器更新，当前不打包或部署软件源。

验证工程置于 `rewrite/`，使用独立 application identifier、数据目录和测试产物目录。换栈应用的产品版本为 v5.0.0；v4.1.0 仅作为功能、界面和操作基线，不能复制成新版产品版本。当前开发产物不能用于正式发布。禁止访问生产更新源、自动生成 OTA 密钥、覆盖正式应用或真实媒体库。手动更新目标选择使用明确标记的隔离夹具；不接入自动更新插件或生成 OTA 密钥。

## 分支与发布

保留 main 与 v4.1.0 历史，在 `v5.0.0-rewrite` 复用已有实现并定向纠偏。当前不提交、不打包（含验证应用包）、不发布、不打标签、不向 GitHub 同步或触发云端任务，不使用子代理；维护者本阶段要求有效额度剩余 10% 停工。旧文档中的相反执行授权已被替代。正式软件源部署、既有密钥变更和最终制包验收须另行授权。正式产物命名按 product/version/os/arch/package 唯一匹配，文件名、签名、校验、更新目录及文档必须一致。未来正式命名规范在发布前冻结，不更改旧版 OTA 对旧产物的识别。

## 证据边界

静态检查、单元测试、交叉编译、打包、安装、启动、UI 渲染和退出分别记账。缺少环境必须记为 blocked/unverified；任何首发目标不得因此删除。

## 浏览器依赖边界

Manager 窗口使用 Tauri 的系统 WebView，不要求用户另装 Chrome/Edge 浏览器。它仍依赖 macOS WKWebView、Windows WebView2 Runtime、Linux WebKitGTK；Windows 安装包带离线 Runtime 安装支持。IMDb 数据获取另行验收：ITM 已接通 HTTP 与隔离 IMDb WebView 后备并记录部分原生证据；网络服务的长期稳定性及全部平台获取行为仍须分别验收，不能从 Manager 窗口启动推断。参见 [Tauri WebView](https://tauri.app/reference/webview-versions/)。
