<div align="center">

# Tech Card Manager

[簡體中文](../../README.md) | **繁體中文** | [English](./README.en.md) | [Français](./README.fr.md) | [Русский](./README.ru.md) | [日本語](./README.ja.md) | [Español](./README.es.md) | [ไทย](./README.th.md)

[![Release](https://img.shields.io/github/v/release/Eric-Hou1997/Tech-Card-Manager?label=release)](https://github.com/Eric-Hou1997/Tech-Card-Manager/releases)
[![Downloads](https://img.shields.io/github/downloads/Eric-Hou1997/Tech-Card-Manager/total?label=downloads)](https://github.com/Eric-Hou1997/Tech-Card-Manager/releases)

<img src="../../windows/assets/TCM_logo.png" alt="Tech Card Manager" width="220">

Emby Server 的唯讀 NFO 索引與 Technical Specifications 卡片管理工具。

</div>

## 專案簡介

Tech Card Manager（TCM）以唯讀方式掃描使用者設定的電影與電視劇 NFO，建立自己的派生索引，並透過 Emby Web Card 在媒體詳情頁顯示攝影機、鏡頭、拍攝格式、聲音格式、畫面比例及製作流程等 Technical Specifications。

TCM 不取得 IMDb 資料，也不寫入 NFO 或產生標籤。需要建立或維護這些資料時，請使用獨立的 [IMDb Tech Manager（ITM）](https://github.com/Eric-Hou1997/IMDb-Tech-Manager)。

## 目前版本與平台

- 正式版本：[`v4.1.0`](https://github.com/Eric-Hou1997/Tech-Card-Manager/releases/tag/v4.1.0)
- 平台：Windows x64 Portable
- 安裝包：`TCM-v4.1.0-Windows-x64-EXE.zip`
- ZIP 內的程式名稱固定為 `Tech-Card-Manager.exe`
- Release 同時提供 SHA-256、說明、更新記錄與語言包

## 核心能力

- 電影與電視劇獨立媒體目錄、索引、搜尋、篩選與重新整理範圍
- 唯讀 NFO Inspector、XML 錯誤與完整路徑診斷
- Emby Technical Specifications Web Card 的安裝、更新、移除與狀態檢查
- Windows 單實例、系統匣、登入啟動與靜默啟動
- 依內容寬度切換的標題區、控制台、NFO 工具列與雙欄版面
- 可區分代理/網路、GitHub 限流、資產缺失與下載失敗的更新檢查

## 語言

簡體中文、繁體中文與 English (United States) 內建於 Portable 程式。Français、Русский、日本語、Español 與ไทย以 `v4.1.0` Release 的獨立語言包提供，下載並驗證後載入。

新任務在啟動時固定日誌語言；舊日誌、NFO、索引快取、備份與使用者資料不會被改寫。未安裝或不支援的 Emby 語言回退簡體中文。`Camera`、`Sound mix` 等 Technical Specs 鍵與 JSON 結構永遠保持不變，只翻譯顯示文字。

## 安全邊界

- 媒體 NFO 對 TCM 永遠唯讀；位元組與時間戳不因索引而改變。
- Web Card 維護只作用於經驗證的 Emby Web 檔案，並使用外部備份、鎖定、CAS、日誌與原子替換。
- 歷史元件遷移或管理員操作必須顯示計畫並由使用者明確確認。
- 路徑、權屬或備份無法驗證時，維護流程會安全停止。

## 介面預覽

![TCM Manager](../images/card-manager.PNG)

![Emby Technical Specifications Card](../images/media-library-card.png)

## 安裝與更新

完整解壓 ZIP 後執行 `Tech-Card-Manager.exe`，在設定中分別選擇電影與電視劇目錄，建立索引並按畫面提示設定 Web Card。使用卡片時需保持 TCM 運行，可以最小化到系統匣。

Portable 更新不會自行取代正在執行的 EXE。請從系統匣完整退出，解壓新版並替換程式；保留既有 `data`、`logs`、`backup`、`runtime` 與 `updates` 目錄。

## Roadmap

已完成：Windows x64 Portable、唯讀 NFO 索引、電影/電視劇獨立空間、Web Card 安全維護、系統匣生命週期、多語言註冊表與五種下載語言包、代理友善更新，以及內容測量驅動的響應式版面。

持續推進：更多真實 Windows／Emby／PowerShell 5.1／UAC 回歸、不同 Emby DOM 與媒體類型相容、歷史元件復原、Portable 更新體驗，以及其他平台支援。

## 開發與授權

開發前請閱讀 [`AGENTS.md`](../../AGENTS.md)。語言包設計見 [`docs/language-packs.md`](../language-packs.md)。

本專案採用 [Apache License 2.0](../../LICENSE)。IMDb、Emby 及其他商標歸各自權利人所有；本專案與 IMDb.com, Inc. 或 Emby LLC 無從屬、授權或背書關係。
