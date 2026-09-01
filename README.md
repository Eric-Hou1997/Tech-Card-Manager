# Tech Card Manager v4.0.0

面向 Emby Server 的只读 NFO 索引与网页技术规格卡片管理器。

当前源码平台：

- `windows/`：Windows x64 Portable GUI 源码。
- `packaging/`：当前版本的发布说明源文件。
- `tools/`：源码验证与发布构建脚本。

本仓库按产品而不是按操作系统划分。未来若增加 macOS 版本，仍应是同一个只读 Tech Card Manager 产品，不会吸收 IMDb-Tech-Manager 的抓取、NFO 编辑、AI 或标签生产职责。

## 产品边界

- Windows 端只读索引 NFO，并维护 Emby Web Card。
- 不抓取 IMDb、不写 NFO、不运行 AI、不生成标签，也不迁移 NFO ownership。
- 对旧组件的检测、迁移和网页补丁恢复必须由用户明确确认，并保留可验证备份。
- 发布前必须运行 `tools/test-source.sh`，并在真实 Windows、PowerShell、UAC、托盘和 Emby DOM 环境完成验收。
- 发布产物只能是包含 GUI `.exe` 的 ZIP；源码验证与交叉编译不能替代真实平台验收。

本仓库从 v4.0.0 起独立维护，正式产品名为 Tech Card Manager；仓库目录与机器友好的发布文件名使用 `Tech-Card-Manager`。默认开发主线为 `main`。
