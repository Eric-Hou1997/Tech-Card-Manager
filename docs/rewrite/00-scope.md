# Tech Card Manager 重写范围与规则

状态：范围已冻结，平台支持待验证。正式产品仍为 v4.1.0；验证工程不代表 v5.0.0 完成。

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

Rust + Tauri 2 + TypeScript；本轮验证工程统一 Vue 3 + Vite。每产品一套 Rust 核心，平台差异集中适配。前端不直接管理媒体写入与凭据。两个产品独立构建、安装和运行。TCM 保持 NFO 只读；ITM 保留全部 NFO 安全、归属、AI 费用与任务范围约束。

业务迁移阶段退出 Go/Python/PowerShell 正式运行链；目前旧实现仅作基线，不能先删除。行为差异必须有对应编号、原因和测试。已知新增缺陷清零才可发布，不以测试通过宣称绝对零缺陷。

## 安装、更新与隔离

macOS DMG 初装，OTA 使用独立机器更新产物；Windows 当前用户 NSIS 安装，目标为应用内一次点击自动完成更新；AppImage 自更新，DEB/RPM 由系统包管理器拥有文件并通过签名软件源更新。

验证工程置于 `rewrite/`，使用独立 application identifier、数据目录和测试产物目录。验证元数据沿用当前基线版本 4.1.0，不能用于正式发布。禁止访问生产更新源、自动生成 OTA 密钥、覆盖正式应用或真实媒体库。安装更新演练使用隔离测试通道；现有 Ed25519 PEM 与 Tauri 签名格式需验证，不能直接假定兼容。

## 分支与发布

保留 main 与 v4.1.0 历史。此次授权已扩展到第 1–8 步的开发、测试和隔离验证打包，不包含正式发布、tag、push 或部署软件源。正式产物命名按 product/version/os/arch/package 唯一匹配，文件名、签名、校验、更新目录及文档必须一致。未来正式命名规范在发布前冻结，不更改旧版 OTA 对旧产物的识别。

## 证据边界

静态检查、单元测试、交叉编译、打包、安装、启动、UI 渲染和退出分别记账。缺少环境必须记为 blocked/unverified；任何首发目标不得因此删除。

## 浏览器依赖边界

Manager 窗口使用 Tauri 的系统 WebView，不要求用户另装 Chrome/Edge 浏览器。它仍依赖 macOS WKWebView、Windows WebView2 Runtime、Linux WebKitGTK；Windows 安装包带离线 Runtime 安装支持。IMDb 数据获取是另一个验收链，当前 HTTP 202 未证明可用，不能把窗口可启动当作已摆脱获取侧外部浏览器。参见 [Tauri WebView](https://tauri.app/reference/webview-versions/)。
