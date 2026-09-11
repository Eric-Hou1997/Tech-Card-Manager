# 云端验收与本地交付进度

这是重写分支的验证记录，不是 v5.0.0 发布说明。维护者已允许将审核后的源码同步到两个公开仓库的 `v5.0.0-rewrite` 分支，并运行 GitHub Actions；不发布 Release、不打发布标签、不替换 main。

## 环境与证据边界

- 五目标工作流：macOS ARM64、Windows x64/ARM64、Linux x64/ARM64。Linux 两个架构分别构建 AppImage、DEB、RPM，每产品共九种安装包。
- Windows x64 当前云端镜像为 Windows Server 2022，不能替代首发 Windows 11 的完整验收。Windows ARM64 使用 Windows 11 ARM 节点。
- Linux 使用 Ubuntu 22.04 的原生 x64/ARM64 节点。Xvfb 运行验证不能替代 GNOME/KDE、Wayland、托盘缺失环境及其他目标发行版的桌面验收。
- macOS 使用 ARM64 macOS 15 节点，最低支持版本另行验收。
- 安装包运行检查区分 DMG 临时安装、NSIS 静默安装/同版本重装/卸载、AppImage 解包运行。首轮检查不证明 OTA、数据迁移或 DEB/RPM 安装；DEB/RPM 后续另有 24 个发行版容器组合的安装证据。
- TCM 的独立 Emby 环境工作流使用官方 4.9.5.0 DEB 及固定 SHA-256，在临时目录启动真实服务。服务 API 和网页响应不等于新 TCM 的安装/发布/卡片渲染链路通过。
- 所有产物使用独立 Validation 应用身份；未接入正式更新渠道，不得用于替换已发布产品。测试产物保留三天，按实际结果下载至本地 build/rewrite/cloud 目录。

## 云端发现与修正

1. Windows Git 自动转换换行导致冻结源码清单误报漂移。检出前禁用 core.autocrlf，Python 统一 UTF-8。未重写冻结台账来掩盖漂移。
2. Windows 标准化路径的盘符前缀 `\\?\C:` 被当作完整目录读取，造成根配置失败。现在组装到 RootDir 后才读取元数据；仍然拒绝相对路径、父级遍历和重解析点。增加 Windows 原始路径边界用例。
3. Windows 的 PathBuf::join 会提前规范化 verbatim 路径中的 `..`。回归用例改用原始 OsString，确保验证的确收到带父级遍历的输入。
4. 隔离 Emby 包使用相对 ELF 解释器及私有 C 运行库，需要按官方包根目录和库目录启动；不能套用系统库或臆测绝对安装目录。以真实进程日志定位并复测。

## 尚未满足的最终放行条件

第 9–12 步仍未整体验收。完整 Emby 集成迁移、全部用户入口和八语言、旧数据迁移、签名兼容与自动更新、各发行版安装升级卸载、长期稳定性及旧引擎退出都必须逐项提供证据。早期第 5–8 步记录中的未迁移行为仍然有效；云端测试通过不能关闭这些缺口。

main、v4.1.0 标签和现有 OTA 密钥保持原有状态。未创建 GitHub Release。

## 已取得的云端结果

- 两个产品的五目标构建与运行流水线全部通过：10/10。核心行为测试、类型漂移、格式/静态检查、九包清单和原生窗口/IPC/退出均执行。
- DEB/RPM 在 Ubuntu 22.04/24.04、Debian 12/13、Fedora 43/44 的 x64/ARM64 用户态容器中完成安装、普通用户启动、同版本重装、LICENSE/NOTICE 字节检查及卸载：24/24。容器允许 WebKit 创建子进程沙箱，使用 Xvfb；不作为完整桌面、内核或受限容器策略验收。
- 隔离 Emby 4.9.5.0 在原生 Linux x64/ARM64 节点完成两轮 API、Web 客户端 HTML、重启和退出后端口关闭。TCM 注入/数据发布/真实卡片渲染尚未迁移验收。
- 每产品五份运行报告、十二份发行版报告已归档，得到九个互不冲突的已测试安装包哈希。实际交付文件必须逐个匹配这些哈希。

详见 [云端结果与源提交](evidence/cloud-validation.json)、[逐包状态](evidence/platform-matrix.json) 和 [报告目录](evidence/cloud)。此处的“通过”仅限记录的验证范围，不关闭第 9–12 步的完整产品放行条件。

本地交付已核对：`build/rewrite/cloud/34557054059/README.md`，9 个包全部匹配云端验收哈希；安装包和本地交付文件不提交 Git。

Git 归档的 Windows 报告仅统一为 LF 换行，JSON 字段和包字节未改变；原始下载报告保留在本地交付目录及对应 Actions 产物中。
