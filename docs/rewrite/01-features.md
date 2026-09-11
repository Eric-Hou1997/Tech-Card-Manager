# 原始功能对照台账

状态：静态入口已定位；语义复核、原版真实运行与新版功能迁移尚未全部验收。每个入口有稳定编号，不能将分组覆盖率当成功能完成率。

来源：v4.1.0、当前源码、README、原生入口和现有测试。未来 Roadmap 与已实现行为分别处理。

| 编号 | 功能 | 行为契约 | 异常/恢复 | 新版位置 |
| --- | --- | --- | --- | --- |
| TCM-WEBPATCH | Emby 安装、修复、移除与回滚 | 验证准确目标和归属；提供维护计划、确认与最小提权；外部可信备份、候选校验、事务替换与后置验证。 | 中途失败、并发修改、备份缺失、未知标记、权限拒绝、重复维护和日志恢复。 | application/emby-maintenance / maintenance |
| TCM-LEGACY | 历史组件识别与迁移 | 只凭路径/命令行/manifest/标记等证据识别历史组件；列明操作、确认后重验目标；保护未知归属。 | PID复用、同名进程、未知patch、部分迁移失败、取消和回滚。 | application/legacy / migration |
| TCM-CARD | Web Card 数据、渲染与失效 | 只发布允许字段；路由、条目类型和可见详情一致；多语言/布局正确；管理端退出后卡片租约按契约失效。 | 旧脚本卸载、路由变化、后台页面、公开数据不得含本地路径、服务停止/崩溃/离线。 | domain/card / Emby adapter |
| TCM-SERVICE | 服务状态和资源所有权 | 请求、运行、磁盘就绪、服务可用、客户端加载和渲染状态区分；启动和停止幂等；失败不得伪报成功。 | 初始租约写入失败、停止写入失败可重试、重复启动、退出时同步清理。 | application/service / console |
| TCM-READONLY | 只读 NFO 解析和归属展示 | 读取 Movie/Series/Season/Episode；直接 section 解析与去重；仅展示 manifest 归属；保持原始 NFO 字节和 mtime。 | 坏XML、缺IMDb、同IMDb多条目、重复标签、无归属元数据、共享盘暂时不可读。 | domain/nfo-reader / inspector |
| TCM-PROTOCOL | 持久化和接口字段 | 保持字段类型、缺省值、枚举和版本迁移语义；模型在 Rust 为权威并生成 TypeScript；字段不能随语言改变。 | 旧配置缺失字段、未知枚举、版本升级、历史状态兼容；序列化字段不是独立用户功能。 | contracts / persistence |
| TCM-DISPATCH | 操作分发和旧命令兼容 | 校验动作、参数、媒体根与运行状态；保留现有用户可观察操作；内部协议替换需要对应入口。 | 未知动作、缺参数、重复请求、权限失败和任务冲突。 | application/commands |
| TCM-LOCALE | 语言和语言包 | 选择语言；验证包完整性及版本绑定；界面与原生控件同步；任务固定启动语言；保持旧数据和机器字段。 | 下载失败、包不匹配、任务中切换、不支持语言回退。 | locale registry / settings |
| TCM-UPDATE | 安装和更新 | 识别正确产品、系统、架构和包型；校验后更新；显示真实阶段；保留用户数据。 | 断网、限流、资产错误、签名失败、取消、升级中断及回退。 | platform/update / settings |
| TCM-DIAGNOSTICS | 诊断、日志和路径定位 | 可导出诊断、打开日志/数据和受影响路径；错误包含可用的媒体身份、路径与任务。 | 无权限、路径不存在、导出失败；不泄露凭据。 | application/diagnostics / diagnostics |
| TCM-LIFECYCLE | 单实例、窗口、托盘和退出 | 单实例；最小化和恢复；显式登录启动；退出清理资源；启动偏好独立于后台运行状态。 | 重复启动/点击、关闭和退出区别、权限失败、进程残留、升级后启动项。 | platform/lifecycle / native UI |
| TCM-TASK | 任务状态与恢复 | 权威任务状态、阶段、范围、进度、历史；暂停/取消/恢复遵循旧行为；重复操作不产生重复副作用。 | 中断、失败重试、任务冲突、退出、日志和状态持久化不一致。 | application/tasks / task center |
| TCM-SCOPE | 媒体空间和操作范围 | Movie/TV 独立根目录和状态；选择、全选/反选、季级选择及当前范围准确；全库从不默认。 | 空目录、越界、符号链接、共享盘离线、目录重叠、路径大小写和不同媒体种类。 | domain/library / library setup |
| TCM-CATALOG | 索引、筛选和状态 | 索引权威数据；增量/重建范围准确；搜索过滤排序与选择状态保留；缓存容量、清理和错误状态可见。 | XML 异常、缓存过期、部分根离线、外部变更、冷启动、索引重建失败。 | application/catalog / library |
| TCM-LAYOUT | 界面布局与交互 | 保留列设置、排序、面板尺寸、上下文入口和响应式语义；可键盘操作；错误/空状态可见。 | 窄窗口、缩放、长翻译、溢出、遮挡、布局持久化和重置。 | presentation/layout / manager |
| TCM-UNCLASSIFIED | 待逐项语义复核 | 源码已冻结并定位；仍需逐项检查调用方、用户入口和运行时语义，不能计为已验收。 | 不得通过通用分组隐藏遗漏。 | pending-design |

## 逐入口追踪

下表保留原版入口与契约。第 5–8 步实现片段、实际测试映射和各平台状态见 features.json；partial 不代表整个功能组已完成，原始 baseline_test_candidates 仍只是候选证据。

