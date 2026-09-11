# 平台验收与阻塞

首发矩阵不可减少。当前只探测到 macOS ARM64 本机，Windows/Linux 的构建与桌面验收尚未执行。CI 配置只是可运行配方，本轮不推送、不触发云端工作流。

## 每一个包必须执行

1. 保存提交、锁文件、系统版本、CPU、包 hash 和签名状态。
2. 干净环境安装，确认真实程序架构、依赖、应用入口、图标、许可证。
3. 启动 Tauri，核对前端 mounted、IPC、屏幕渲染分别成立。
4. 目录读取、隔离文件读写、凭据存取和删除、网络错误反馈。
5. 托盘/菜单、二次启动、最小化恢复、正常退出和崩溃后恢复。
6. 测试通道升级到下一测试构建；数据保留、失败恢复、重启后版本核对。
7. 卸载应用，确认临时资源清理，用户数据按约定保留。

## 更新阻塞

验证工程暂不注册 updater，不包含生产 URL、公钥或私钥。需要先验证旧版 Ed25519 PEM 与 Tauri/minisign 格式及旧版资产选择器衔接；不得自动生成新密钥。本轮未批准更换密钥或正式签名。

Windows 的无交互更新需要两个不同版本的测试产物、可信签名和真实 NSIS 进程/重启验证；仅能打包不是完成。DEB/RPM 需系统包管理器演练，并设计签名 APT/RPM 仓库；AppImage 需可写位置替换与桌面入口验证。

## 外部系统阻塞

- IMDb：固定 URL 请求只能证明 HTTP 连接；可解析规格、受限网页与自有 WebView 获取仍需验证。
- Emby：目录读取只证明可读；真实安装位置、持续资源发布权限、提权及恢复需要真实 Emby 测试实例，不修改用户实际安装。
- macOS 最低 12 系统、Windows x64/ARM64、Linux x64/ARM64 的所列发行版均需单独验收。ARM 上 x64 仿真不能代替原生应用验收。

## 官方依据

- [Tauri Windows ARM/NSIS](https://tauri.app/distribute/windows-installer/#building-for-32-bit-or-arm)
- [Tauri ARM AppImage](https://tauri.app/distribute/appimage/#appimages-for-arm-based-devices)
- [Tauri Linux 基础系统](https://tauri.app/distribute/debian/#limitations)
- [Tauri 更新签名](https://tauri.app/plugin/updater/)
- [GitHub runner 标签](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)

CI 五个目标生成九个包。Windows ARM runner 为 windows-11-arm，Linux ARM 为 ubuntu-22.04-arm；真实运行可用性以工作流执行和平台报告为准。

## 本机可重复演练

`python3 tools/rewrite/macos_smoke.py` 从唯一测试 DMG 挂载只读卷，复制至本次临时目录，核对 ARM64、许可证与 NOTICE，运行窗口并等待 Vue → Rust IPC 回执及退出事件，核对进程退出后删除本次隔离安装。输出位于忽略的 build/rewrite/native-smoke.json。它不是 Gatekeeper、公证、OTA 或正式卸载数据策略的证明。
