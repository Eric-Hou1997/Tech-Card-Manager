<div align="center">

# Tech Card Manager

**简体中文** | [繁體中文](./README.zh-Hant.md) | [English](./README.en.md) | [Français](./README.fr.md) | [Русский](./README.ru.md) | [日本語](./README.ja.md) | [Español](./README.es.md) | [ไทย](./README.th.md)

<p align="center">

[![Release](https://img.shields.io/github/v/release/Eric-Hou1997/Tech-Card-Manager?label=release)](https://github.com/Eric-Hou1997/Tech-Card-Manager/releases)
[![Downloads](https://img.shields.io/github/downloads/Eric-Hou1997/Tech-Card-Manager/total?label=downloads)](https://github.com/Eric-Hou1997/Tech-Card-Manager/releases)
[![Stars](https://img.shields.io/github/stars/Eric-Hou1997/Tech-Card-Manager?style=flat&logo=github)](https://github.com/Eric-Hou1997/Tech-Card-Manager/stargazers)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](https://github.com/Eric-Hou1997/Tech-Card-Manager/pulls)

</p>

<img src="./windows/assets/TCM_logo.png" alt="Tech Card Manager" width="220">

**Technical specifications card management and Emby media library integration tool.**

面向 **Emby Server** 的只读 NFO 索引与 Technical Specifications 卡片管理工具。

</div>

---

## 🎬 项目简介

**Tech Card Manager（TCM）** 用于把已经存在于媒体 NFO 中的 **Technical Specifications（影视技术规格）** 带到 Emby 的实际浏览界面中。

现在的大多数媒体库已经可以很好地展示：

* 片名
* 演员
* 年份
* 分辨率
* 视频编码
* 音频编码
* HDR / Dolby Vision 等媒体流信息

但一部影视作品在制作阶段使用了：

* 什么摄影机
* 什么镜头
* 什么胶片或数字采集格式
* 怎样的摄影工艺
* 什么声音格式
* 什么画幅比例
* 怎样的母版与放映格式

这些信息通常并没有得到完整、结构化的展示。

TCM 会以**只读方式**扫描用户配置的电影和电视剧 NFO，建立自己的派生索引，并通过 Emby Web Card 将这些技术规格展示在媒体详情页中。

TCM 的核心目标包括：

* 让已有 Technical Specifications 真正进入媒体库浏览体验
* 保持媒体 NFO 原文件只读
* 不接管用户现有的元数据工作流
* 将电影与电视剧作为独立空间管理和刷新
* 安全地维护 Emby Technical Specifications Web Card
* 提供长期运行所需的 Windows GUI、系统托盘和本地服务能力
* 让重要状态、错误和维护操作尽可能可观察、可验证
* 内置简体中文、繁體中文和 English (United States)，并通过版本绑定的语言包支持法语、俄语、日语、西班牙语和泰语

---

## ✨ 核心能力

### 📚 只读 NFO 索引

TCM 可以读取用户配置的电影与电视剧媒体库目录，从 NFO 中提取并索引 Technical Specifications 以及其他用于展示和识别的只读信息。

媒体 NFO 是 TCM 的**只读数据源**。

TCM 不会：

* 修改 NFO
* 自动整理 NFO
* 自动“修复” NFO
* 写入 Technical Specifications
* 生成或删除标签
* 修改标签 ownership
* 改变 NFO 原有内容

索引结果由 TCM 自己维护。

媒体原始数据仍然由用户以及原本使用的元数据管理工具掌控。

---

### 🖥️ Emby Technical Specifications Web Card

TCM 会将索引后的技术规格转换为适合媒体库展示的数据，并通过 Web Card 集成到 Emby 页面。

当前主要能力包括：

* Technical Specifications 卡片生成
* Technical Specifications 卡片展示
* 卡片资源服务
* Emby Web UI 集成
* Web Card 安装
* Web Card 更新
* Web Card 移除
* Web Card 状态检测
* 历史组件兼容检测
* 必要的备份与恢复

Web Card 的维护与媒体 NFO 是两个完全独立的操作域。

TCM 可以维护 Emby Web 集成文件，但**不会因此修改媒体 NFO**。

Manager 可在设置中即时切换简体中文、繁體中文与 English (United States)，不会重载页面或清空当前界面状态。法语、俄语、日语、西班牙语和泰语以 `v4.1.0` GitHub Release 的独立语言包提供，下载并验证后才能加载。Emby Web Card 使用独立的语言注册表；未安装或不支持的 Emby 语言回退简体中文。`Camera`、`Sound mix` 等 Technical Specs 字段键和数据结构不会随显示语言改变。

---

### 🎬 电影与电视剧独立管理

电影和电视剧在 TCM 中属于独立媒体空间。

包括：

* 独立媒体库根目录
* 独立索引
* 独立搜索
* 独立过滤
* 独立浏览
* 独立刷新范围
* 独立状态展示

例如：

```text
电影
 ↓
刷新当前媒体库
 ↓
仅扫描电影目录
```

不会因为刷新电影而自动扫描电视剧目录。

同样，刷新电视剧也不会默认重新扫描电影。

---

### 🔎 索引浏览与检查

TCM 的 Manager 界面不仅负责启动服务，也可以直接浏览已经建立的媒体索引。

目前可以用于查看：

* 已索引影片数量
* NFO 总数
* 缓存 / 索引状态
* XML 解析异常
* 媒体库可访问状态
* 电影与电视剧目录
* Technical Specifications
* 技术标签等只读信息
* NFO 路径
* 当前任务
* 错误状态

当发生错误时，TCM 会尽可能保留能够帮助定位问题的信息，例如影片、媒体类型、NFO 路径和相关任务。

---

### 🪟 Windows 常驻与系统托盘

当前正式实现面向：

**Windows x64**

启动 Tech Card Manager 后，Manager 和本地服务一起运行。

窗口最小化后，TCM 可以继续驻留在系统托盘。

目前支持：

* 单实例运行
* 最小化到系统托盘
* 从托盘恢复 Manager
* 登录后启动
* 登录启动时静默最小化
* 服务状态管理
* 明确的程序退出
* 退出时的资源清理

完全退出 TCM 后，本地服务也会停止。

---

## 🖼️ 界面预览

### Manager 主界面

Manager 主界面用于查看：

* 服务状态
* 索引统计
* 电影 / 电视剧空间
* 媒体目录
* Technical Specifications
* 当前任务状态
* 错误信息

<div align="center">

<img src="./docs/images/card-manager.PNG" alt="Tech Card Manager Manager UI" width="700">

</div>

---

### 设置与维护

设置区域用于管理：

* 电影媒体库根目录
* 电视剧媒体库根目录
* 启动行为
* 登录启动
* 静默启动
* 刷新周期
* Web Card 维护
* 更新检查
* 其他应用设置

<div align="center">

<img src="./docs/images/media-etting.PNG" alt="Tech Card Manager Settings" width="700">

</div>

---

## 🎞️ 成果展示

### Emby 电影详情页

Technical Specifications 可以作为独立卡片出现在 Emby 电影详情页中。

用于展示：

* Cameras
* Lenses
* Film / Digital Capture
* Cinematographic Process
* Laboratory
* Aspect Ratio
* Sound Mix
* Printed Film Format
* Master / Presentation Format
* 其他 Technical Specifications

<div align="center">

<img src="./docs/images/media-library-card.png" alt="Tech Card Manager Emby Media Library Card" width="700">

</div>

---

### Emby 电视剧详情页

电视剧使用独立的索引与展示逻辑。

节目级 Technical Specifications 可以进一步进入相应的 Emby 媒体页面。

> 📷 **成果截图预留位置**
>
> 建议文件：
>
> `./docs/images/emby-series-card.png`

<!--
<div align="center">

<img src="./docs/images/emby-series-card.png" alt="Tech Card Manager Emby Series Card" width="700">

</div>
-->

---

### Technical Specifications 卡片细节

这里可以展示 Technical Specifications Card 的完整视觉效果，例如：

* 摄影机
* 镜头
* 胶片规格
* 数字采集格式
* 摄影工艺
* 声音格式
* 画幅比例
* 母版格式
* 放映格式

> 📷 **成果截图预留位置**
>
> 建议文件：
>
> `./docs/images/technical-specs-card-detail.png`

<!--
<div align="center">

<img src="./docs/images/technical-specs-card-detail.png" alt="Tech Card Manager Technical Specifications Card" width="700">

</div>
-->

---

## 🔄 核心工作流

```text
Media NFO
    ↓
只读扫描与解析
    ↓
TCM 派生索引
    ↓
本地服务 / Card Assets
    ↓
Emby Web Card
    ↓
媒体库 Technical Specifications 展示
```

TCM 不把索引结果写回媒体 NFO。

可以把 TCM 理解成位于：

```text
NFO 数据层
    ↓
   TCM
    ↓
Emby 展示层
```

之间的一个只读适配器。

它负责读取已有技术规格、建立自己的展示索引，再把这些信息送到媒体库界面。

---

## 🔗 与 IMDb Tech Manager（ITM）的关系

TCM 与 [**IMDb Tech Manager（ITM）**](https://github.com/Eric-Hou1997/IMDb-Tech-Manager) 是两个独立工具。

两者围绕同一套 **Technical Specifications** 工作流协同工作。

### 📦 IMDb Tech Manager（ITM）

ITM 主要负责上游数据生产和维护，包括：

* IMDb Technical Specifications 获取
* Technical Specifications 结构化
* Technical Specifications 标准化
* NFO 管理
* Technical Specifications 写入
* 技术标签生成
* AI 辅助语义处理
* 手动修正
* 批量处理
* 元数据维护

---

### 🖥️ Tech Card Manager（TCM）

TCM 主要负责下游的读取、索引、展示与媒体库集成：

* 只读读取已有 NFO
* 建立 Technical Specifications 派生索引
* 管理 Emby Web Card
* 将技术规格展示到媒体库页面

两者组合后，可以形成完整工作流：

```text
IMDb
  ↓
IMDb Tech Manager（ITM）
  ↓
NFO / Technical Specifications
  ↓
Tech Card Manager（TCM）
  ↓
Emby Technical Specifications Card
```

TCM **不强制依赖 ITM**。

只要 NFO 中存在 TCM 可以识别的兼容 Technical Specifications 数据，也可以由其他数据源提供。

---

## 🚫 产品边界

Tech Card Manager 的职责边界非常明确。

TCM **不会**：

* 抓取 IMDb
* 修改媒体 NFO
* 写入 Technical Specifications
* 生成 Technical Tags
* 删除标签
* 修改用户标签
* 运行 AI
* 管理 Prompt
* 统计 AI Token
* 维护 AI API 调用费用
* 接管 NFO ownership
* 迁移 NFO ownership

这些属于其他数据管理工具，例如 IMDb Tech Manager 的职责。

TCM 专注于：

**安全地读取已有数据，并把它可靠地展示出来。**

---

## 🔒 数据与 Web 集成安全

### NFO 始终只读

TCM 在：

* 扫描
* 索引
* 刷新
* 搜索
* 展示

过程中，应保持媒体 NFO 内容不被修改。

无法解析的 NFO 会作为错误记录，而不会尝试自动修复文件。

---

### Web Card 使用可恢复维护流程

TCM 对 Emby Web 文件的安装、更新或移除，与 NFO 数据完全分开。

这类维护操作需要尽可能做到：

* 确认准确目标
* 建立备份
* 验证备份可恢复
* 构造完整修改结果
* 保持必要的 BOM / 换行规则
* 完成后验证结果
* 失败时回滚

需要管理员权限的操作通过 Windows UAC 明确执行。

---

### 历史组件谨慎迁移

TCM 保留对部分旧组件、旧 Web Patch 和历史安装痕迹的兼容识别能力。

涉及：

* 旧组件移除
* 进程终止
* Web Patch 替换
* 历史文件清理

等具有副作用的操作时，应：

```text
识别目标
    ↓
展示维护计划
    ↓
用户确认
    ↓
重新验证目标
    ↓
执行
    ↓
验证结果
```

如果无法可靠确认某个历史组件的 ownership，TCM 默认停止处理，而不是冒险删除。

---

## 🧩 当前架构

当前 Windows 实现主要由以下部分组成：

```text
Windows GUI / Native Integration
          +
        Go Core
          +
      Local Web UI
          +
   PowerShell Engine
          +
Tray / Browser Integration
          ↓
     Emby Web Card
```

Repository 按**产品**组织，而不是永久按照操作系统组织。

当前正式支持 Windows x64，未来计划增加其他操作系统实现。

---

## 💻 当前运行环境

目前正式维护的平台：

**Windows x64**

当前产品与发布流程重点围绕以下环境：

* Windows x64
* Windows PowerShell 5.1
* Windows UAC
* Windows 系统托盘
* 浏览器加载
* Emby Server Web UI

需要注意：

**源码能够编译，不代表真实平台行为已经得到验证。**

例如下面这些能力仍然需要实际 Windows / Emby 环境进行验收：

* UAC
* 托盘生命周期
* 登录启动
* 浏览器加载
* Emby DOM
* Web Card 安装
* Web Card 删除
* Web Card 恢复
* 应用退出后的资源清理

---

## 📦 安装与使用

### 1. 下载

前往：

[**GitHub Releases →**](https://github.com/Eric-Hou1997/Tech-Card-Manager/releases)

项目不单独提供裸 `.exe` Release。

当前正式安装包为：

```text
TCM-v4.1.0-Windows-x64-EXE.zip
```

---

### 2. 完整解压

请先把 ZIP 完整解压到一个固定目录。

然后运行其中的：

```text
Tech-Card-Manager.exe
```

不要直接在压缩包内部运行程序。

---

### 3. 配置媒体库

首次运行后，在 Manager 中配置对应的媒体库根目录。

电影与电视剧可以分别配置。

例如：

```text
Movies
  └── D:\Movies

TV
  └── D:\TV
```

TCM 会从这些目录读取对应的 NFO。
TCM 也支持从Emby Server中自动发现目录。

---

### 4. 建立索引

根据当前所在的电影或电视剧空间刷新媒体库。

```text
NFO
 ↓
解析
 ↓
派生索引
```

整个过程不会把数据写回 NFO。

---

### 5. 配置 Emby Web Card

根据 Manager 中的状态与提示完成 Emby Web Card 的安装或维护。

涉及 Emby Web 文件修改时，Windows 可能要求管理员权限。

---

### 6. 保持 TCM 运行

TCM 提供本地服务用于向 Web Card 提供索引和相关资源。

因此使用 Technical Specifications Card 时，需要保持 TCM 运行。

Manager 可以最小化到托盘，不需要一直显示在桌面。

---

## 🔄 更新方式

TCM 可以在设置页检查 GitHub 官方 Release。

当前 Portable 版本**不会自动替换正在运行的 EXE**。

推荐更新方式：

```text
检查新版本
    ↓
打开 GitHub Release
    ↓
下载新的 ZIP
    ↓
从托盘完全退出 TCM
    ↓
解压新版本
    ↓
替换程序文件
    ↓
保留数据目录 / 配置
    ↓
重新启动
    ↓
验证运行状态
```

Release 同时提供：

```text
TCM-v4.1.0-Windows-x64-EXE-SHA256SUMS.txt
```

用于验证下载包完整性。

---

## 🤖 面向 Coding Agent 的开发

Repository 已经提供：

[**`AGENTS.md` →**](./AGENTS.md)

它是 Codex 等 Coding Agent 理解 Tech Card Manager 的主要上下文入口之一。

其中包含：

* 产品身份
* Repository 边界
* TCM 与 ITM 的职责区分
* NFO 只读规则
* Technical Specifications 索引边界
* Web Card 安全规则
* 历史兼容标识规则
* Windows 生命周期规则
* UAC 与管理员维护约束
* 测试要求
* Release 边界
* 不应该进行的修改

推荐开发流程：

```text
Fork / Clone
        ↓
Coding Agent 读取 AGENTS.md
        ↓
读取相关源码与测试
        ↓
确认 TCM 产品边界
        ↓
分析受到影响的功能链路
        ↓
制定修改方案
        ↓
修改代码
        ↓
运行测试
        ↓
真实 Windows / Emby 验证
        ↓
提交 Pull Request
```

Repository 希望同时提供：

```text
源代码
  +
架构知识
  +
设计约束
  +
测试方法
  +
Agent Context
```

减少开发者和 Coding Agent 修改项目时破坏既有产品边界的风险。

---

## 🚧 当前状态

Tech Card Manager 目前处于：

**公开源码 · 持续开发阶段**

本 Repository 从：

**v4.0.0**

开始作为独立产品正式维护。

默认开发主线：

```text
main
```

当前已经提供：

* 完整公开源码
* Windows x64 Portable GUI
* v4.1.0 Release
* SHA-256 校验文件
* 只读 NFO 索引
* Emby Technical Specifications Web Card
* 电影 / 电视剧独立空间
* Windows 系统托盘
* 登录启动
* 静默启动
* 更新检查
* 基础测试体系
* Release 构建脚本
* `AGENTS.md`
* Apache License 2.0

---

## 🗺️ Roadmap

### 已完成

* [x] 建立独立 Tech Card Manager Repository
* [x] 从 `v4.0.0` 开始公开维护源码
* [x] Windows x64 Portable GUI
* [x] 电影 / 电视剧独立媒体空间
* [x] 只读 NFO 索引
* [x] Technical Specifications 派生索引
* [x] Emby Web Card 集成
* [x] 系统托盘
* [x] 单实例生命周期
* [x] 登录启动
* [x] 静默托盘启动
* [x] GitHub Release 更新检查
* [x] 建立基础回归测试
* [x] 建立 Release 构建流程
* [x] 发布首个公开版本 `v4.0.0`
* [x] 发布 `v4.1.0` 多语言注册表与版本绑定语言包
* [x] 完成简体中文、繁體中文和 English (United States) 内置界面
* [x] 发布法语、俄语、日语、西班牙语和泰语语言包
* [x] 固定新任务的日志语言并保留旧日志、索引和 NFO 原文
* [x] 区分代理/网络、GitHub 限流、资产缺失和下载失败
* [x] 完成内容测量驱动的标题区、控制台与 NFO 工具栏响应式布局

### 持续推进

* [ ] 完善 Emby Technical Specifications Card 展示效果
* [ ] 改进不同媒体类型的兼容处理
* [ ] 改进不同 Emby 页面结构的兼容性
* [ ] 加强不同 Emby Web UI / DOM 版本适配
* [ ] 完善索引错误定位
* [ ] 完善错误恢复体验
* [ ] 完善历史组件迁移流程
* [ ] 完善历史组件回滚能力
* [ ] 增加更多真实 Windows / Emby 回归测试
* [ ] 完善 Manager 状态可视化
* [ ] 完善设置体验
* [ ] 改进 Portable 更新体验
* [ ] 持续完善 `AGENTS.md`
* [ ] 持续完善 Coding Agent Context
* [ ] 探索其他操作系统支持，同时保持 TCM 的只读产品边界

Roadmap 会随着项目开发和实际使用反馈继续调整。

---

## 🐛 Issues

如果遇到可以稳定复现的问题，或者已经比较明确的功能需求，欢迎提交：

[**GitHub Issues →**](https://github.com/Eric-Hou1997/Tech-Card-Manager/issues)

建议尽可能提供：

* Tech Card Manager 版本
* Emby Server 版本
* Windows 版本
* 媒体类型
* 操作步骤
* Manager 中的错误信息
* NFO 路径（注意隐私信息）
* 是否涉及 UAC
* 是否涉及 Web Card
* 是否涉及旧组件迁移

这些信息可以帮助判断问题发生在：

```text
NFO
 ↓
Index
 ↓
Service
 ↓
Browser
 ↓
Emby DOM
 ↓
Card Render
```

哪一个环节。

---

## 🤝 Contributing

Tech Card Manager 已开放源码。

欢迎：

* Fork
* 阅读和研究源码
* Bug 修复
* 功能改进
* 测试完善
* UI / UX 改进
* Emby 兼容性改进
* 文档完善
* Pull Request

开始修改代码前，请优先阅读：

[**`AGENTS.md` →**](./AGENTS.md)

尤其是在修改以下部分时：

* NFO 扫描
* NFO 解析
* Technical Specifications 索引
* Emby Web Card
* Web 文件修改
* Web 文件恢复
* 历史组件兼容
* Windows UAC
* 系统托盘
* 应用生命周期
* 更新与 Release

需要保持当前产品边界。

**TCM 的媒体 NFO 只读原则属于核心设计约束。**

---

## 📄 License

Tech Card Manager 采用：

**Apache License 2.0**

完整许可证：

[**LICENSE →**](./LICENSE)

其他项目文档：

* [NOTICE](./NOTICE)
* [PRIVACY.md](./PRIVACY.md)
* [SECURITY.md](./SECURITY.md)
* [TERMS.md](./TERMS.md)

作者：**侯雁泽**

---

## ⚠️ 免责声明

Tech Card Manager 是一个独立开发的开源项目。

本项目**与 Emby、IMDb 以及其他第三方平台不存在官方隶属、授权或背书关系**。

相关第三方名称、商标、数据和服务归各自权利方所有。

TCM 不负责第三方 Technical Specifications 数据的来源与授权。

用户应自行确认其媒体元数据、第三方数据以及相关服务的使用方式符合适用的服务条款、授权条件和法律要求。

---

## 💡 反馈与建议

Tech Card Manager 会继续围绕：

* Technical Specifications 只读索引
* Emby Technical Specifications Card
* 媒体库展示
* Web UI 集成
* Windows 使用体验
* Emby 兼容性
* 稳定性
* Coding Agent 开发工作流

持续开发。

如果你对卡片布局、媒体类型兼容、Emby 页面集成、索引浏览、Windows 使用体验或开发流程有想法，欢迎通过 Issues 参与项目。
