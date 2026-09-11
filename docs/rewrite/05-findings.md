# 前置调查与已知问题

> 本页保留前期阶段的执行记录；后续云端环境与验收进展见 [云端记录](09-cloud-validation.md)。

## OTA 格式桥接（阻塞）

原 ITM 的 macos/update.go 对 .zip.sig 做 base64 解码并要求 64 字节 Ed25519 签名，公钥要求 32 字节，直接对归档字节验签。Tauri updater 使用 minisign_verify 的 PublicKey/Signature 格式，不能直接把原 .sig 当作新的 Tauri 签名。

依据：ITM 仓库 macos/update.go:638（TCM 无此文件，不建立跨仓库相对链接）；[Tauri 官方实现](https://raw.githubusercontent.com/tauri-apps/plugins-workspace/v2/plugins/updater/src/updater.rs)。TCM 旧更新入口也要单独对照安装方式和资产选择，不假定继承 ITM 的密钥。

结论是格式不直接兼容，不是现有密钥失效。现有密钥未访问、生成、复制、转换或替换。后续需决定并验证保留既有信任的签名适配、旧客户端桥接、更新通道隔离。未经验证不接生产更新源。

## IMDb 获取（阻塞）

本机 ITM Rust 固定请求收到 HTTP 202，HTTP 通路可达，但没有可用 Technical Specs 的解析证明。后续验证应用自有 WebView 获取与页面解析，不能宣称已摆脱获取链路限制。

## Emby 集成（阻塞）

本轮未操作真实 Emby。目录读取不证明维护权限、动态资源发布、修复、卸载及恢复。需要隔离 Emby 测试实例，五个目标分别核对路径、部署版本及最小权限。

## 旧版法律链接（已定位，待迁移修复）

ITM 当前 Web UI 的英文隐私/条款链接仍指向仓库根目录 PRIVACY.en.md/TERMS.en.md，而当前文件位于 docs/legal。已修正旧测试对文件位置的过时断言；没有把修正测试宣称为已修复 UI 链接。该已有问题单列，不作为新实现必须保留的行为。

## 基线测试的证据边界

TCM 的旧测试脚本 go test -c 只证明编译。Windows 执行及 PowerShell 5.1 仍需真实环境。基线测试运行器曾出现共享隔离目录污染，已改为每项测试独立目录并对受影响用例复验。沙箱端口和浏览器阻断也已与产品失败分离。
