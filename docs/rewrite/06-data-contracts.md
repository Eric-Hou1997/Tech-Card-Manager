# 原始持久化与迁移契约

状态：源码路径及迁移规则已登记；没有迁移、读取或修改真实 Emby/媒体。所有目的模块属于规划，需 Windows 原版与五个新版目标验证。

原路径根：P = 旧便携版 exe 所在目录；E = `%APPDATA%/Emby-Server`；C = E/programdata/custom-tech-specs；W = E/system/dashboard-ui。

| 数据编号 | 原始产物 | 新版责任及行为契约 | 必须验证 |
| --- | --- | --- | --- |
| TCM-D-01 | P/data/settings.json、agent-cycle.json、cleanup-report.json；P/logs | settings/migration：从便携版显式导入安装版用户数据目录，保留根配置、界面、启动偏好和历史日志；不写安装程序目录 | 多份便携目录冲突、旧目录只读、重复导入、取消、用户选择错误目录 |
| TCM-D-02 | C/manager-root-discovery.json、manager-root-state.json；显式 RootConfigPath | library：保留每个物理根、Movie/TV 空间、映射和刷新范围，不按公共父路径合并 | 断网、权限拒绝、路径大小写、符号链接、外置盘、NAS/远程路径；功能以原版实证为准 |
| TCM-D-03 | E/programdata/data/library.db | emby-adapter：Emby 拥有数据库；只读发现真实父子关系，不接管 Schema、不写入 | SQLite 锁/WAL、一致快照、Emby 升级、目录版本差异；不得假设所有平台同一路径 |
| TCM-D-04 | C/manager-items-cache.json、manager-catalog.json、manager-index-summary.json、manager-state.json、manager-xml-errors.json | catalog：保留索引/状态来源及解析版本；可重建派生索引但不可静默丢根或修改 NFO | 坏 XML 不是无数据；临时读失败下次可重试；部分扫描、外部变化、同 IMDb 多条目 |
| TCM-D-05 | 媒体 NFO、embedded ownership manifest | nfo-reader：始终只读；归属仅展示；Movie/Series/Season/Episode 行为一致 | 字节、BOM、换行、mtime 不变；未知 XML、不完整写入、重复标签；TCM 无媒体写入迁移 |
| TCM-D-06 | W/technical-specs-card.js、technical-specs-data.json、technical-specs-runtime.json、technical-specs-languages.json；C/technical-specs-card.js | card/service：区别源码资源、磁盘文件、已提供服务、客户端实际加载/渲染；对外数据不含本地私密路径 | Card 版本、哈希、缓存、语言、租约失效、停止后行为和 Emby 路由切换 |
| TCM-D-07 | W/index.html、index.html.techspecs.original.bak；P/backup/emby-<id>/web-patch-state.json、web-patch-transaction.json、锁与基线文件 | emby-maintenance：长期备份必须在 Emby 管理树之外；保留实例标识、原始字节、事务日志和可信归属，先恢复中断事务 | 部分替换、磁盘满、并发写入、未知标记、备份缺失、Emby 自动更新覆盖、失败回滚 |
| TCM-D-08 | P/runtime、旧 Edge 会话目录；C/technical-specs-worker.ps1、历史计划任务/登录启动项 | lifecycle/legacy：不用便携位置做安装版业务数据根；旧资源逐项证据识别；新 Manager 嵌入 WebView；旧 worker 在验收后退出正式链 | PID 复用、同名进程、未知归属、重复维护、静默登录启动、第二次启动与退出清理 |
| TCM-D-09 | C/update-state.json；语言包目录、descriptor/catalog | update/locale：便携旧更新器到 NSIS 安装版需要桥接；保留语言与版本绑定；Manager/Card 语言同时验收 | 旧资产选择、安装权限、错架构、离线/限流、软件内自动升级、坏包与失败重启 |

源码依据：[便携根与目录](../../windows/platform_windows.go#L31)、[Emby 数据根](../../windows/platform_windows.go#L99)、[引擎资源清单](../../windows/engine/windows-engine.ps1#L18)、[外部备份与事务](../../windows/engine/windows-engine.ps1#L970)、[Manager 状态路径](../../windows/main.go#L1438)。字段锚点见 entrypoints.json，逐字段的实际转换尚未实现。

安装包升级与 Emby Web Card 维护是两个事务：软件升级成功不能冒充卡片已加载。迁移必须先生成可审查计划，验证目标和备份，再执行并重验；未验证的新用户数据根不能删除旧便携备份。跨平台 Emby 部署位置通过适配与发现处理，禁止硬编码当前 Windows 布局为跨平台事实。
