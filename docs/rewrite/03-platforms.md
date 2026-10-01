# 平台验收与阻塞

首发矩阵不可减少。当前可执行的原生开发环境为 macOS ARM64；Windows/Linux 的当前源码编译与桌面验收尚未取得结果。旧检查记录仅证明当时源码的检查范围。CI 配置只是配方，本轮不推送、不触发云端工作流。以下制包/安装检查等待另行授权，现阶段只执行必要的源码、编译与开发态检查。

## 每一个包必须执行

1. 保存提交、锁文件、系统版本、CPU、包 hash 和签名状态。
2. 干净环境安装，确认真实程序架构、依赖、应用入口、图标、许可证。
3. 启动 Tauri，核对前端 mounted、IPC、屏幕渲染分别成立。
4. 目录读取、隔离文件读写、凭据存取和删除、网络错误反馈。
5. 托盘/菜单、二次启动、最小化恢复、正常退出和崩溃后恢复。
6. 原版手动检查、确认下载与对应系统安装方式；跨版本数据保留、失败恢复、重启后版本核对。
7. 卸载应用，确认临时资源清理，用户数据按约定保留。

## 手动更新与制包边界

TCM 保留原版手动更新：检查官方版本、确认精确安装包、由系统浏览器打开下载地址，用户完成安装。TCM 不注册 updater，不增加 OTA 密钥、后台安装、自动重启或独立更新通道。ITM 的签名与 OTA 责任不适用于 TCM。

发行准备须将精确包名、目标架构和安装渠道接入原有手动更新选择器；真实 NSIS 安装、DMG 替换、DEB/RPM 包管理器操作及 AppImage 替换属于后续获授权的包验收。无需为本次迁移新增 APT/RPM 仓库或无交互更新方案。

## 外部系统阻塞

- Emby：目录读取只证明可读；真实安装位置、持续资源发布权限、提权及恢复需要真实 Emby 测试实例，不修改用户实际安装。
- 维护者已确认当前没有隔离 Emby 测试实例；先完成其余可执行项，卡片真实加载、显示、停止与恢复保持待验收。
- macOS 最低 12 系统、Windows x64/ARM64、Linux x64/ARM64 的所列发行版均需单独验收。ARM 上 x64 仿真不能代替原生应用验收。

## 官方依据

- [Tauri Windows ARM/NSIS](https://tauri.app/distribute/windows-installer/#building-for-32-bit-or-arm)
- [Tauri ARM AppImage](https://tauri.app/distribute/appimage/#appimages-for-arm-based-devices)
- [Tauri Linux 基础系统](https://tauri.app/distribute/debian/#limitations)
- [GitHub runner 标签](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)

CI 配方覆盖五个目标、九个包；其中配置的 ARM runner 标签为 windows-11-arm 与 ubuntu-22.04-arm，当前可用性和实际运行仍待获授权后核实。

## 本机可重复演练

已有 `python3 tools/rewrite/macos_smoke.py` 从唯一测试 DMG 挂载只读卷，复制至本次临时目录，核对 ARM64、许可证与 NOTICE，运行窗口并等待 Vue → Rust IPC 回执及退出事件，核对进程退出后删除本次隔离安装。当前不执行此包演练，也不生成测试 DMG。输出位于忽略的 build/rewrite/native-smoke.json；该脚本本身不能证明 Gatekeeper、公证或正式卸载数据策略。
