# TCM 桌面技术验证工程

已包含 Rust 媒体读取核心、持久化扫描任务及 Vue 读取工作台；第 5–8 步仍在迁移中。独立应用 ID、独立测试数据，不连接正式 OTA。版本 4.1.0 是基线元数据，不可作为正式交付。

## 开发

安装 Rust 1.98.1、Node 24 和平台依赖后，在此目录执行：

```sh
npm ci
npm run build
npm run tauri dev
```

## 本机验证包

```sh
npm run tauri build -- --target aarch64-apple-darwin --bundles dmg
```

Windows target 分别为 x86_64-pc-windows-msvc、aarch64-pc-windows-msvc，bundle 为 nsis；Linux target 分别为 x86_64-unknown-linux-gnu、aarch64-unknown-linux-gnu，bundles 为 appimage,deb,rpm。应用默认不生成更新签名，不带更新公钥或 endpoints；更新演练属于待完成项，不能通过生成临时密钥绕过现有密钥要求。

在支持图形桌面的实际系统中启动后，检查 IPC、目录读取、隔离文件读写、原生凭据、固定 URL 请求、菜单/托盘、二次启动和退出。网络返回 200 也不等于 IMDb 内容可解析或 Emby 卡片正常。

可用环境变量 `REWRITE_PROBE_REPORT` 指定本地 JSONL 证据路径，`REWRITE_PROBE_AUTOCLOSE=1` 在前端 mounted 并完成 IPC 后退出；它们不证明屏幕真实渲染。不要指向有价值的已有文件。

完整范围与待验收项见 [重写文档](../docs/rewrite/README.md)。

## 业务核心验证

```sh
cd src-tauri
cargo test --locked -p tcm-core
cargo run --locked -q -p tcm-core --example export_types -- --check ../src/contracts.ts
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```

读取工作台的根目录、索引、历史位于此验证应用独立数据目录的 `workspace.sqlite`。Movie/TV 分别选择扫描范围；不会自动导入旧设置或默认处理全库。停止应用后再次打开可恢复历史，中断任务需明确恢复。

完整行为迁移状态见 [第 5–8 步记录](../docs/rewrite/07-implementation.md)。
