<div align="center">

# Tech Card Manager

[简体中文](./README.md) | [繁體中文](./README.zh-Hant.md) | [English](./README.en.md) | [Français](./README.fr.md) | [Русский](./README.ru.md) | **日本語** | [Español](./README.es.md) | [ไทย](./README.th.md)

[![Release](https://img.shields.io/github/v/release/Eric-Hou1997/Tech-Card-Manager?label=release)](https://github.com/Eric-Hou1997/Tech-Card-Manager/releases)
[![Downloads](https://img.shields.io/github/downloads/Eric-Hou1997/Tech-Card-Manager/total?label=downloads)](https://github.com/Eric-Hou1997/Tech-Card-Manager/releases)

<img src="./windows/assets/TCM_logo.png" alt="Tech Card Manager" width="220">

Emby Server 向けの読み取り専用 NFO インデックス／Technical Specifications カード管理ツールです。

</div>

## 概要

Tech Card Manager（TCM）は、ユーザーが指定した映画とテレビ番組の NFO を読み取り専用でスキャンし、独自の派生インデックスを作成します。Emby Web Card を通して、カメラ、レンズ、撮影形式、音声形式、アスペクト比、制作工程などの Technical Specifications を詳細ページに表示します。

TCM は IMDb データを取得せず、NFO への書き込みやタグ生成も行いません。これらのデータを作成・管理する場合は、独立した [IMDb Tech Manager（ITM）](https://github.com/Eric-Hou1997/IMDb-Tech-Manager) を使用してください。

## 現在のバージョンと対応環境

- 安定版：[`v4.1.0`](https://github.com/Eric-Hou1997/Tech-Card-Manager/releases/tag/v4.1.0)
- 対応環境：Windows x64 Portable
- 配布ファイル：`TCM-v4.1.0-Windows-x64-EXE.zip`
- ZIP 内の実行ファイル名：`Tech-Card-Manager.exe`
- SHA-256、説明書、変更履歴、言語パックを同じ Release で提供します

## 主な機能

- 映画とテレビ番組ごとのフォルダ、インデックス、検索、フィルター、更新範囲
- 読み取り専用 NFO Inspector と、完全なパスを含む XML エラー診断
- Emby Web Card のインストール、更新、削除、状態確認
- Windows の単一インスタンス、システムトレイ、ログイン起動、サイレント起動
- タイトル、ダッシュボード、NFO ツール、2 ペインの内容幅に応じたレイアウト
- プロキシ／ネットワーク、GitHub 制限、アセット欠落、ダウンロード失敗を区別する更新確認

## 言語

簡体字中国語、繁体字中国語、英語（米国）は Portable EXE に内蔵されています。フランス語、ロシア語、日本語、スペイン語、タイ語は `v4.1.0` Release の個別言語パックとして提供され、ダウンロードと検証の後に読み込まれます。

新しいタスクのログ言語は開始時に固定されます。過去のログ、NFO、インデックス、バックアップ、ユーザーデータは書き換えられません。未導入または非対応の Emby 言語は簡体字中国語へフォールバックします。`Camera`、`Sound mix` などのキーと JSON 構造は固定され、表示文字列だけが翻訳されます。

## 安全境界

- TCM にとってメディア NFO は常に読み取り専用で、インデックス作成によってバイト列やタイムスタンプは変わりません。
- Web Card の保守は検証済み Emby Web ファイルだけを対象とし、外部バックアップ、ロック、CAS、ジャーナル、アトミック置換を使用します。
- 旧コンポーネントの移行や管理者操作は計画を表示し、明示的な確認を要求します。
- パス、所有権、バックアップを検証できない場合は安全に停止します。

## 画面

![TCM Manager](./docs/images/card-manager.PNG)

![Emby Technical Specifications Card](./docs/images/media-library-card.png)

## インストールと更新

ZIP を完全に展開して `Tech-Card-Manager.exe` を起動し、映画とテレビ番組のフォルダを別々に設定します。インデックスを作成し、画面の案内に従って Web Card を設定してください。カード利用中は TCM を動作させ、ウィンドウはトレイへ最小化できます。

Portable 版は実行中の EXE を自動置換しません。トレイから完全終了し、新版を展開してプログラムを置換してください。`data`、`logs`、`backup`、`runtime`、`updates` は保持します。

## ロードマップ

完了：Windows x64 Portable、読み取り専用 NFO、映画／TV 独立空間、安全な Web Card、トレイライフサイクル、多言語レジストリと 5 パック、プロキシ対応更新、内容計測型レスポンシブレイアウト。

継続：実環境の Windows／Emby／PowerShell 5.1／UAC テスト、Emby DOM とメディア種別の互換性、旧コンポーネントの復旧、Portable 更新、他プラットフォーム対応。

## 開発とライセンス

開発前に [`AGENTS.md`](./AGENTS.md) を確認してください。言語パックの設計は [`docs/language-packs.md`](./docs/language-packs.md) にあります。

本プロジェクトは [Apache License 2.0](./LICENSE) で公開されています。IMDb、Emby などの商標は各所有者に帰属します。本プロジェクトは IMDb.com, Inc. または Emby LLC と提携、承認、推奨の関係にはありません。
