# TCM v5.0.0 功能与界面迁移工程

目标是 Rust + Tauri 2 + TypeScript + Vue 3/Vite 运行原产品；以 `windows/web/index.html` 为界面和行为基线，原文件位于仓库根目录下。保留只读 NFO 索引、Emby 卡片、媒体目录、维护/诊断、语言及原手动更新；没有 AI 或应用内自动安装器。 复用已有实现，不重新搭建验证工作台。完整 UI、业务与平台验收仍未完成。

独立应用 ID 和数据目录继续用于开发隔离；v5.0.0 是当前换栈版本，v4.1.0 是功能、界面和操作基线。当前执行约束和状态见 [重写规划入口](../docs/rewrite/README.md)，旧 12 步与旧验收记录只作查证。

## 开发与验证

使用仓库锁定的工具链和依赖。依赖已安装时直接复用；需要恢复 Node 依赖时运行 `npm ci`。原生开发入口：

```sh
npm run tauri dev -- --no-watch
```

仅 `npm run dev` 的浏览器页面没有 Tauri IPC，只可检查布局。原生开发必须验证窗口可操作、实际 IPC、对应原用户流程及退出清理；进程启动或探针成功不等于界面已显示。使用隔离数据，避免触碰生产媒体和正式应用。

按修改范围选择检查。例如移除废弃 IPC 包装层时，可以检查桌面命令编译和保留的数据兼容测试：

```sh
cd src-tauri
cargo check --locked -p tcm-validation --bin tcm-validation
cargo test --locked -p tcm-core --test migration --test history --test startup_migration
```

前端修改运行相关前端回归与类型检查；核心修改运行对应行为和安全回归。不要把每个小改动自动升级为无关全量构建。受影响的平台和 ARM64 检查随开发推进，缺少实际环境时明确记录，不用交叉编译代替实机。

`REWRITE_PROBE_REPORT` / `REWRITE_PROBE_AUTOCLOSE` 是现有隔离验收工具；保留必要验证用途，不进入正常产品 UI，也不据此声称像素或业务通过。探针输出不得覆盖有价值的文件。

## 交付边界

每产品 5 目标、9 包：macOS ARM64 DMG，Windows x64/ARM64 NSIS，Linux x64/ARM64 AppImage/DEB/RPM；无 macOS Intel。维护者于 2026-10-02 授权 TCM 正式制包、GitHub 源码同步/Actions 与 v5.0.0 发布，接受临时签名且未做 Apple 公证的 macOS DMG。九包与发行输入实际核对后才执行最终标签/发布，不覆盖旧版本。不使用子代理、不改 ITM；旧源码、用户数据、密钥、有效测试和发行输入保留。真实 Emby、完整原生界面/操作与 Windows/Linux 运行验收暂缓，不计通过。

数据兼容后端按原版正常入口接线；不要恢复已移除的通用数据导入、独立历史浏览面板或其 Tauri 命令。底层迁移/历史读取和有效回归继续保留；它们的存在不等于完整旧数据迁移已通过。

## v5.0.0 发行准备

开发仍默认使用隔离的 validation 身份。维护者已授权正式制包及 GitHub Actions；真实 Emby、完整原生界面/操作与 Windows/Linux 运行验收仍暂缓，未记为通过。

正式构建同时设置 `TCM_RELEASE_BUILD=1` 并合并 `src-tauri/tauri.release.conf.json`。身份为 `io.github.eric-hou1997.tcm`，包内程序为 `Tech-Card-Manager`；维护辅助程序使用同一发行标记验证父程序及维护目录。macOS 使用临时签名，未做 Apple 公证。

```sh
cd rewrite
TCM_RELEASE_BUILD=1 npm run tauri build -- --config src-tauri/tauri.release.conf.json --target aarch64-apple-darwin --bundles dmg -- --locked
```

`src-tauri/core/assets/release-packages.json` 是五目标九包和手动更新包名的共同来源。`tools/release.py` 检查主程序与辅助程序架构并收集规范包名；不会安装、打标签或发布。可以设置绝对路径 `CARGO_TARGET_DIR` 将可再生成缓存放在项目外。

外部语言包使用 `rewrite/language_catalog.json`，嵌入目录为 `src-tauri/core/assets/language_catalog.json`；旧版目录与 r1 翻译保持不变。

```sh
python3 tools/build-language-packs.py --catalog rewrite/language_catalog.json --embedded-catalog rewrite/src-tauri/core/assets/language_catalog.json --additional-web rewrite/src/assets/baseline-languages.json --require-complete --app-version v5.0.0
```

上一条语言检查从仓库根目录执行。发行说明输入为 `packaging/v5.0.0/CHANGELOG.zh-CN.txt` 与 `CHANGELOG.en-US.txt`。当前版本的实际包检查与发布状态以 `docs/rewrite/15-baseline-only-correction.md` 为准。