| 入口编号 | 类型 | 原入口 | 源码 | 功能编号 |
| --- | --- | --- | --- | --- |
| TCM-E-645ad4936d2e | engine-option | `IndexOnly` | [windows/engine/windows-engine.ps1:2](../../windows/engine/windows-engine.ps1#L2) | TCM-CATALOG |
| TCM-E-011020462a04 | engine-option | `CheckOnly` | [windows/engine/windows-engine.ps1:3](../../windows/engine/windows-engine.ps1#L3) | TCM-DIAGNOSTICS |
| TCM-E-4a6d2e7cbfe3 | engine-option | `DiscoverOnly` | [windows/engine/windows-engine.ps1:4](../../windows/engine/windows-engine.ps1#L4) | TCM-CATALOG |
| TCM-E-f18ca3d807de | engine-option | `ManagedInstall` | [windows/engine/windows-engine.ps1:5](../../windows/engine/windows-engine.ps1#L5) | TCM-WEBPATCH |
| TCM-E-222954d680f0 | engine-option | `RebuildIndexOnly` | [windows/engine/windows-engine.ps1:6](../../windows/engine/windows-engine.ps1#L6) | TCM-CATALOG |
| TCM-E-7c84c670aa8a | engine-option | `RepairWebOnly` | [windows/engine/windows-engine.ps1:7](../../windows/engine/windows-engine.ps1#L7) | TCM-WEBPATCH |
| TCM-E-2c0c1c354e17 | engine-option | `DisableIntegration` | [windows/engine/windows-engine.ps1:8](../../windows/engine/windows-engine.ps1#L8) | TCM-WEBPATCH |
| TCM-E-892193e4f2a6 | engine-option | `RootConfigPath` | [windows/engine/windows-engine.ps1:9](../../windows/engine/windows-engine.ps1#L9) | TCM-SCOPE |
| TCM-E-9d7908ecd633 | engine-option | `OnlyRoot` | [windows/engine/windows-engine.ps1:10](../../windows/engine/windows-engine.ps1#L10) | TCM-SCOPE |
| TCM-E-99a6f4b01fe1 | engine-option | `BackupRootPath` | [windows/engine/windows-engine.ps1:11](../../windows/engine/windows-engine.ps1#L11) | TCM-WEBPATCH |
| TCM-E-4d8ce889214c | engine-option | `OutputLanguage` | [windows/engine/windows-engine.ps1:13](../../windows/engine/windows-engine.ps1#L13) | TCM-LOCALE |
| TCM-E-a3fc0e65afe4 | serialized-field | `asset` | [windows/language_packs.go:32](../../windows/language_packs.go#L32) | TCM-PROTOCOL |
| TCM-E-c19f5eb83d71 | serialized-field | `sha256` | [windows/language_packs.go:33](../../windows/language_packs.go#L33) | TCM-PROTOCOL |
| TCM-E-7eede668f0e6 | serialized-field | `app_version` | [windows/language_packs.go:41](../../windows/language_packs.go#L41) | TCM-UPDATE |
| TCM-E-19738f2753ad | serialized-field | `languages` | [windows/language_packs.go:42](../../windows/language_packs.go#L42) | TCM-LOCALE |
| TCM-E-81d699d5f432 | serialized-field | `schema` | [windows/language_packs.go:46](../../windows/language_packs.go#L46) | TCM-PROTOCOL |
| TCM-E-d2bc229440ee | serialized-field | `product` | [windows/language_packs.go:47](../../windows/language_packs.go#L47) | TCM-PROTOCOL |
| TCM-E-8d3aa7fe8bfd | serialized-field | `locale` | [windows/language_packs.go:48](../../windows/language_packs.go#L48) | TCM-LOCALE |
| TCM-E-5d1754c3231d | serialized-field | `revision` | [windows/language_packs.go:49](../../windows/language_packs.go#L49) | TCM-PROTOCOL |
| TCM-E-0590acd35b0d | serialized-field | `released_with` | [windows/language_packs.go:50](../../windows/language_packs.go#L50) | TCM-CARD |
| TCM-E-76e1c7ee435d | serialized-field | `catalog_schema` | [windows/language_packs.go:51](../../windows/language_packs.go#L51) | TCM-DIAGNOSTICS |
| TCM-E-9a3933900ca1 | serialized-field | `message_set_hash` | [windows/language_packs.go:52](../../windows/language_packs.go#L52) | TCM-PROTOCOL |
| TCM-E-6d922ade1dcc | serialized-field | `files` | [windows/language_packs.go:53](../../windows/language_packs.go#L53) | TCM-PROTOCOL |
| TCM-E-24da5565436b | serialized-field | `language` | [windows/language_packs.go:531](../../windows/language_packs.go#L531) | TCM-LOCALE |
| TCM-E-a6858d1c453c | serialized-field | `code` | [windows/localization.go:15](../../windows/localization.go#L15) | TCM-PROTOCOL |
| TCM-E-ab5f82cdc326 | serialized-field | `native_name` | [windows/localization.go:16](../../windows/localization.go#L16) | TCM-LIFECYCLE |
| TCM-E-91f8f3b71ef8 | serialized-field | `english_name` | [windows/localization.go:17](../../windows/localization.go#L17) | TCM-PROTOCOL |
| TCM-E-0f9f2a74e7c5 | serialized-field | `flag` | [windows/localization.go:18](../../windows/localization.go#L18) | TCM-LOCALE |
| TCM-E-6238db613a64 | serialized-field | `built_in` | [windows/localization.go:19](../../windows/localization.go#L19) | TCM-PROTOCOL |
| TCM-E-af0a85b06138 | serialized-field | `installed` | [windows/localization.go:20](../../windows/localization.go#L20) | TCM-WEBPATCH |
| TCM-E-a681e1e09803 | serialized-field | `downloadable` | [windows/localization.go:21](../../windows/localization.go#L21) | TCM-PROTOCOL |
| TCM-E-372e20b218fc | serialized-field | `state` | [windows/localization.go:22](../../windows/localization.go#L22) | TCM-PROTOCOL |
| TCM-E-c00ee82bd553 | serialized-field | `revision` | [windows/localization.go:23](../../windows/localization.go#L23) | TCM-PROTOCOL |
| TCM-E-f7e2ea435d8a | serialized-field | `released_with` | [windows/localization.go:24](../../windows/localization.go#L24) | TCM-CARD |
| TCM-E-a0b370d6dcd6 | serialized-field | `error` | [windows/localization.go:25](../../windows/localization.go#L25) | TCM-DIAGNOSTICS |
| TCM-E-7fc967378c1a | serialized-field | `auto_start_configured` | [windows/main.go:36](../../windows/main.go#L36) | TCM-SERVICE |
| TCM-E-372fe563c935 | serialized-field | `library_roots` | [windows/main.go:40](../../windows/main.go#L40) | TCM-SCOPE |
| TCM-E-99ce692bd2d6 | serialized-field | `source` | [windows/main.go:47](../../windows/main.go#L47) | TCM-PROTOCOL |
| TCM-E-969d57ff15f9 | serialized-field | `enabled` | [windows/main.go:48](../../windows/main.go#L48) | TCM-PROTOCOL |
| TCM-E-fe5949e547cc | serialized-field | `name` | [windows/main.go:52](../../windows/main.go#L52) | TCM-PROTOCOL |
| TCM-E-154caee6796f | serialized-field | `kind` | [windows/main.go:54](../../windows/main.go#L54) | TCM-PROTOCOL |
| TCM-E-fc3af69a5a34 | serialized-field | `online` | [windows/main.go:55](../../windows/main.go#L55) | TCM-PROTOCOL |
| TCM-E-554c755b0795 | serialized-field | `state` | [windows/main.go:56](../../windows/main.go#L56) | TCM-PROTOCOL |
| TCM-E-ed25c5ba85a9 | serialized-field | `evidence` | [windows/main.go:57](../../windows/main.go#L57) | TCM-PROTOCOL |
| TCM-E-dea3a7a6979f | serialized-field | `access_error` | [windows/main.go:58](../../windows/main.go#L58) | TCM-DIAGNOSTICS |
| TCM-E-c7979894215d | serialized-field | `exit_code` | [windows/main.go:66](../../windows/main.go#L66) | TCM-LIFECYCLE |
| TCM-E-54cdff4b7c1c | serialized-field | `message` | [windows/main.go:67](../../windows/main.go#L67) | TCM-PROTOCOL |
| TCM-E-58484773a549 | serialized-field | `log` | [windows/main.go:68](../../windows/main.go#L68) | TCM-DIAGNOSTICS |
| TCM-E-6e74c22cc76e | serialized-field | `blocks_exit` | [windows/main.go:69](../../windows/main.go#L69) | TCM-LIFECYCLE |
| TCM-E-34442724634c | serialized-field | `needs_admin` | [windows/main.go:70](../../windows/main.go#L70) | TCM-PROTOCOL |
| TCM-E-bc75efda196f | serialized-field | `already_admin` | [windows/main.go:71](../../windows/main.go#L71) | TCM-READONLY |
| TCM-E-88160a73eac7 | serialized-field | `language_pack_revision` | [windows/main.go:73](../../windows/main.go#L73) | TCM-LOCALE |
| TCM-E-f3a7cf2ae884 | serialized-field | `language_catalog_hash` | [windows/main.go:74](../../windows/main.go#L74) | TCM-LOCALE |
| TCM-E-85e5691fd5d7 | serialized-field | `running` | [windows/main.go:78](../../windows/main.go#L78) | TCM-PROTOCOL |
| TCM-E-6546937c02a5 | serialized-field | `started_at` | [windows/main.go:79](../../windows/main.go#L79) | TCM-SERVICE |
| TCM-E-3480cf530e87 | serialized-field | `ended_at` | [windows/main.go:80](../../windows/main.go#L80) | TCM-PROTOCOL |
| TCM-E-dbe725646f12 | serialized-field | `duration_ms` | [windows/main.go:81](../../windows/main.go#L81) | TCM-PROTOCOL |
| TCM-E-9875fdfbb225 | serialized-field | `stamp` | [windows/main.go:87](../../windows/main.go#L87) | TCM-PROTOCOL |
| TCM-E-b3c26cd2f857 | serialized-field | `error` | [windows/main.go:88](../../windows/main.go#L88) | TCM-DIAGNOSTICS |
| TCM-E-5978d0a901fa | serialized-field | `app_version` | [windows/main.go:92](../../windows/main.go#L92) | TCM-UPDATE |
| TCM-E-1864a220fb65 | serialized-field | `platform` | [windows/main.go:93](../../windows/main.go#L93) | TCM-PROTOCOL |
| TCM-E-93cf258a4207 | serialized-field | `platform_label` | [windows/main.go:94](../../windows/main.go#L94) | TCM-PROTOCOL |
| TCM-E-fd3f6c933cd3 | serialized-field | `installed` | [windows/main.go:95](../../windows/main.go#L95) | TCM-WEBPATCH |
| TCM-E-36202e4fed55 | serialized-field | `agent_running` | [windows/main.go:96](../../windows/main.go#L96) | TCM-LIFECYCLE |
| TCM-E-bede697df28f | serialized-field | `auto_start` | [windows/main.go:97](../../windows/main.go#L97) | TCM-SERVICE |
| TCM-E-7b98589c349d | serialized-field | `silent_start` | [windows/main.go:98](../../windows/main.go#L98) | TCM-SERVICE |
| TCM-E-8a825a7270f6 | serialized-field | `language` | [windows/main.go:99](../../windows/main.go#L99) | TCM-LOCALE |
| TCM-E-d17adc52df15 | serialized-field | `languages` | [windows/main.go:100](../../windows/main.go#L100) | TCM-LOCALE |
| TCM-E-5871bc3eb3e7 | serialized-field | `agent_pid` | [windows/main.go:101](../../windows/main.go#L101) | TCM-LIFECYCLE |
| TCM-E-3209c51bc1f9 | serialized-field | `last_heartbeat` | [windows/main.go:102](../../windows/main.go#L102) | TCM-SERVICE |
| TCM-E-3e26ed396c60 | serialized-field | `last_cycle` | [windows/main.go:103](../../windows/main.go#L103) | TCM-PROTOCOL |
| TCM-E-d0d23c42af9d | serialized-field | `interval_seconds` | [windows/main.go:104](../../windows/main.go#L104) | TCM-SERVICE |
| TCM-E-a966997f7ae5 | serialized-field | `engine_ready` | [windows/main.go:105](../../windows/main.go#L105) | TCM-READONLY |
| TCM-E-173caa63231e | serialized-field | `python` | [windows/main.go:106](../../windows/main.go#L106) | TCM-PROTOCOL |
| TCM-E-d2e3303b6cd7 | serialized-field | `chrome` | [windows/main.go:107](../../windows/main.go#L107) | TCM-PROTOCOL |
| TCM-E-a0a7bc1d2917 | serialized-field | `indexed_titles` | [windows/main.go:108](../../windows/main.go#L108) | TCM-CATALOG |
| TCM-E-50afdecdbbec | serialized-field | `nfo_total` | [windows/main.go:109](../../windows/main.go#L109) | TCM-READONLY |
| TCM-E-d5cca3939d60 | serialized-field | `cache_count` | [windows/main.go:110](../../windows/main.go#L110) | TCM-CATALOG |
| TCM-E-b6e423c6d756 | serialized-field | `xml_errors` | [windows/main.go:111](../../windows/main.go#L111) | TCM-READONLY |
| TCM-E-f2827cd0e59d | serialized-field | `xml_error_details` | [windows/main.go:112](../../windows/main.go#L112) | TCM-CARD |
| TCM-E-81781c2dbd20 | serialized-field | `web_patch` | [windows/main.go:113](../../windows/main.go#L113) | TCM-WEBPATCH |
| TCM-E-3feba29bd6a0 | serialized-field | `web_version` | [windows/main.go:114](../../windows/main.go#L114) | TCM-UPDATE |
| TCM-E-c4c9af305529 | serialized-field | `libraries` | [windows/main.go:115](../../windows/main.go#L115) | TCM-PROTOCOL |
| TCM-E-638eb50d57ba | serialized-field | `configured_roots` | [windows/main.go:116](../../windows/main.go#L116) | TCM-SCOPE |
| TCM-E-9185b421a423 | serialized-field | `roots_configured` | [windows/main.go:117](../../windows/main.go#L117) | TCM-SCOPE |
| TCM-E-3865e493af5c | serialized-field | `discovered_roots` | [windows/main.go:118](../../windows/main.go#L118) | TCM-SCOPE |
| TCM-E-8a7f534b81b7 | serialized-field | `counts` | [windows/main.go:119](../../windows/main.go#L119) | TCM-PROTOCOL |
| TCM-E-b23806b2bbda | serialized-field | `job` | [windows/main.go:120](../../windows/main.go#L120) | TCM-TASK |
| TCM-E-62b99cf2efdc | serialized-field | `capabilities` | [windows/main.go:121](../../windows/main.go#L121) | TCM-PROTOCOL |
| TCM-E-11584e3f8e90 | serialized-field | `notes` | [windows/main.go:122](../../windows/main.go#L122) | TCM-PROTOCOL |
| TCM-E-3daad310e75c | serialized-field | `paths` | [windows/main.go:123](../../windows/main.go#L123) | TCM-PROTOCOL |
| TCM-E-2b43df45539f | serialized-field | `cleanup_removed` | [windows/main.go:124](../../windows/main.go#L124) | TCM-LEGACY |
| TCM-E-46459809de2c | serialized-field | `service` | [windows/main.go:125](../../windows/main.go#L125) | TCM-SERVICE |
| TCM-E-0d25470fa486 | serialized-field | `extra` | [windows/main.go:126](../../windows/main.go#L126) | TCM-PROTOCOL |
| TCM-E-f891b9a95696 | serialized-field | `action` | [windows/main.go:130](../../windows/main.go#L130) | TCM-DISPATCH |
| TCM-E-cd724b812a14 | serialized-field | `imdb` | [windows/main.go:131](../../windows/main.go#L131) | TCM-PROTOCOL |
| TCM-E-27484c3f0ec9 | serialized-field | `path` | [windows/main.go:132](../../windows/main.go#L132) | TCM-PROTOCOL |
| TCM-E-6b5d9066b8b3 | command | `--agent` | [windows/main.go:161](../../windows/main.go#L161) | TCM-LIFECYCLE |
| TCM-E-a9032a990015 | command | `--headless-action` | [windows/main.go:167](../../windows/main.go#L167) | TCM-LIFECYCLE |
| TCM-E-41811a03cca3 | command | `--login-startup` | [windows/main.go:206](../../windows/main.go#L206) | TCM-SERVICE |
| TCM-E-ffb4341b1516 | serialized-field | `version` | [windows/main.go:241](../../windows/main.go#L241) | TCM-UPDATE |
| TCM-E-3ff53e6cb260 | serialized-field | `itemTypes` | [windows/main.go:244](../../windows/main.go#L244) | TCM-PROTOCOL |
| TCM-E-0b3b00238cf1 | serialized-field | `libraryRoots` | [windows/main.go:251](../../windows/main.go#L251) | TCM-SCOPE |
| TCM-E-f7481e507aeb | serialized-field | `scanStats` | [windows/main.go:252](../../windows/main.go#L252) | TCM-CATALOG |
| TCM-E-92f471634120 | http-route | `/` | [windows/main.go:296](../../windows/main.go#L296) | TCM-LAYOUT |
| TCM-E-ec4fe5d10a06 | http-route | `/assets/TCM_logo_letter_only.png` | [windows/main.go:297](../../windows/main.go#L297) | TCM-DIAGNOSTICS |
| TCM-E-47cf9f9668cf | http-route | `/assets/TCM_logo_tiny.png` | [windows/main.go:298](../../windows/main.go#L298) | TCM-DIAGNOSTICS |
| TCM-E-7ef19fdbd7e9 | http-route | `/api/status` | [windows/main.go:299](../../windows/main.go#L299) | TCM-CATALOG |
| TCM-E-de339442c89d | http-route | `/api/catalog` | [windows/main.go:300](../../windows/main.go#L300) | TCM-DIAGNOSTICS |
| TCM-E-d804b569eac3 | http-route | `/api/action` | [windows/main.go:301](../../windows/main.go#L301) | TCM-DISPATCH |
| TCM-E-ac162d5e852c | http-route | `/api/settings` | [windows/main.go:302](../../windows/main.go#L302) | TCM-LAYOUT |
| TCM-E-185e0b20791b | http-route | `/api/job` | [windows/main.go:303](../../windows/main.go#L303) | TCM-TASK |
| TCM-E-25da9ac8b771 | http-route | `/api/update` | [windows/main.go:304](../../windows/main.go#L304) | TCM-UPDATE |
| TCM-E-741df56c4bbf | http-route | `/api/languages` | [windows/main.go:305](../../windows/main.go#L305) | TCM-LOCALE |
| TCM-E-4df34bea2a2f | http-route | `/api/heartbeat` | [windows/main.go:306](../../windows/main.go#L306) | TCM-SERVICE |
| TCM-E-603b7da781d0 | serialized-field | `generatedAt` | [windows/main.go:519](../../windows/main.go#L519) | TCM-PROTOCOL |
| TCM-E-9b157a5c1cfe | serialized-field | `count` | [windows/main.go:520](../../windows/main.go#L520) | TCM-PROTOCOL |
| TCM-E-6b5b81082e57 | serialized-field | `items` | [windows/main.go:521](../../windows/main.go#L521) | TCM-PROTOCOL |
| TCM-E-d7b923718f3d | serialized-field | `removed` | [windows/main.go:561](../../windows/main.go#L561) | TCM-PROTOCOL |
| TCM-E-57ef5ebf3bcb | command | `service-start` | [windows/main.go:696](../../windows/main.go#L696) | TCM-SERVICE |
| TCM-E-de7a1e4e3283 | command | `service-stop` | [windows/main.go:710](../../windows/main.go#L710) | TCM-SERVICE |
| TCM-E-95ac05eb259f | command | `legacy-cancel` | [windows/main.go:720](../../windows/main.go#L720) | TCM-LEGACY |
| TCM-E-50cc77e9d80e | command | `open-data` | [windows/main.go:734](../../windows/main.go#L734) | TCM-DIAGNOSTICS |
| TCM-E-83a1cb4c32a1 | command | `open-logs` | [windows/main.go:741](../../windows/main.go#L741) | TCM-DIAGNOSTICS |
| TCM-E-73eec2038326 | command | `open-path` | [windows/main.go:749](../../windows/main.go#L749) | TCM-DIAGNOSTICS |
| TCM-E-8fc2a72d1760 | command | `choose-library-root` | [windows/main.go:766](../../windows/main.go#L766) | TCM-SCOPE |
| TCM-E-641cef99e472 | command | `export-diagnostics` | [windows/main.go:774](../../windows/main.go#L774) | TCM-DIAGNOSTICS |
| TCM-E-45c48fb26224 | command | `install` | [windows/main.go:1076](../../windows/main.go#L1076) | TCM-WEBPATCH |
| TCM-E-484bb5c8b9d1 | command | `migrate-legacy` | [windows/main.go:1078](../../windows/main.go#L1078) | TCM-LEGACY |
| TCM-E-7ae7a9c6d833 | command | `disable-integration` | [windows/main.go:1080](../../windows/main.go#L1080) | TCM-WEBPATCH |
| TCM-E-e2c96c08ad22 | command | `start` | [windows/main.go:1082](../../windows/main.go#L1082) | TCM-SERVICE |
| TCM-E-378e78be9bf6 | command | `stop` | [windows/main.go:1084](../../windows/main.go#L1084) | TCM-SERVICE |
| TCM-E-a2c3b19f1f70 | command | `cleanup-legacy` | [windows/main.go:1086](../../windows/main.go#L1086) | TCM-LEGACY |
| TCM-E-3a443c453387 | command | `auto` | [windows/main.go:1089](../../windows/main.go#L1089) | TCM-TASK |
| TCM-E-03cdb21d731f | command | `run` | [windows/main.go:1091](../../windows/main.go#L1091) | TCM-TASK |
| TCM-E-6948cc0750c5 | command | `scan-root` | [windows/main.go:1091](../../windows/main.go#L1091) | TCM-SCOPE |
| TCM-E-ba167419f5f1 | command | `scan-space` | [windows/main.go:1093](../../windows/main.go#L1093) | TCM-SCOPE |
| TCM-E-5ff623975ac0 | command | `diagnose` | [windows/main.go:1109](../../windows/main.go#L1109) | TCM-DIAGNOSTICS |
| TCM-E-0c6c9c72a871 | command | `discover-roots` | [windows/main.go:1109](../../windows/main.go#L1109) | TCM-SCOPE |
| TCM-E-30253f3d9dd5 | command | `rebuild-index` | [windows/main.go:1109](../../windows/main.go#L1109) | TCM-CATALOG |
| TCM-E-ddb3f640f689 | command | `repair-web` | [windows/main.go:1109](../../windows/main.go#L1109) | TCM-WEBPATCH |
| TCM-E-b2307b6aa1be | serialized-field | `name` | [windows/platform_windows.go:175](../../windows/platform_windows.go#L175) | TCM-PROTOCOL |
| TCM-E-c720c950ab84 | serialized-field | `path` | [windows/platform_windows.go:176](../../windows/platform_windows.go#L176) | TCM-PROTOCOL |
| TCM-E-65789d6c8727 | serialized-field | `kind` | [windows/platform_windows.go:177](../../windows/platform_windows.go#L177) | TCM-PROTOCOL |
| TCM-E-a0c7742873de | serialized-field | `libraryRoots` | [windows/platform_windows.go:179](../../windows/platform_windows.go#L179) | TCM-SCOPE |
| TCM-E-f87f170fae35 | serialized-field | `ProcessId` | [windows/platform_windows.go:537](../../windows/platform_windows.go#L537) | TCM-PROTOCOL |
| TCM-E-ad72736e9788 | serialized-field | `ExecutablePath` | [windows/platform_windows.go:538](../../windows/platform_windows.go#L538) | TCM-LAYOUT |
| TCM-E-e405e3b0d1b5 | serialized-field | `CommandLine` | [windows/platform_windows.go:539](../../windows/platform_windows.go#L539) | TCM-PROTOCOL |
| TCM-E-73c36ea842fe | command | `auto` | [windows/platform_windows.go:702](../../windows/platform_windows.go#L702) | TCM-TASK |
| TCM-E-850c4f506551 | command | `run` | [windows/platform_windows.go:702](../../windows/platform_windows.go#L702) | TCM-TASK |
| TCM-E-69f5af0b3a71 | command | `repair-web` | [windows/platform_windows.go:712](../../windows/platform_windows.go#L712) | TCM-WEBPATCH |
| TCM-E-ea4ce85f0069 | command | `disable-integration` | [windows/platform_windows.go:720](../../windows/platform_windows.go#L720) | TCM-WEBPATCH |
| TCM-E-0d008f194df1 | command | `diagnose` | [windows/platform_windows.go:728](../../windows/platform_windows.go#L728) | TCM-DIAGNOSTICS |
| TCM-E-f67bce8d6c6e | command | `rebuild-index` | [windows/platform_windows.go:736](../../windows/platform_windows.go#L736) | TCM-CATALOG |
| TCM-E-92aede59a09b | command | `discover-roots` | [windows/platform_windows.go:744](../../windows/platform_windows.go#L744) | TCM-SCOPE |
| TCM-E-0dc8278b9eeb | serialized-field | `indexedTitles` | [windows/platform_windows.go:836](../../windows/platform_windows.go#L836) | TCM-CATALOG |
| TCM-E-c0e2938888b7 | serialized-field | `catalogCount` | [windows/platform_windows.go:837](../../windows/platform_windows.go#L837) | TCM-DIAGNOSTICS |
| TCM-E-7f35cd0256b3 | serialized-field | `libraries` | [windows/platform_windows.go:841](../../windows/platform_windows.go#L841) | TCM-PROTOCOL |
| TCM-E-08dab27745bf | serialized-field | `onlineRootsScanned` | [windows/platform_windows.go:843](../../windows/platform_windows.go#L843) | TCM-SCOPE |
| TCM-E-9c63f2569977 | serialized-field | `nfoSeen` | [windows/platform_windows.go:844](../../windows/platform_windows.go#L844) | TCM-READONLY |
| TCM-E-3eec3d0105cb | serialized-field | `nfoReparsed` | [windows/platform_windows.go:845](../../windows/platform_windows.go#L845) | TCM-READONLY |
| TCM-E-eb22b81040cf | serialized-field | `technicalSpecsFound` | [windows/platform_windows.go:846](../../windows/platform_windows.go#L846) | TCM-CARD |
| TCM-E-1cd59d50d154 | serialized-field | `webEligibleSpecsFound` | [windows/platform_windows.go:847](../../windows/platform_windows.go#L847) | TCM-CARD |
| TCM-E-a0fea5602e9c | serialized-field | `episodeSpecsExcludedFromWeb` | [windows/platform_windows.go:848](../../windows/platform_windows.go#L848) | TCM-CARD |
| TCM-E-6c7ed6cc1866 | serialized-field | `xmlReadErrors` | [windows/platform_windows.go:849](../../windows/platform_windows.go#L849) | TCM-READONLY |
| TCM-E-84ccae386f00 | serialized-field | `scanStats` | [windows/platform_windows.go:850](../../windows/platform_windows.go#L850) | TCM-CATALOG |
| TCM-E-02e8787d04a0 | serialized-field | `generatedAt` | [windows/platform_windows.go:882](../../windows/platform_windows.go#L882) | TCM-PROTOCOL |
| TCM-E-67a92683aa13 | serialized-field | `count` | [windows/platform_windows.go:883](../../windows/platform_windows.go#L883) | TCM-PROTOCOL |
| TCM-E-bc11e1fbd342 | serialized-field | `errors` | [windows/platform_windows.go:884](../../windows/platform_windows.go#L884) | TCM-DIAGNOSTICS |
| TCM-E-3b06c385c6fb | serialized-field | `version` | [windows/platform_windows.go:930](../../windows/platform_windows.go#L930) | TCM-UPDATE |
| TCM-E-17ead366e139 | serialized-field | `items` | [windows/platform_windows.go:931](../../windows/platform_windows.go#L931) | TCM-PROTOCOL |
| TCM-E-38f0cbcc393a | serialized-field | `itemTypes` | [windows/platform_windows.go:932](../../windows/platform_windows.go#L932) | TCM-PROTOCOL |
| TCM-E-2c2509a7314a | serialized-field | `ok` | [windows/platform_windows.go:1440](../../windows/platform_windows.go#L1440) | TCM-PROTOCOL |
| TCM-E-0cf5ed8f36a2 | serialized-field | `request_id` | [windows/platform_windows.go:1446](../../windows/platform_windows.go#L1446) | TCM-PROTOCOL |
| TCM-E-c43cfd77e7a4 | serialized-field | `action` | [windows/platform_windows.go:1447](../../windows/platform_windows.go#L1447) | TCM-DISPATCH |
| TCM-E-4caeeec19e35 | serialized-field | `allow` | [windows/platform_windows.go:1448](../../windows/platform_windows.go#L1448) | TCM-PROTOCOL |
| TCM-E-6e5ed967d81b | serialized-field | `message` | [windows/platform_windows.go:1449](../../windows/platform_windows.go#L1449) | TCM-PROTOCOL |
| TCM-E-6af483b2df38 | serialized-field | `time` | [windows/platform_windows.go:1450](../../windows/platform_windows.go#L1450) | TCM-PROTOCOL |
| TCM-E-5a28c81fc61d | serialized-field | `required` | [windows/service_controller.go:33](../../windows/service_controller.go#L33) | TCM-PROTOCOL |
| TCM-E-a4e140e9285f | serialized-field | `agent_running` | [windows/service_controller.go:34](../../windows/service_controller.go#L34) | TCM-LIFECYCLE |
| TCM-E-00f82de7b101 | serialized-field | `auto_start` | [windows/service_controller.go:35](../../windows/service_controller.go#L35) | TCM-SERVICE |
| TCM-E-2dbc5909ec67 | serialized-field | `scheduled_task` | [windows/service_controller.go:36](../../windows/service_controller.go#L36) | TCM-TASK |
| TCM-E-b3b324384218 | serialized-field | `artifacts` | [windows/service_controller.go:37](../../windows/service_controller.go#L37) | TCM-PROTOCOL |
| TCM-E-5a84127ebaa3 | serialized-field | `web_patch` | [windows/service_controller.go:38](../../windows/service_controller.go#L38) | TCM-WEBPATCH |
| TCM-E-b747cd82f420 | serialized-field | `unsafe_patch` | [windows/service_controller.go:39](../../windows/service_controller.go#L39) | TCM-WEBPATCH |
| TCM-E-32dd2f3f3ded | serialized-field | `items` | [windows/service_controller.go:40](../../windows/service_controller.go#L40) | TCM-PROTOCOL |
| TCM-E-2c6832c9db72 | serialized-field | `processes` | [windows/service_controller.go:41](../../windows/service_controller.go#L41) | TCM-PROTOCOL |
| TCM-E-503e58c62c66 | serialized-field | `pid` | [windows/service_controller.go:45](../../windows/service_controller.go#L45) | TCM-PROTOCOL |
| TCM-E-d448e88feec4 | serialized-field | `path` | [windows/service_controller.go:46](../../windows/service_controller.go#L46) | TCM-PROTOCOL |
| TCM-E-a0ac99a333f3 | serialized-field | `-` | [windows/service_controller.go:47](../../windows/service_controller.go#L47) | TCM-PROTOCOL |
| TCM-E-1a0c9711c86a | serialized-field | `state` | [windows/service_controller.go:51](../../windows/service_controller.go#L51) | TCM-PROTOCOL |
| TCM-E-09204e71b588 | serialized-field | `running` | [windows/service_controller.go:52](../../windows/service_controller.go#L52) | TCM-PROTOCOL |
| TCM-E-45186a7c8efe | serialized-field | `message` | [windows/service_controller.go:53](../../windows/service_controller.go#L53) | TCM-PROTOCOL |
| TCM-E-303798bca581 | serialized-field | `last_started_at` | [windows/service_controller.go:54](../../windows/service_controller.go#L54) | TCM-SERVICE |
| TCM-E-f6955a6ad6d8 | serialized-field | `legacy` | [windows/service_controller.go:55](../../windows/service_controller.go#L55) | TCM-LEGACY |
| TCM-E-7471d464d734 | serialized-field | `legacy_prompt_pending` | [windows/service_controller.go:56](../../windows/service_controller.go#L56) | TCM-LEGACY |
| TCM-E-9b71fb0868ab | serialized-field | `card_lease_active` | [windows/service_controller.go:57](../../windows/service_controller.go#L57) | TCM-CARD |
| TCM-E-7db88d2dfe70 | serialized-field | `version` | [windows/service_controller.go:61](../../windows/service_controller.go#L61) | TCM-UPDATE |
| TCM-E-5172b463e8d1 | serialized-field | `manager_version` | [windows/service_controller.go:62](../../windows/service_controller.go#L62) | TCM-UPDATE |
| TCM-E-8a210e36bad6 | serialized-field | `web_card_version` | [windows/service_controller.go:63](../../windows/service_controller.go#L63) | TCM-CARD |
| TCM-E-655b178e0e52 | serialized-field | `session_id` | [windows/service_controller.go:64](../../windows/service_controller.go#L64) | TCM-SERVICE |
| TCM-E-56e9d09a5036 | serialized-field | `sequence` | [windows/service_controller.go:65](../../windows/service_controller.go#L65) | TCM-SERVICE |
| TCM-E-bb35091ec86b | serialized-field | `enabled` | [windows/service_controller.go:66](../../windows/service_controller.go#L66) | TCM-PROTOCOL |
| TCM-E-676d54e82e32 | serialized-field | `updated_at` | [windows/service_controller.go:67](../../windows/service_controller.go#L67) | TCM-UPDATE |
| TCM-E-e81d3b983e73 | serialized-field | `expires_at` | [windows/service_controller.go:68](../../windows/service_controller.go#L68) | TCM-PROTOCOL |
| TCM-E-0b65cd898dad | tray-action | `menuRestore` | [windows/tray_windows.go:59](../../windows/tray_windows.go#L59) | TCM-WEBPATCH |
| TCM-E-268bb58c632f | tray-action | `menuExit` | [windows/tray_windows.go:60](../../windows/tray_windows.go#L60) | TCM-LIFECYCLE |
| TCM-E-8b8e19f1b316 | serialized-field | `tag_name` | [windows/update.go:29](../../windows/update.go#L29) | TCM-PROTOCOL |
| TCM-E-8691bdedec1a | serialized-field | `html_url` | [windows/update.go:30](../../windows/update.go#L30) | TCM-PROTOCOL |
| TCM-E-361d791d85ae | serialized-field | `draft` | [windows/update.go:31](../../windows/update.go#L31) | TCM-PROTOCOL |
| TCM-E-a4192fdab3eb | serialized-field | `prerelease` | [windows/update.go:32](../../windows/update.go#L32) | TCM-CARD |
| TCM-E-411c8f0c21eb | serialized-field | `published_at` | [windows/update.go:33](../../windows/update.go#L33) | TCM-PROTOCOL |
| TCM-E-2e61c22d9732 | serialized-field | `assets` | [windows/update.go:34](../../windows/update.go#L34) | TCM-PROTOCOL |
| TCM-E-e09d192d14cb | serialized-field | `name` | [windows/update.go:38](../../windows/update.go#L38) | TCM-PROTOCOL |
| TCM-E-43a23f4e12c5 | serialized-field | `browser_download_url` | [windows/update.go:39](../../windows/update.go#L39) | TCM-PROTOCOL |
| TCM-E-b03238f78933 | serialized-field | `schema_version` | [windows/update.go:43](../../windows/update.go#L43) | TCM-UPDATE |
| TCM-E-b966ee57d4f4 | serialized-field | `etag` | [windows/update.go:44](../../windows/update.go#L44) | TCM-PROTOCOL |
| TCM-E-6a05bc7c0ba4 | serialized-field | `checked_at` | [windows/update.go:45](../../windows/update.go#L45) | TCM-PROTOCOL |
| TCM-E-a3f9de553715 | serialized-field | `release` | [windows/update.go:46](../../windows/update.go#L46) | TCM-CARD |
| TCM-E-28cede620562 | control | `openSettings` | [windows/web/index.html:54](../../windows/web/index.html#L54) | TCM-LAYOUT |
| TCM-E-149c2098fefb | control | `refreshLibrary` | [windows/web/index.html:54](../../windows/web/index.html#L54) | TCM-SCOPE |
| TCM-E-45e55226da26 | control | `bannerAction` | [windows/web/index.html:56](../../windows/web/index.html#L56) | TCM-DISPATCH |
| TCM-E-3ea1e84ef7b8 | control | `serviceButton` | [windows/web/index.html:59](../../windows/web/index.html#L59) | TCM-SERVICE |
| TCM-E-447990b74203 | control | `catalogMovieTab` | [windows/web/index.html:64](../../windows/web/index.html#L64) | TCM-DIAGNOSTICS |
| TCM-E-1e2f5178400c | control | `catalogSearch` | [windows/web/index.html:64](../../windows/web/index.html#L64) | TCM-DIAGNOSTICS |
| TCM-E-d73910c72244 | control | `catalogSpecFilter` | [windows/web/index.html:64](../../windows/web/index.html#L64) | TCM-CARD |
| TCM-E-882dd3413201 | control | `catalogTvTab` | [windows/web/index.html:64](../../windows/web/index.html#L64) | TCM-DIAGNOSTICS |
| TCM-E-f1e90055b100 | control | `autoStart` | [windows/web/index.html:72](../../windows/web/index.html#L72) | TCM-SERVICE |
| TCM-E-e00434335d21 | control | `language` | [windows/web/index.html:72](../../windows/web/index.html#L72) | TCM-LOCALE |
| TCM-E-57822d5e02c4 | control | `languageCurrent` | [windows/web/index.html:72](../../windows/web/index.html#L72) | TCM-LOCALE |
| TCM-E-a12397349bc9 | control | `silentStart` | [windows/web/index.html:72](../../windows/web/index.html#L72) | TCM-SERVICE |
| TCM-E-7c749e79e8fa | control | `addLibraryRoot` | [windows/web/index.html:73](../../windows/web/index.html#L73) | TCM-SCOPE |
| TCM-E-edc7ed3830c3 | control | `chooseLibraryRoot` | [windows/web/index.html:73](../../windows/web/index.html#L73) | TCM-SCOPE |
| TCM-E-7a30ca4181b8 | control | `manualRootPath` | [windows/web/index.html:73](../../windows/web/index.html#L73) | TCM-SCOPE |
| TCM-E-44039d9774b6 | control | `saveLibraryRoots` | [windows/web/index.html:73](../../windows/web/index.html#L73) | TCM-SCOPE |
| TCM-E-880224997e11 | control | `interval` | [windows/web/index.html:74](../../windows/web/index.html#L74) | TCM-SERVICE |
| TCM-E-29624eef46e4 | control | `cancelCardInstall` | [windows/web/index.html:76](../../windows/web/index.html#L76) | TCM-WEBPATCH |
| TCM-E-28176bccfebb | control | `checkCardUpdate` | [windows/web/index.html:76](../../windows/web/index.html#L76) | TCM-CARD |
| TCM-E-93d096f1a515 | control | `confirmCardInstall` | [windows/web/index.html:76](../../windows/web/index.html#L76) | TCM-WEBPATCH |
| TCM-E-0f67d2e50a0e | control | `cancelLegacy` | [windows/web/index.html:80](../../windows/web/index.html#L80) | TCM-LEGACY |
| TCM-E-d81f551ae158 | control | `confirmLegacy` | [windows/web/index.html:80](../../windows/web/index.html#L80) | TCM-LEGACY |
| TCM-E-b23517cdc681 | delegated-control | `data-tv-auto` | [windows/web/index.html:172](../../windows/web/index.html#L172) | TCM-TASK |
| TCM-E-25832e9c6dff | delegated-control | `data-tv-season` | [windows/web/index.html:172](../../windows/web/index.html#L172) | TCM-SCOPE |
| TCM-E-312f5b824e46 | delegated-control | `data-tv-show` | [windows/web/index.html:172](../../windows/web/index.html#L172) | TCM-LIFECYCLE |
| TCM-E-5db20d4c9c31 | delegated-control | `data-close-modal` | [windows/web/index.html:191](../../windows/web/index.html#L191) | TCM-LIFECYCLE |
| TCM-E-0f08ad6ec64c | frontend-action | `scan-space` | [windows/web/index.html:191](../../windows/web/index.html#L191) | TCM-SCOPE |
| TCM-E-9fc802d0cd7e | delegated-control | `data-root-remove` | [windows/web/index.html:192](../../windows/web/index.html#L192) | TCM-SCOPE |
| TCM-E-643e3e729590 | delegated-control | `data-root-toggle` | [windows/web/index.html:192](../../windows/web/index.html#L192) | TCM-SCOPE |
| TCM-E-4cae5ffae7b0 | delegated-control | `data-scan-root` | [windows/web/index.html:192](../../windows/web/index.html#L192) | TCM-SCOPE |
| TCM-E-771160393c1b | frontend-action | `scan-root` | [windows/web/index.html:192](../../windows/web/index.html#L192) | TCM-SCOPE |
| TCM-E-9996275dfc47 | delegated-control | `data-catalog-key` | [windows/web/index.html:194](../../windows/web/index.html#L194) | TCM-DIAGNOSTICS |
| TCM-E-fb6b915019df | delegated-control | `data-catalog-open` | [windows/web/index.html:194](../../windows/web/index.html#L194) | TCM-DIAGNOSTICS |
| TCM-E-ec9c6e9ff55b | delegated-control | `data-copy-path` | [windows/web/index.html:194](../../windows/web/index.html#L194) | TCM-DIAGNOSTICS |
| TCM-E-77ce3a6664f5 | frontend-action | `open-path` | [windows/web/index.html:194](../../windows/web/index.html#L194) | TCM-DIAGNOSTICS |
| TCM-E-e4098b8808b5 | frontend-action | `choose-library-root` | [windows/web/index.html:196](../../windows/web/index.html#L196) | TCM-SCOPE |
| TCM-E-b34ea016c6c6 | frontend-action | `run` | [windows/web/index.html:196](../../windows/web/index.html#L196) | TCM-TASK |
| TCM-E-a19d094bbb23 | frontend-action | `service-start` | [windows/web/index.html:196](../../windows/web/index.html#L196) | TCM-SERVICE |
| TCM-E-6e6c28dc4dbf | frontend-action | `legacy-cancel` | [windows/web/index.html:197](../../windows/web/index.html#L197) | TCM-LEGACY |
| TCM-E-cdd812eee333 | frontend-action | `migrate-legacy` | [windows/web/index.html:197](../../windows/web/index.html#L197) | TCM-LEGACY |

## 尚未关闭的完整性检查

- 动态生成菜单/控件、事件委托和原生系统分支需要逐项运行核对；正则发现器不证明其完整性。
- unclassified 项必须完成语义复核后才可关闭步骤 2。
- 修改默认行为、删除旧选项或替换内部命令必须更新台账并说明用户可观察结果。
- Rust 业务核心尚未实现，因此新旧差分仍为待执行，不伪造新版预期输出。
