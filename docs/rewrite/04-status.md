# 前四步执行状态

> 本页保留前期阶段的执行记录；后续云端环境与验收进展见 [云端记录](09-cloud-validation.md)。

## 当前结论

| 步骤 | 已完成内容 | 尚未完成 |
| --- | --- | --- |
| 1 | 5 个目标、9 个包、系统/发行版目标、隔离规则及分支指令冻结 | 实际支持认证由步骤 4 决定 |
| 2 | 259 个静态入口/字段/事件锚点，行为分组、新版去向、各平台状态及漂移检查 | 所有动态用户流程和原版真实运行逐项复核；静态分组不等于完整语义审计 |
| 3 | 旧版测试运行器、隔离环境、证据分类和新增真实引擎行为测试 | 全部真实平台行为、新旧 Rust 业务差分、崩溃恢复扩展 |
| 4 | 五目标 CI 与九包配置；macOS ARM64 测试 DMG 构建、隔离安装及原生窗口验证 | Windows/Linux 执行、更新/卸载全链路、最低系统、IMDb 可用数据和 Emby 权限集成 |

基线报告统计：`{"passed": 29, "blocked": 2}`。它包含静态/行为混合测试，不能按数字推算产品验收覆盖率。

## 执行入口

```sh
python3 tools/rewrite/inventory.py
python3 tools/rewrite/baseline.py --output build/rewrite/baseline-local.json
cd rewrite
npm ci
npm run build
npm run tauri build -- --target aarch64-apple-darwin --bundles dmg -- --locked
```

CI `.github/workflows/rewrite-validation.yml` 包含全部目标，权限只读，不签名、不发布。本轮未推送或触发 GitHub Actions。GitHub Windows runner 管理员/UAC 默认环境不替代标准用户权限验收。

## 证据

- [基线执行报告](evidence/baseline-local.json)
- [逐包状态](evidence/platform-matrix.json)
- [入口台账](01-features.md)
- [签名与外部系统调查](05-findings.md)

未经真实目标系统运行的项目保持 blocked/unverified。前四步尚未整体通过完成条件。维护者已授权推进第 5–8 步；新增实现和证据见 [迁移执行记录](07-implementation.md)，原有平台阻塞继续保留。

- [原始数据与迁移契约](06-data-contracts.md)
- [最终 DMG 哈希、安装、IPC 与退出证据](evidence/native-smoke.json)
