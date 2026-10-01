<div align="center">

# Tech Card Manager

[簡體中文](../../README.md) | **繁體中文** | [English](./README.en.md) | [Français](./README.fr.md) | [Русский](./README.ru.md) | [日本語](./README.ja.md) | [Español](./README.es.md) | [ไทย](./README.th.md)

<p align="center">

[![Release](https://img.shields.io/github/v/release/Eric-Hou1997/Tech-Card-Manager?label=release)](https://github.com/Eric-Hou1997/Tech-Card-Manager/releases)
[![Downloads](https://img.shields.io/github/downloads/Eric-Hou1997/Tech-Card-Manager/total?label=downloads)](https://github.com/Eric-Hou1997/Tech-Card-Manager/releases)
[![Stars](https://img.shields.io/github/stars/Eric-Hou1997/Tech-Card-Manager?style=flat&logo=github)](https://github.com/Eric-Hou1997/Tech-Card-Manager/stargazers)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](https://github.com/Eric-Hou1997/Tech-Card-Manager/pulls)

</p>

<img src="../../windows/assets/TCM_logo.png" alt="Tech Card Manager" width="220">

**Technical specifications card management and Emby media library integration tool.**

面向 **Emby Server** 的只讀 NFO 索引與 Technical Specifications 卡片管理工具。

</div>

---

v5.0.0 僅遷移技術棧與系統適配，目標是保留 v4.1.0 的功能、介面和操作。TCM 仍唯讀 NFO、管理 Emby 卡片並手動更新，沒有 AI。真實 Emby、完整原生介面/操作和 Windows/Linux 執行驗收尚未完成；原始碼檢查與製包不代表這些驗收通過。現有截圖作為原介面參照。

## 🎬 項目簡介

**Tech Card Manager（TCM）** 用於把已經存在於媒體 NFO 中的 **Technical Specifications（影視技術規格）** 帶到 Emby 的實際瀏覽界面中。

現在的大多數媒體庫已經可以很好地展示：

* 片名
* 演員
* 年份
* 分辨率
* 影片編碼
* 音頻編碼
* HDR / Dolby Vision 等媒體流資訊

但一部影視作品在製作階段使用了：

* 什麼攝影機
* 什麼鏡頭
* 什麼膠片或數字採集格式
* 怎樣的攝影工藝
* 什麼聲音格式
* 什麼畫幅比例
* 怎樣的母版與放映格式

這些資訊通常並沒有得到完整、結構化的展示。

TCM 會以**只讀方式**掃描使用者配置的電影和電視劇 NFO，建立自己的派生索引，並通過 Emby Web Card 將這些技術規格展示在媒體詳情頁中。

TCM 的核心目標包括：

* 讓已有 Technical Specifications 真正進入媒體庫瀏覽體驗
* 保持媒體 NFO 原檔案只讀
* 不接管使用者現有的元資料工作流
* 將電影與電視劇作為獨立空間管理和刷新
* 安全地維護 Emby Technical Specifications Web Card
* 提供長期運行所需的 Windows GUI、系統托盤和本地服務能力
* 讓重要狀態、錯誤和維護操作盡可能可觀察、可驗證
* 內置簡體中文、繁體中文和 English (United States)，並通過版本綁定的語言包支持法語、俄語、日語、西班牙語和泰語

---

## ✨ 核心能力

### 📚 只讀 NFO 索引

TCM 可以讀取使用者配置的電影與電視劇媒體庫目錄，從 NFO 中提取並索引 Technical Specifications 以及其他用於展示和識別的只讀資訊。

媒體 NFO 是 TCM 的**只讀資料源**。

TCM 不會：

* 修改 NFO
* 自動整理 NFO
* 自動“修復” NFO
* 寫入 Technical Specifications
* 生成或刪除標籤
* 修改標籤 ownership
* 改變 NFO 原有內容

索引結果由 TCM 自己維護。

媒體原始資料仍然由使用者以及原本使用的元資料管理工具掌控。

---

### 🖥️ Emby Technical Specifications Web Card

TCM 會將索引後的技術規格轉換為適合媒體庫展示的資料，並通過 Web Card 集成到 Emby 頁面。

當前主要能力包括：

* Technical Specifications 卡片生成
* Technical Specifications 卡片展示
* 卡片資源服務
* Emby Web UI 集成
* Web Card 安裝
* Web Card 更新
* Web Card 移除
* Web Card 狀態檢測
* 歷史組件兼容檢測
* 必要的備份與恢復

Web Card 的維護與媒體 NFO 是兩個完全獨立的操作域。

TCM 可以維護 Emby Web 集成檔案，但**不會因此修改媒體 NFO**。

Manager 可在設置中即時切換簡體中文、繁體中文與 English (United States)，不會重載頁面或清空當前界面狀態。法語、俄語、日語、西班牙語和泰語以 `v5.0.0` GitHub Release 的獨立語言包提供，下載並驗證後才能加載。Emby Web Card 使用獨立的語言註冊表；未安裝或不支持的 Emby 語言回退簡體中文。`Camera`、`Sound mix` 等 Technical Specs 字段鍵和資料結構不會隨顯示語言改變。

---

### 🎬 電影與電視劇獨立管理

電影和電視劇在 TCM 中屬於獨立媒體空間。

包括：

* 獨立媒體庫根目錄
* 獨立索引
* 獨立搜索
* 獨立過濾
* 獨立瀏覽
* 獨立刷新範圍
* 獨立狀態展示

例如：

```text
電影
 ↓
刷新當前媒體庫
 ↓
僅掃描電影目錄
```

不會因為刷新電影而自動掃描電視劇目錄。

同樣，刷新電視劇也不會預設重新掃描電影。

---

### 🔎 索引瀏覽與檢查

TCM 的 Manager 界面不僅負責啓動服務，也可以直接瀏覽已經建立的媒體索引。

目前可以用於查看：

* 已索引影片數量
* NFO 總數
* 緩存 / 索引狀態
* XML 解析異常
* 媒體庫可訪問狀態
* 電影與電視劇目錄
* Technical Specifications
* 技術標籤等只讀資訊
* NFO 路徑
* 當前任務
* 錯誤狀態

當發生錯誤時，TCM 會盡可能保留能夠幫助定位問題的資訊，例如影片、媒體類型、NFO 路徑和相關任務。

---

### 🪟 常駐與系統托盤

當前正式實現面向：

**macOS ARM64 / Windows x64, ARM64 / Linux x64, ARM64**

啓動 Tech Card Manager 後，Manager 和本地服務一起運行。

窗口最小化後，TCM 可以繼續駐留在系統托盤。

目前支持：

* 單實例運行
* 最小化到系統托盤
* 從托盤恢復 Manager
* 登錄後啓動
* 登錄啓動時靜默最小化
* 服務狀態管理
* 明確的程序退出
* 退出時的資源清理

完全退出 TCM 後，本地服務也會停止。

---

## 🖼️ 界面預覽

### Manager 主界面

Manager 主界面用於查看：

* 服務狀態
* 索引統計
* 電影 / 電視劇空間
* 媒體目錄
* Technical Specifications
* 當前任務狀態
* 錯誤資訊

<div align="center">

<img src="../images/card-manager.PNG" alt="Tech Card Manager Manager UI" width="700">

</div>

---

### 設置與維護

設置區域用於管理：

* 電影媒體庫根目錄
* 電視劇媒體庫根目錄
* 啓動行為
* 登錄啓動
* 靜默啓動
* 刷新週期
* Web Card 維護
* 更新檢查
* 其他應用設置

<div align="center">

<img src="../images/media-etting.PNG" alt="Tech Card Manager Settings" width="700">

</div>

---

## 🎞️ 成果展示

### Emby 電影詳情頁

Technical Specifications 可以作為獨立卡片出現在 Emby 電影詳情頁中。

用於展示：

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

<img src="../images/media-library-card.png" alt="Tech Card Manager Emby Media Library Card" width="700">

</div>

---

### Emby 電視劇詳情頁

電視劇使用獨立的索引與展示邏輯。

節目級 Technical Specifications 可以進一步進入相應的 Emby 媒體頁面。

> 📷 **成果截圖預留位置**
>
> 建議檔案：
>
> `../images/emby-series-card.png`

<!--
<div align="center">

<img src="../images/emby-series-card.png" alt="Tech Card Manager Emby Series Card" width="700">

</div>
-->

---

### Technical Specifications 卡片細節

這裡可以展示 Technical Specifications Card 的完整視覺效果，例如：

* 攝影機
* 鏡頭
* 膠片規格
* 數字採集格式
* 攝影工藝
* 聲音格式
* 畫幅比例
* 母版格式
* 放映格式

> 📷 **成果截圖預留位置**
>
> 建議檔案：
>
> `../images/technical-specs-card-detail.png`

<!--
<div align="center">

<img src="../images/technical-specs-card-detail.png" alt="Tech Card Manager Technical Specifications Card" width="700">

</div>
-->

---

## 🔄 核心工作流

```text
Media NFO
    ↓
只讀掃描與解析
    ↓
TCM 派生索引
    ↓
本地服務 / Card Assets
    ↓
Emby Web Card
    ↓
媒體庫 Technical Specifications 展示
```

TCM 不把索引結果寫回媒體 NFO。

可以把 TCM 理解成位於：

```text
NFO 資料層
    ↓
   TCM
    ↓
Emby 展示層
```

之間的一個只讀適配器。

它負責讀取已有技術規格、建立自己的展示索引，再把這些資訊送到媒體庫界面。

---

## 🔗 與 IMDb Tech Manager（ITM）的關係

TCM 與 [**IMDb Tech Manager（ITM）**](https://github.com/Eric-Hou1997/IMDb-Tech-Manager) 是兩個獨立工具。

兩者圍繞同一套 **Technical Specifications** 工作流協同工作。

### 📦 IMDb Tech Manager（ITM）

ITM 主要負責上游資料生產和維護，包括：

* IMDb Technical Specifications 獲取
* Technical Specifications 結構化
* Technical Specifications 標準化
* NFO 管理
* Technical Specifications 寫入
* 技術標籤生成
* AI 輔助語義處理
* 手動修正
* 批量處理
* 元資料維護

---

### 🖥️ Tech Card Manager（TCM）

TCM 主要負責下游的讀取、索引、展示與媒體庫集成：

* 只讀讀取已有 NFO
* 建立 Technical Specifications 派生索引
* 管理 Emby Web Card
* 將技術規格展示到媒體庫頁面

兩者組合後，可以形成完整工作流：

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

TCM **不強制依賴 ITM**。

只要 NFO 中存在 TCM 可以識別的兼容 Technical Specifications 資料，也可以由其他資料源提供。

---

## 🚫 產品邊界

Tech Card Manager 的職責邊界非常明確。

TCM **不會**：

* 抓取 IMDb
* 修改媒體 NFO
* 寫入 Technical Specifications
* 生成 Technical Tags
* 刪除標籤
* 修改使用者標籤
* 運行 AI
* 管理 Prompt
* 統計 AI Token
* 維護 AI API 調用費用
* 接管 NFO ownership
* 遷移 NFO ownership

這些屬於其他資料管理工具，例如 IMDb Tech Manager 的職責。

TCM 專注於：

**安全地讀取已有資料，並把它可靠地展示出來。**

---

## 🔒 資料與 Web 集成安全

### NFO 始終只讀

TCM 在：

* 掃描
* 索引
* 刷新
* 搜索
* 展示

過程中，應保持媒體 NFO 內容不被修改。

無法解析的 NFO 會作為錯誤記錄，而不會嘗試自動修復檔案。

---

### Web Card 使用可恢復維護流程

TCM 對 Emby Web 檔案的安裝、更新或移除，與 NFO 資料完全分開。

這類維護操作需要盡可能做到：

* 確認準確目標
* 建立備份
* 驗證備份可恢復
* 構造完整修改結果
* 保持必要的 BOM / 換行規則
* 完成後驗證結果
* 失敗時回滾

需要管理員權限的操作通過 Windows UAC 明確執行。

---

### 歷史組件謹慎遷移

TCM 保留對部分舊組件、舊 Web Patch 和歷史安裝痕跡的兼容識別能力。

涉及：

* 舊組件移除
* 進程終止
* Web Patch 替換
* 歷史檔案清理

等具有副作用的操作時，應：

```text
識別目標
    ↓
展示維護計劃
    ↓
使用者確認
    ↓
重新驗證目標
    ↓
執行
    ↓
驗證結果
```

如果無法可靠確認某個歷史組件的 ownership，TCM 預設停止處理，而不是冒險刪除。

---

## 🧩 當前架構

v5 使用 Rust + Tauri 2 與 TypeScript + Vue 3/Vite。原 v4 Windows 原始碼和發行輸入保留在 `windows/` 與 `packaging/`；v5 原始碼位於 `rewrite/`。儲存庫仍按產品組織。

```text
Tauri 2 / Native Integration
          +
       Rust Core
          +
TypeScript / Vue 3 / Vite
          ↓
     Emby Web Card
```

---

## 💻 當前運行環境

v5 製包涵蓋 macOS ARM64、Windows x64/ARM64、Linux x64/ARM64，共五個目標、九個套件，不提供 macOS Intel。macOS 最低版本為 12.0；Windows 使用 WebView2，NSIS 包含離線安裝程式；Linux 使用 WebKitGTK 4.1、GTK 3、系統匣函式庫及需要提升權限時的 pkexec。沒有新增 PowerShell 執行依賴。完整真實平台與 Emby 驗收仍待執行，包括權限、系統匣、登入啟動、用戶端載入、卡片安裝/移除/復原和退出清理。macOS 套件僅臨時簽章，未做 Apple 公證；Windows 套件未做發行憑證簽章。

---

## 📦 安裝與使用

### 1. 下載

從 [GitHub Releases](https://github.com/Eric-Hou1997/Tech-Card-Manager/releases) 下載與系統、應用程式架構相符的安裝套件。v5.0.0 套件檔名如下；Setup.exe 是安裝程式，AppImage 是可執行檔案。

| 系統 | 架構 | 安裝套件 |
| --- | --- | --- |
| macOS | ARM64 | `TCM-v5.0.0-MacOS-AArch64.dmg` |
| Windows | x64 | `TCM-v5.0.0-Windows-x64-Setup.exe` |
| Windows | ARM64 | `TCM-v5.0.0-Windows-AArch64-Setup.exe` |
| Linux | x64 | `TCM-v5.0.0-Linux-x64.AppImage` |
| Linux | x64 | `TCM-v5.0.0-Linux-x64.deb` |
| Linux | x64 | `TCM-v5.0.0-Linux-x64.rpm` |
| Linux | ARM64 | `TCM-v5.0.0-Linux-AArch64.AppImage` |
| Linux | ARM64 | `TCM-v5.0.0-Linux-AArch64.deb` |
| Linux | ARM64 | `TCM-v5.0.0-Linux-AArch64.rpm` |

---

### 2. 安裝應用程式

macOS：開啟 DMG，將 Tech Card Manager.app 放入 Applications。Windows：執行對應架構的 NSIS Setup.exe。Linux：使用發行版套件管理器安裝 DEB/RPM，或賦予 AppImage 執行權限後執行。升級前從選單列或系統匣完全退出應用程式。應用程式資料儲存於系統使用者資料目錄；保留舊 Portable 的 `data`、`logs`、`backup`、`runtime`、`updates` 及其他原檔案，不刪除原目錄。舊資料遷移按應用程式內清單確認，不改寫 NFO。

從 v4.1.0 升級時，舊程式只匹配舊 Portable ZIP，不能直接識別 v5 的新安裝渠道；請直接前往官方 Releases 下載相符的 v5 套件。不要把 NSIS 安裝程式當成舊 Portable 的主程式替換，保留原目錄並按新版內的遷移清單確認資料。

---

### 3. 配置媒體庫

首次運行後，在 Manager 中配置對應的媒體庫根目錄。

電影與電視劇可以分別配置。

例如：

```text
Movies
  └── D:\Movies

TV
  └── D:\TV
```

TCM 會從這些目錄讀取對應的 NFO。
TCM 也支持從Emby Server中自動發現目錄。

---

### 4. 建立索引

根據當前所在的電影或電視劇空間刷新媒體庫。

```text
NFO
 ↓
解析
 ↓
派生索引
```

整個過程不會把資料寫回 NFO。

---

### 5. 配置 Emby Web Card

根據 Manager 中的狀態與提示完成 Emby Web Card 的安裝或維護。

修改 Emby Web 檔案時，Windows 使用 UAC，macOS 使用系統管理員授權，Linux 使用 pkexec。先核對維護清單，再確認操作；拒絕授權不會改寫媒體 NFO。

---

### 6. 保持 TCM 運行

TCM 提供本地服務用於向 Web Card 提供索引和相關資源。

因此使用 Technical Specifications Card 時，需要保持 TCM 運行。

Manager 可以最小化到托盤，不需要一直顯示在桌面。

---

## 🔄 更新方式

設定頁保留原檢查更新、內嵌下載確認與開啟系統瀏覽器的手動流程。它按目前安裝渠道選擇 DMG、NSIS、AppImage、DEB 或 RPM，不自動安裝或替換應用程式。下載相符套件 → 完全退出 → 安裝或替換 → 保留使用者資料與備份 → 重新啟動 → 核對執行狀態。DEB/RPM 使用系統套件管理器升級。發行附件提供 `TCM-v5.0.0-SHA256SUMS.txt`；先核對下載檔案摘要。

---

## 🤖 面向 Coding Agent 的開發

Repository 已經提供：

[**`AGENTS.md` →**](../../AGENTS.md)

它是 Codex 等 Coding Agent 理解 Tech Card Manager 的主要上下文入口之一。

其中包含：

* 產品身份
* Repository 邊界
* TCM 與 ITM 的職責區分
* NFO 只讀規則
* Technical Specifications 索引邊界
* Web Card 安全規則
* 歷史兼容標識規則
* Windows 生命週期規則
* UAC 與管理員維護約束
* 測試要求
* Release 邊界
* 不應該進行的修改

推薦開發流程：

```text
Fork / Clone
        ↓
Coding Agent 讀取 AGENTS.md
        ↓
讀取相關源碼與測試
        ↓
確認 TCM 產品邊界
        ↓
分析受到影響的功能鏈路
        ↓
制定修改方案
        ↓
修改代碼
        ↓
運行測試
        ↓
真實 Windows / Emby 驗證
        ↓
提交 Pull Request
```

Repository 希望同時提供：

```text
源代碼
  +
架構知識
  +
設計約束
  +
測試方法
  +
Agent Context
```

減少開發者和 Coding Agent 修改項目時破壞既有產品邊界的風險。

---

## 🚧 當前狀態

Tech Card Manager 目前處於：

**公開源碼 · 持續開發階段**

本 Repository 從：

**v4.0.0**

開始作為獨立產品正式維護。

預設開發主線：

```text
main
```

當前已經提供：

* 完整公開源碼
* Tauri 2 GUI (macOS ARM64 / Windows x64, ARM64 / Linux x64, ARM64)
* v5.0.0 Release
* SHA-256 校驗檔案
* 只讀 NFO 索引
* Emby Technical Specifications Web Card
* 電影 / 電視劇獨立空間
* Windows 系統托盤
* 登錄啓動
* 靜默啓動
* 更新檢查
* 基礎測試體系
* Release 構建腳本
* `AGENTS.md`
* Apache License 2.0

---

## 🗺️ Roadmap

### 已完成

* [x] 建立獨立 Tech Card Manager Repository
* [x] 從 `v4.0.0` 開始公開維護源碼
* [x] Windows x64 Portable GUI
* [x] 電影 / 電視劇獨立媒體空間
* [x] 只讀 NFO 索引
* [x] Technical Specifications 派生索引
* [x] Emby Web Card 集成
* [x] 系統托盤
* [x] 單實例生命週期
* [x] 登錄啓動
* [x] 靜默托盤啓動
* [x] GitHub Release 更新檢查
* [x] 建立基礎回歸測試
* [x] 建立 Release 構建流程
* [x] 發佈首個公開版本 `v4.0.0`
* [x] 發佈 `v4.1.0` 多語言註冊表與版本綁定語言包
* [x] 完成簡體中文、繁體中文和 English (United States) 內置界面
* [x] 發佈法語、俄語、日語、西班牙語和泰語語言包
* [x] 固定新任務的日誌語言並保留舊日誌、索引和 NFO 原文
* [x] 區分代理/網路、GitHub 限流、資產缺失和下載失敗
* [x] 完成內容測量驅動的標題區、控制台與 NFO 工具欄響應式佈局

### 持續推進

目前 v5 範圍僅為原功能遷移與平台適配。

* [ ] 完成真實 Emby 卡片安裝、顯示、撤除與失敗復原驗收
* [ ] 完成原生介面、彈窗、操作與像素對照
* [ ] 完成五目標真實系統執行與安裝/升級/解除安裝驗收

上述未驗收項不會因建置成功而勾選；不以 v5 為由新增功能。

---

## 🐛 Issues

如果遇到可以穩定復現的問題，或者已經比較明確的功能需求，歡迎提交：

[**GitHub Issues →**](https://github.com/Eric-Hou1997/Tech-Card-Manager/issues)

建議盡可能提供：

* Tech Card Manager 版本
* Emby Server 版本
* Windows 版本
* 媒體類型
* 操作步驟
* Manager 中的錯誤資訊
* NFO 路徑（注意隱私資訊）
* 是否涉及 UAC
* 是否涉及 Web Card
* 是否涉及舊組件遷移

這些資訊可以幫助判斷問題發生在：

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

哪一個環節。

---

## 🤝 Contributing

Tech Card Manager 已開放源碼。

歡迎：

* Fork
* 閱讀和研究源碼
* Bug 修復
* 功能改進
* 測試完善
* UI / UX 改進
* Emby 兼容性改進
* 文檔完善
* Pull Request

開始修改代碼前，請優先閱讀：

[**`AGENTS.md` →**](../../AGENTS.md)

尤其是在修改以下部分時：

* NFO 掃描
* NFO 解析
* Technical Specifications 索引
* Emby Web Card
* Web 檔案修改
* Web 檔案恢復
* 歷史組件兼容
* Windows UAC
* 系統托盤
* 應用生命週期
* 更新與 Release

需要保持當前產品邊界。

**TCM 的媒體 NFO 只讀原則屬於核心設計約束。**

---

## 📄 License

Tech Card Manager 採用：

**Apache License 2.0**

完整許可證：

[**LICENSE →**](../../LICENSE)

其他項目文檔：

* [NOTICE](../../NOTICE)
* [PRIVACY.md](../legal/PRIVACY.zh-Hant.md)
* [SECURITY.md](../../SECURITY.md)
* [TERMS.md](../legal/TERMS.zh-Hant.md)

作者：**侯雁澤**

---

## ⚠️ 免責聲明

Tech Card Manager 是一個獨立開發的開源項目。

本項目**與 Emby、IMDb 以及其他第三方平台不存在官方隸屬、授權或背書關係**。

相關第三方名稱、商標、資料和服務歸各自權利方所有。

TCM 不負責第三方 Technical Specifications 資料的來源與授權。

使用者應自行確認其媒體元資料、第三方資料以及相關服務的使用方式符合適用的服務條款、授權條件和法律要求。

---

## 💡 反饋與建議

Tech Card Manager 會繼續圍繞：

* Technical Specifications 只讀索引
* Emby Technical Specifications Card
* 媒體庫展示
* Web UI 集成
* Windows 使用體驗
* Emby 兼容性
* 穩定性
* Coding Agent 開發工作流

持續開發。

如果你對卡片佈局、媒體類型兼容、Emby 頁面集成、索引瀏覽、Windows 使用體驗或開發流程有想法，歡迎通過 Issues 參與項目。
