# 原始持久化与迁移契约

状态：原版源码路径、迁移规则和现有实现已登记；当前实现及检查证据以 features.json 和 15-baseline-only-correction.md 为准。隔离夹具检查不能代替 Windows 原版数据迁移或五目标现场验收；本轮没有修改真实 Emby/媒体。

原路径根：P = 旧便携版 exe 所在目录；E = `%APPDATA%/Emby-Server`；C = E/programdata/custom-tech-specs；W = E/system/dashboard-ui。

| 数据编号 | 原始产物 | 新版责任及行为契约 | 必须验证 |
| --- | --- | --- | --- |
| TCM-D-01 | P/data/settings.json、agent-cycle.json、cleanup-report.json；P/logs | settings/migration：从便携版显式导入安装版用户数据目录，保留根配置、原有语言和启动偏好及历史日志；不写安装程序目录，不新增跨次启动的界面状态偏好 | 多份便携目录冲突、旧目录只读、重复导入、取消、用户选择错误目录 |
| TCM-D-02 | C/manager-root-discovery.json、manager-root-state.json；显式 RootConfigPath | library：保留每个物理根、Movie/TV 空间、映射和刷新范围，不按公共父路径合并 | 断网、权限拒绝、路径大小写、符号链接、外置盘、NAS/远程路径；功能以原版实证为准 |
| TCM-D-03 | E/programdata/data/library.db | emby-adapter：Emby 拥有数据库；只读发现真实父子关系，不接管 Schema、不写入 | SQLite 锁/WAL、一致快照、Emby 升级、目录版本差异；不得假设所有平台同一路径 |
| TCM-D-04 | C/manager-items-cache.json、manager-catalog.json、manager-index-summary.json、manager-state.json、manager-xml-errors.json | catalog：保留索引/状态来源及解析版本；可重建派生索引但不可静默丢根或修改 NFO | 坏 XML 不是无数据；临时读失败下次可重试；部分扫描、外部变化、同 IMDb 多条目 |
| TCM-D-05 | 媒体 NFO、embedded ownership manifest | nfo-reader：始终只读；归属仅展示；Movie/Series/Season/Episode 行为一致 | 字节、BOM、换行、mtime 不变；未知 XML、不完整写入、重复标签；TCM 无媒体写入迁移 |
| TCM-D-06 | W/technical-specs-card.js、technical-specs-data.json、technical-specs-runtime.json、technical-specs-languages.json；C/technical-specs-card.js | card/service：保留源码资源、磁盘文件、网页集成、索引当前性与运行许可的原版边界；对外数据不含本地私密路径，不新增客户端加载/渲染遥测 | Card 版本、哈希、缓存、语言、租约失效、停止后行为和 Emby 路由切换；真实客户端渲染属于验收证据，不成为新产品状态 |
| TCM-D-07 | W/index.html、index.html.techspecs.original.bak；P/backup/emby-<id>/web-patch-state.json、web-patch-transaction.json、锁与基线文件 | emby-maintenance：长期备份必须在 Emby 管理树之外；保留实例标识、原始字节、事务日志和可信归属，先恢复中断事务 | 部分替换、磁盘满、并发写入、未知标记、备份缺失、Emby 自动更新覆盖、失败回滚 |
| TCM-D-08 | P/runtime、旧 Edge 会话目录；C/technical-specs-worker.ps1、历史计划任务/登录启动项 | lifecycle/legacy：不用便携位置做安装版业务数据根；旧资源逐项证据识别；新 Manager 嵌入 WebView；旧 worker 在验收后退出正式链 | PID 复用、同名进程、未知归属、重复维护、静默登录启动、第二次启动与退出清理 |
| TCM-D-09 | C/update-state.json；语言包目录、descriptor/catalog | update/locale：保留原版手动版本检查、缓存、下载确认和精确安装包选择，按当前目标/架构/渠道给出安装说明；保留语言与版本绑定；Manager/Card 语言分别验收 | 旧缓存兼容、错包/错架构、离线/限流、下载确认/取消、语言包缺失和恢复；系统安装及数据保留另在包验收验证，不增加自动升级/重启 |

源码依据：[便携根与目录](../../windows/platform_windows.go#L31)、[Emby 数据根](../../windows/platform_windows.go#L99)、[引擎资源清单](../../windows/engine/windows-engine.ps1#L18)、[外部备份与事务](../../windows/engine/windows-engine.ps1#L970)、[Manager 状态路径](../../windows/main.go#L1438)。字段锚点见 entrypoints.json；已有转换与未验收边界见上述现有台账，不把已实现模块重复列为重新搭建任务。

安装包升级与 Emby Web Card 维护是两个事务：软件升级成功不能冒充卡片已加载。迁移必须先生成可审查计划，验证目标和备份，再执行并重验；未验证的新用户数据根不能删除旧便携备份。跨平台 Emby 部署位置通过适配与发现处理，禁止硬编码当前 Windows 布局为跨平台事实。
