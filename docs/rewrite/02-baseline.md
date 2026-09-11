# 旧行为基线

运行：`python3 tools/rewrite/baseline.py --output build/rewrite/baseline-local.json`。Python 原引擎兼容范围仍为 3.8–3.11；本机使用 /usr/bin/python3 (3.9.6)。日志保留在忽略的 build/rewrite/baseline-logs，不进入正式产物。

- Python 测试在各自隔离目录运行，Path.home 重定向，不改用户 HOME 配置；模拟 HTTP 仅监听 loopback。
- 现有 Python contracts 含静态和行为混合断言，报告不得将其全部描述为真实运行验证。
- ITM 新增 itm_behavior.py 直接调用原始引擎，测试只写规格、BOM/换行/权限/备份、预演、fsync/替换失败、并发修改、源 hash、根目录边界。
- TCM 新增 tcm_behavior.ps1 通过 AST 仅加载原始引擎函数，不执行生产入口；在 Windows 上验证四类 NFO、归属、字节/mtime、坏XML、事务替换和锁冲突。未在 Windows 实际执行之前状态是 blocked。
- TCM 原有 Go 源码检查脚本 go test -c 只编译；新报告将它明确分类为 cross-compile-only。Windows 需要另执行 go test / go test -race、PowerShell 5.1 和真实 Emby/托盘测试。
- Rust 业务核心尚未迁移，新旧差分仅建立接口与样本基线，不伪造完成。后续需要冻结输入并比较语义结果、允许修改的字节区域、操作状态和恢复结果。

## 基线发现

ITM 的 backend_native_localization_contract 仍断言根目录 PRIVACY.en.md/TERMS.en.md；文件在 v4.1.0 后的文档整理已移动至 docs/legal。本轮仅修正测试路径，保持原断言目的。未更改媒体业务。

开发沙箱曾阻止 loopback HTTP 与浏览器测试；经允许的环境重试后，以重试证据为准。环境失败与产品缺陷分别记录。

## 下一批必需的行为证据

- 各入口的动态菜单、选择范围、持久化和错误反馈逐项验收。
- NFO/Emby 事务崩溃重启恢复、undo 日志丢失、网络共享并发写入和重放。
- AI 缓存、费用、截断/限流、任务暂停恢复保持旧测试，并扩展为新 Rust 核心对照。
- TCM 实际浏览器加载、Emby DOM、停止后的租约与卡片失效。

以上未执行项不能因已有测试通过而视为完成。
