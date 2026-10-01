# Tech Card Manager 重写执行记录

- 当前共用规划位于配对 ITM 仓库的 `docs/rewrite/16-replan-2026-09-13.md`；两仓库并排检出时可[直接打开](../../../IMDb-Tech-Manager/docs/rewrite/16-replan-2026-09-13.md)。完整 TCM → 完整 ITM → 五目标系统验收 → 发行准备；各自保持 v4.1.0 界面和行为，TCM 只读、无 AI、手动更新。当前已定向清理额外入口和冲突指令，完整产品迁移与验收仍未完成；维护者本阶段要求有效额度剩余 10% 停工。禁止提交、打包、发布、标签、GitHub 同步和子代理。
- [当前 TCM 源码断点与既有证据](15-baseline-only-correction.md)：首次索引失败状态、服务/helper 清理和重试已完成源码接线；末尾继续记录生命周期与界面差异。真实权限弹窗、托盘/登录会话、Emby 客户端和五目标原生验收仍未完成。

- [交付范围与重写规则](00-scope.md)
- [功能台账](01-features.md) / [机器台账](features.json)
- [基线验证方法](02-baseline.md)
- [平台验收与阻塞](03-platforms.md)
- [可重复执行入口](../../rewrite/README.md)

完成状态必须以执行证据为准；不能把入口覆盖检查或工程骨架当作功能迁移完成。

- [前四步状态与证据](04-status.md)
- [前置调查和阻塞](05-findings.md)

- [原始数据与迁移契约](06-data-contracts.md)
- [最终 DMG 哈希、安装、IPC 与退出证据](evidence/native-smoke.json)

- [第 5–8 步实现、验证和剩余工作](07-implementation.md)

- [维护者模拟 NFO 库：101 份样本验收](08-sample-library.md)

- [云端验收与本地交付进度](09-cloud-validation.md)

- [历史第 9–12 步清单及证据](10-completion-checklist.md)：保留查证，不作为当前执行顺序或授权；冲突要求以共用规划及最新用户指令为准。
