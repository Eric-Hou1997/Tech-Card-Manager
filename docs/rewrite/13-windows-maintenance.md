# Windows 最小提权实现与待验收项

本文件记录 2026-09-12 本地未发布重写代码。Windows 辅助程序已接入源码，但**尚未构建、运行或通过安装验收**；不属于 Windows 支持声明，最终包放行仍为 0/18。

## 已接通的代码

- Manager 通过 Windows `ShellExecuteExW` 的 `runas` 启动同目录固定名称 `tcm-maintenance-helper.exe`。保留实际进程句柄；系统取消（1223）、拒绝（5）及其他启动失败分别返回结构化错误。参数采用 UTF-16 引用规则，不经过 PowerShell、cmd 或脚本解释器。
- 创建唯一随机名称的本机命名管道：单实例、首次实例标志、拒绝远程客户端、显式 DACL。高权限客户端仅请求具体读写权限，并使用 `SECURITY_IDENTIFICATION`，不允许低权限服务端模拟管理员身份。Manager 用管道客户端 PID 核对实际启动句柄的 PID；helper 用服务端 PID 和已打开的 Manager 进程句柄核对同目录 Manager。
- 管道使用 overlapped I/O。超时先取消并等待 I/O 结束，再释放缓冲区和事件。帧仍使用原有 64 MiB 上限、序列校验、严格命令协议和未知结果不自动重发的约束。
- helper 只接收初始化时选定的一个 Emby web 目录。拒绝 UNC、父目录穿越、ADS、含尾随空格/点的歧义路径、重解析点、多链接文件及普通用户能修改的目标。目标及祖先目录的句柄不共享删除权限，贯穿维护会话；每个请求重验目标，写操作后再次验证。
- 恢复目录固定为系统 Known Folder `ProgramData/TechCardManagerValidation`，不读取环境变量选取恢复根，也不接受调用方指定备份目录。只允许 Administrators/System 访问。helper 调整自身新对象的默认所有者为 Administrators，避免交互用户利用所有者权限修改恢复数据的 DACL。已有恢复文件在初始化时递归检查；随后核对仍被固定的私有根，不在每次状态查询时遍历全部历史备份。
- Linux 与 Windows 复用一个维护工作线程：串行请求、唯一索引发布者、发布失败后停止服务、查询保留原始错误、关闭管道并等待辅助进程退出。共享层没有增加第二个发布入口。
- `tauri.windows.conf.json` 和 `build-helper.mjs` 已登记 Windows x64/ARM64 的 `.exe` sidecar；现有按用户安装模式保持原设置。

ACL 策略刻意拒绝无法明确解释的条件或对象 ACE。系统支持差异、域账号、服务账号和管理员策略导致的合法目录兼容仍需实际验收，不能自行放宽为 Everyone 写权限。

## 当前证据与边界

1. macOS 实际执行：Windows 参数引用 1 项测试、Linux 授权/进程退出 2 项、共用维护工作线程 2 项通过。后两项验证发布失败停止、原始原因可见、同步关闭与不再发布。它们不是 Windows UAC 运行证据。
2. macOS 工作区 Clippy 全 target/feature 检查通过。
3. 正常 Windows x64 核心检查在 SQLite C 依赖处失败：本机没有 Windows SDK，`stdlib.h` 不可用。未生成 Windows 程序或包。
4. 使用仓库外 `/private/tmp/tcm-windows-check` 临时入口进行 x64/ARM64 Rust 类型检查与 Clippy。入口直接引用当前源文件，并且仅为 metadata 检查显式用 SQLite 的 `in_gecko` 模式省略 C 编译/链接。**这不验证 SQLite、链接、运行、安装或真实平台行为，不得作为构建配置或包输入。** 产品 Cargo 配置没有启用该模式。
5. Windows 管道往返/断开、读超时后缓冲区复用、错误进程/端点、路径拒绝及硬链接回归已编写，但未在 Windows 执行。另一个 ACL 行为测试显式标为 ignored，要求管理员手动在已提权测试终端运行，单元测试不会主动弹 UAC。

临时检查日志：`/private/tmp/tcm-windows-check/{core-native-dependencies,x64-metadata,arm64-metadata}.log`。后续源码修改后须重新生成对应证据。

## Windows 实机放行要求

- 两种架构安装包都实际包含匹配的 helper；从含空格、中文、较长路径启动。系统同用户提升和输入另一个管理员凭据分别验收。
- UAC 允许、取消、拒绝；helper 缺失、启动立即失败、管道身份不符、Manager 或 helper 崩溃、超时、关闭期间未完成请求，都必须提供真实结果并停止服务与子进程。
- 在隔离 Emby 安装中验证受保护目录：计划预览、确认安装、实际服务提供资源、客户端卡片加载/渲染、增量变化、停止、修复、升级、移除及恢复。不能操作未经单独授权的生产 Emby。
- 运行普通权限 Windows 测试；在显式管理员测试终端运行 ACL 测试。覆盖私有恢复数据、Public/Users 可写 ACL、条件 ACE、链接/重解析点、权限在运行中改变，以及继承权限和文件所有者。
- 检查断开后租约终止、子进程退出、锁与句柄清理、恢复收据可查询、未知结果不重发。确认所有 NFO 字节与修改时间不变。
- 同步执行最终源码的 Linux 受保护目录回归，确认共用工作线程抽取没有改变已有行为。

## API 依据

- [命名管道安全及访问权限](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights)
- [ShellExecuteEx 的启动选项](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/ns-shellapi-shellexecuteinfow)
- [管道客户端身份模拟与安全上下文](https://learn.microsoft.com/en-us/windows/win32/ipc/impersonating-a-named-pipe-client)
- [新对象的默认所有者](https://learn.microsoft.com/en-us/windows/win32/secauthz/owner-of-a-new-object)
