<div align="center">

# Tech Card Manager

[简体中文](../../README.md) | [繁體中文](./README.zh-Hant.md) | [English](./README.en.md) | [Français](./README.fr.md) | [Русский](./README.ru.md) | **日本語** | [Español](./README.es.md) | [ไทย](./README.th.md)

<p align="center">

[![リリース](https://img.shields.io/github/v/release/Eric-Hou1997/Tech-Card-Manager?label=release)](https://github.com/Eric-Hou1997/Tech-Card-Manager/releases)
[![ダウンロード](https://img.shields.io/github/downloads/Eric-Hou1997/Tech-Card-Manager/total?label=downloads)](https://github.com/Eric-Hou1997/Tech-Card-Manager/releases)
[![スター](https://img.shields.io/github/stars/Eric-Hou1997/Tech-Card-Manager?style=flat&logo=github)](https://github.com/Eric-Hou1997/Tech-Card-Manager/stargazers)
[![PR 歓迎](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](https://github.com/Eric-Hou1997/Tech-Card-Manager/pulls)

</p>

<img src="../../windows/assets/TCM_logo.png" alt="Tech Card Manager" width="220">

**Technical Specifications カード管理および Emby メディアライブラリ統合ツール。**

**Emby Server** 向けの読み取り専用 NFO インデックスと Technical Specifications カード管理。

</div>

---

## 🎬 概要

**Tech Card Manager (TCM)** は、メディアの NFO ファイルに既に存在する **Technical Specifications** を、実際の Emby 閲覧体験へ取り込みます。

多くのメディアライブラリは、次の情報をすでに適切に表示しています。

* タイトル
* 出演者
* 年
* 解像度
* 映像コーデック
* 音声コーデック
* HDR / Dolby Vision などのストリーム情報

しかし、次のような制作情報は、通常完全かつ構造化された形では表示されません。

* 使用したカメラ
* 使用したレンズ
* 使用したフィルムまたはデジタル撮影形式
* 関係する撮影プロセス
* 使用した音声形式
* 使用したアスペクト比
* 作品のマスタリングおよび提示方法

TCM はユーザーが設定した映画・テレビ番組の NFO ファイルを **読み取り専用** で走査し、独自の派生インデックスを構築して、Emby Web Card によりメディア詳細ページへ技術仕様を表示します。

TCM の主な目標：

* 既存の Technical Specifications をメディアライブラリの閲覧体験へ取り込む
* 元の NFO ファイルを厳密に読み取り専用とする
* ユーザーの既存メタデータワークフローの所有権を取得しない
* 映画とテレビ番組を別々の領域として管理・更新する
* Emby Technical Specifications Web Card を安全に保守する
* 長時間稼働に適した Windows GUI、システムトレイ統合、ローカルサービスを提供する
* 重要な状態、エラー、保守操作を観察・検証可能にする
* 簡体字中国語、繁体字中国語、英語（米国）を内蔵し、フランス語、ロシア語、日本語、スペイン語、タイ語をバージョン連動パックで提供する

---

## ✨ 主な機能

### 📚 読み取り専用 NFO インデックス

TCM はユーザーが設定した映画・テレビ番組ライブラリを読み、NFO から Technical Specifications と表示・識別に必要なその他の読み取り専用情報を抽出します。

TCM にとってメディア NFO は **読み取り専用データソース** です。

TCM は次の操作を行いません。

* NFO ファイルの変更
* NFO ファイルの自動再編成
* NFO ファイルの自動「修復」
* Technical Specifications の書き込み
* タグの生成または削除
* タグ所有権の変更
* NFO の元の内容の変更

インデックスデータは TCM が別途保持します。

元のメディアメタデータは、ユーザーと既存ワークフローで利用しているメタデータツールの管理下に残ります。

---

### 🖥️ Emby Technical Specifications Web Card

TCM はインデックス化した技術仕様をメディアライブラリ表示用データへ変換し、Web Card を通じて Emby へ統合します。

現在の機能：

* Technical Specifications カードの生成
* Technical Specifications カードの表示
* カードアセットの配信
* Emby Web UI 統合
* Web Card のインストール
* Web Card の更新
* Web Card の削除
* Web Card 状態の検出
* 旧コンポーネントの互換性検出
* 必須のバックアップおよび復旧ワークフロー

Web Card の保守とメディア NFO データは、完全に別の操作領域です。

TCM は Emby Web 統合ファイルを保守できますが、**その処理の一部としてメディア NFO を変更することはありません**。

Manager は現在の UI 状態を再読み込み・消去せず、簡体字中国語、繁体字中国語、英語（米国）を即座に切り替えられます。フランス語、ロシア語、日本語、スペイン語、タイ語は `v4.1.0` GitHub Release の個別アセットで、ダウンロードと検証後にのみ読み込まれます。Emby Web Card には独立したロケールレジストリがあり、未インストールまたは非対応の Emby ロケールは簡体字中国語へフォールバックします。`Camera` や `Sound mix` などの Technical Specs キーと基礎データ構造は、表示言語によって変わりません。

---

### 🎬 映画とテレビ番組の分離管理

映画とテレビ番組は TCM 内で別のメディア領域です。

それぞれに次のものがあります。

* 独立したライブラリルート
* 独立したインデックス
* 独立した検索
* 独立したフィルター
* 独立した閲覧
* 独立した更新範囲
* 独立した状態表示

例：

```text
映画
  ↓
現在のライブラリを更新
  ↓
映画ディレクトリのみを走査
```

映画を更新してもテレビ番組ディレクトリは自動走査されません。

同様に、テレビ番組を更新しても映画は自動再走査されません。

---

### 🔎 インデックスの閲覧と検査

TCM Manager はサービス開始だけでなく、構築済みのメディアインデックスの閲覧にも使用できます。

現在表示できる項目：

* インデックス済みタイトル数
* NFO 総数
* キャッシュ / インデックス状態
* XML 解析エラー
* ライブラリアクセス状態
* 映画・テレビ番組ディレクトリ
* Technical Specifications
* 技術タグなどの読み取り専用情報
* NFO パス
* 現在のタスク
* エラー状態

エラー発生時、TCM は対象タイトル、メディア種別、NFO パス、タスクなど、実用的な診断情報を可能な限り保持します。

---

### 🪟 Windows 常駐とシステムトレイ

現在の公式実装の対象：

**Windows x64**

Tech Card Manager を起動すると、Manager とローカルサービスが一緒に実行されます。

ウィンドウを最小化した後も、TCM はシステムトレイで動作を継続できます。

現在の対応内容：

* 単一インスタンス実行
* システムトレイへの最小化
* トレイから Manager を復元
* ログイン後に起動
* ログイン起動時に静かにトレイへ最小化
* サービス状態管理
* 明示的なアプリ終了
* 終了時のリソース解放

TCM を完全に終了すると、ローカルサービスも停止します。

---

## 🖼️ インターフェースプレビュー

### Manager UI

Manager UI では次を確認できます。

* サービス状態
* インデックス統計
* 映画 / テレビ番組領域
* メディアディレクトリ
* Technical Specifications
* 現在のタスク状態
* エラー情報

<div align="center">

<img src="../images/card-manager.PNG" alt="Tech Card Manager Manager UI" width="700">

</div>

---

### 設定と保守

設定領域では次を管理します。

* 映画ライブラリルート
* テレビ番組ライブラリルート
* 起動動作
* ログイン後に起動
* サイレント起動
* 更新間隔
* Web Card 保守
* 更新確認
* その他のアプリ設定

<div align="center">

<img src="../images/media-etting.PNG" alt="Tech Card Manager 設定" width="700">

</div>

---

## 🎞️ 表示結果

### Emby 映画詳細ページ

Technical Specifications は Emby の映画詳細ページに専用カードとして表示できます。

カードに表示できる情報：

* カメラ
* レンズ
* フィルム / デジタル撮影
* 撮影プロセス
* ラボ
* アスペクト比
* サウンドミックス
* プリントフィルム形式
* マスター / 提示形式
* その他の Technical Specifications

<div align="center">

<img src="../images/media-library-card.png" alt="Tech Card Manager Emby メディアライブラリカード" width="700">

</div>

---

### Emby テレビ番組詳細ページ

テレビ番組は独立したインデックス・表示フローを使用します。

番組単位の Technical Specifications を対応する Emby メディアページへ表示できます。

> 📷 **結果スクリーンショットのプレースホルダー**
>
> 推奨ファイル：
>
> `../images/emby-series-card.png`

<!--
<div align="center">

<img src="../images/emby-series-card.png" alt="Tech Card Manager Emby シリーズカード" width="700">

</div>
-->

---

### Technical Specifications カード詳細

この節では次の項目を含む Technical Specifications Card の完全な表示結果を掲載できます。

* カメラ
* レンズ
* フィルム形式
* デジタル撮影形式
* 撮影プロセス
* 音声形式
* アスペクト比
* マスター形式
* 提示形式

> 📷 **結果スクリーンショットのプレースホルダー**
>
> 推奨ファイル：
>
> `../images/technical-specs-card-detail.png`

<!--
<div align="center">

<img src="../images/technical-specs-card-detail.png" alt="Tech Card Manager Technical Specifications カード" width="700">

</div>
-->

---

## 🔄 基本ワークフロー

```text
メディア NFO
    ↓
読み取り専用の走査と解析
    ↓
TCM 派生インデックス
    ↓
ローカルサービス / カードアセット
    ↓
Emby Web Card
    ↓
メディアライブラリの Technical Specifications
```

TCM はインデックス結果をメディア NFO へ書き戻しません。

TCM は次の間に位置する読み取り専用アダプターと考えられます。

```text
NFO データ層
    ↓
   TCM
    ↓
Emby 表示層
```

既存の技術仕様を読み、独自の表示インデックスを構築し、その情報をメディアライブラリ画面へ提供します。

---

## 🔗 IMDb Tech Manager (ITM) との関係

TCM と [**IMDb Tech Manager (ITM)**](https://github.com/Eric-Hou1997/IMDb-Tech-Manager) は独立した 2 つのツールです。

同じ **Technical Specifications** ワークフロー上で連携します。

### 📦 IMDb Tech Manager (ITM)

ITM は主に上流のデータ生成と保守を担当します。

* IMDb Technical Specifications の取得
* Technical Specifications の構造化
* Technical Specifications の正規化
* NFO 管理
* Technical Specifications の書き込み
* 技術タグ生成
* AI 支援による意味処理
* 手動修正
* バッチ処理
* メタデータ保守

---

### 🖥️ Tech Card Manager (TCM)

TCM は主に下流の読み取り、インデックス、表示、メディアライブラリ統合を担当します。

* 既存 NFO を読み取り専用で読む
* Technical Specifications の派生インデックスを構築
* Emby Web Card を管理
* メディアライブラリページへ技術仕様を表示

両者を組み合わせると、完全なワークフローになります。

```text
IMDb
  ↓
IMDb Tech Manager (ITM)
  ↓
NFO / Technical Specifications
  ↓
Tech Card Manager (TCM)
  ↓
Emby Technical Specifications Card
```

TCM は **ITM を必須としません**。

NFO に TCM が認識できる互換 Technical Specifications データが含まれていれば、他のデータソースも利用できます。

---

## 🚫 製品境界

Tech Card Manager は意図的に厳格な責務境界を持ちます。

TCM は次の操作を **行いません**。

* IMDb のスクレイピング
* メディア NFO の変更
* Technical Specifications の書き込み
* Technical Tags の生成
* タグの削除
* ユーザータグの変更
* AI の実行
* プロンプト管理
* AI トークン使用量の追跡
* AI API コストの追跡
* NFO メタデータ所有権の取得
* NFO 所有権の移行

これらは IMDb Tech Manager などのデータ管理ツールが担当します。

TCM が集中するのは 1 つだけです。

**既存データを安全に読み、確実に表示すること。**

---

## 🔒 データと Web 統合の安全性

### NFO ファイルは読み取り専用のまま

次の処理中：

* 走査
* インデックス
* 更新
* 検索
* 表示

TCM はメディア NFO の内容を変更しません。

解析できない NFO は自動修復せず、エラーとして記録します。

---

### 復旧可能な Web Card 保守

Emby Web ファイルのインストール、更新、削除は、メディア NFO データから完全に分離されています。

保守操作は次を行うよう設計されています。

* 正確な対象を確認
* バックアップを作成
* バックアップから復旧可能か検証
* 完全な変更結果を構築
* 必要な BOM / 改行動作を保持
* 完了後の結果を検証
* 失敗時にロールバック

管理者権限が必要な操作は Windows UAC を通じて明示的に行われます。

---

### 旧コンポーネントの慎重な移行

TCM は一部の旧コンポーネント、古い Web Patch、過去のインストール痕跡に対する互換性検出を維持します。

次のような副作用を伴う操作では：

* 旧コンポーネントの削除
* プロセスの終了
* Web Patch の置換
* 過去ファイルの消去

次のフローを使用します。

```text
対象を識別
    ↓
保守計画を表示
    ↓
ユーザー確認
    ↓
対象を再検証
    ↓
実行
    ↓
結果を検証
```

TCM が旧コンポーネントの所有権を確実に判定できない場合、危険な削除を行わず停止します。

---

## 🧩 現在のアーキテクチャ

現在の Windows 実装は主に次で構成されます。

```text
Windows GUI / ネイティブ統合
          +
        Go Core
          +
      ローカル Web UI
          +
   PowerShell Engine
          +
トレイ / ブラウザー統合
          ↓
     Emby Web Card
```

リポジトリはオペレーティングシステムではなく **製品** を中心に構成されています。

現在は Windows x64 をサポートし、将来は他の OS 実装も計画しています。

---

## 💻 現在の実行環境

現在保守されているプラットフォーム：

**Windows x64**

現在の製品と Release ワークフローは次の環境を対象とします。

* Windows x64
* Windows PowerShell 5.1
* Windows UAC
* Windows システムトレイ
* ブラウザー読み込み
* Emby Server Web UI

重要：

**ソースのコンパイル成功は、実環境での動作を証明しません。**

次の機能は実際の Windows / Emby 環境で検証する必要があります。

* UAC
* トレイのライフサイクル
* ログイン起動
* ブラウザー読み込み
* Emby DOM 動作
* Web Card インストール
* Web Card 削除
* Web Card 復旧
* アプリ終了後のリソース解放

---

## 📦 インストールと使用方法

### 1. ダウンロード

次のページを開きます。

[**GitHub Releases →**](https://github.com/Eric-Hou1997/Tech-Card-Manager/releases)

本プロジェクトは単体の裸の `.exe` を Release アセットとして公開しません。

現在の公式パッケージ：

```text
TCM-v4.1.0-Windows-x64-EXE.zip
```

---

### 2. ZIP を完全に展開

まず ZIP 全体を固定ディレクトリへ展開します。

次に実行します。

```text
Tech-Card-Manager.exe
```

圧縮アーカイブ内から直接実行しないでください。

---

### 3. メディアライブラリを設定

初回起動後、Manager で対象ライブラリルートを設定します。

映画とテレビ番組のルートは別々に設定できます。

例：

```text
映画
  └── D:\Movies

テレビ番組
  └── D:\TV
```

TCM はこれらのディレクトリから対応する NFO を読み取ります。
Emby Server からライブラリディレクトリを自動検出することもできます。

---

### 4. インデックスを構築

現在の映画またはテレビ番組領域に対応するライブラリを更新します。

```text
NFO
 ↓
解析
 ↓
派生インデックス
```

この処理はインデックスデータを NFO へ書き戻しません。

---

### 5. Emby Web Card を設定

Manager の状態と案内に従い、Emby Web Card をインストールまたは保守します。

Emby Web ファイルの変更時、Windows が管理者権限を求める場合があります。

---

### 6. TCM を実行したままにする

TCM は Web Card がインデックスと関連アセットへアクセスするためのローカルサービスを提供します。

そのため Technical Specifications Card の使用中は TCM を実行しておく必要があります。

Manager はトレイへ最小化でき、デスクトップ上に表示しておく必要はありません。

---

## 🔄 更新

TCM は設定ページから公式 GitHub Releases を確認できます。

現在の Portable ビルドは **実行中の EXE を自動置換しません**。

推奨更新フロー：

```text
新バージョンを確認
    ↓
GitHub Release を開く
    ↓
新しい ZIP をダウンロード
    ↓
トレイから TCM を完全終了
    ↓
新バージョンを展開
    ↓
プログラムファイルを置換
    ↓
既存のデータディレクトリ / 設定を保持
    ↓
再起動
    ↓
実行状態を確認
```

Release はパッケージ整合性検証用に次も提供します。

```text
TCM-v4.1.0-Windows-x64-EXE-SHA256SUMS.txt
```

---

## 🤖 Coding Agents を利用した開発

リポジトリには次の文書があります。

[**`AGENTS.md` →**](../../AGENTS.md)

Tech Card Manager に取り組む Codex などの Coding Agents にとって、主要なコンテキスト入口の 1 つです。

記載内容：

* 製品識別
* リポジトリ境界
* TCM と ITM の責務分離
* 読み取り専用 NFO 規則
* Technical Specifications インデックス境界
* Web Card 安全規則
* 旧コンポーネント互換識別子規則
* Windows ライフサイクル規則
* UAC と管理者保守の制約
* テスト要件
* Release 境界
* 行ってはならない変更

推奨開発ワークフロー：

```text
Fork / Clone
        ↓
Coding Agent が AGENTS.md を読む
        ↓
関連するソースコードとテストを読む
        ↓
TCM の製品境界を確認
        ↓
影響を受ける機能経路を分析
        ↓
実装計画を作成
        ↓
コードを変更
        ↓
テストを実行
        ↓
必要に応じて実際の Windows / Emby で検証
        ↓
Pull Request を提出
```

リポジトリは次の提供を目指します。

```text
ソースコード
  +
アーキテクチャ知識
  +
設計制約
  +
テスト方法
  +
Agent Context
```

これにより、開発者や Coding Agents が変更時に既存の製品境界を誤って壊す危険を減らします。

---

## 🚧 現在の状態

Tech Card Manager は現在：

**オープンソース · 活発に開発中**

このリポジトリは次のバージョンから独立製品として保守されています。

**v4.0.0**

既定の開発ブランチ：

```text
main
```

現在利用可能：

* 完全な公開ソースコード
* Windows x64 Portable GUI
* v4.1.0 Release
* SHA-256 チェックサムファイル
* 読み取り専用 NFO インデックス
* Emby Technical Specifications Web Card
* 独立した映画 / テレビ番組領域
* Windows システムトレイ統合
* ログイン起動
* サイレント起動
* 更新確認
* 基本テストスイート
* Release ビルドスクリプト
* `AGENTS.md`
* Apache License 2.0

---

## 🗺️ ロードマップ

### 完了

* [x] 独立した Tech Card Manager リポジトリを作成
* [x] `v4.0.0` からソースコードを公開保守
* [x] Windows x64 Portable GUI
* [x] 映画 / テレビ番組の独立したメディア領域
* [x] 読み取り専用 NFO インデックス
* [x] 派生 Technical Specifications インデックス
* [x] Emby Web Card 統合
* [x] システムトレイ統合
* [x] 単一インスタンスのライフサイクル
* [x] ログイン起動
* [x] サイレントなトレイ起動
* [x] GitHub Release 更新確認
* [x] 基本回帰テストを構築
* [x] Release ビルドワークフローを構築
* [x] 最初の公開リリース `v4.0.0` を公開
* [x] `v4.1.0` ローカライズレジストリとバージョン連動言語パックをリリース
* [x] 簡体字中国語、繁体字中国語、英語（米国）の内蔵 UI を完成
* [x] フランス語、ロシア語、日本語、スペイン語、タイ語パックを公開
* [x] 過去ログ、インデックス、NFO バイトを保持しながら新規タスクのログ言語を固定
* [x] プロキシ/ネットワーク、GitHub 制限、アセット欠落、ダウンロード失敗を区別
* [x] ヘッダー、ダッシュボード、NFO ツールバーの内容基準レスポンシブレイアウトを完成

### 進行中

* [ ] Emby Technical Specifications Card 表示を改善
* [ ] メディア種別間の互換性を改善
* [ ] 異なる Emby ページ構造への互換性を改善
* [ ] Emby Web UI / DOM バージョン間の互換性を改善
* [ ] インデックスエラーのローカライズを改善
* [ ] エラー復旧を改善
* [ ] 旧コンポーネント移行を改善
* [ ] 旧コンポーネントのロールバックを改善
* [ ] 実際の Windows / Emby 回帰テストを追加
* [ ] Manager 状態表示を改善
* [ ] 設定 UX を改善
* [ ] Portable 更新体験を改善
* [ ] `AGENTS.md` を継続的に改善
* [ ] Coding Agent Context を継続的に改善
* [ ] TCM の読み取り専用製品境界を維持しつつ他の OS 対応を検討

ロードマップはプロジェクトの開発と実際のフィードバックに応じて更新されます。

---

## 🐛 Issues

再現可能な問題や明確な機能要望がある場合は Issue を開いてください。

[**GitHub Issues →**](https://github.com/Eric-Hou1997/Tech-Card-Manager/issues)

可能であれば次を含めてください。

* Tech Card Manager のバージョン
* Emby Server のバージョン
* Windows のバージョン
* メディア種別
* 再現手順
* Manager に表示されたエラー情報
* 必要に応じて個人情報を除いた NFO パス
* UAC が関係するか
* Web Card が関係するか
* 旧コンポーネント移行が関係するか

この情報は問題が次のどこで起きたかの判断に役立ちます。

```text
NFO
 ↓
インデックス
 ↓
サービス
 ↓
ブラウザー
 ↓
Emby DOM
 ↓
カード描画
```

---

## 🤝 コントリビューション

Tech Card Manager はオープンソースです。

次のようなコントリビューションを歓迎します。

* Fork
* ソースコードのレビューと調査
* バグ修正
* 機能改善
* テスト改善
* UI / UX 改善
* Emby 互換性改善
* ドキュメント改善
* Pull Request

コードを変更する前にお読みください。

[**`AGENTS.md` →**](../../AGENTS.md)

特に次を変更する際は既存の製品境界を維持してください。

* NFO 走査
* NFO 解析
* Technical Specifications インデックス
* Emby Web Card
* Web ファイル変更
* Web ファイル復旧
* 旧コンポーネント互換性
* Windows UAC
* システムトレイ統合
* アプリのライフサイクル
* 更新と Release

**TCM のメディア NFO 読み取り専用規則は中核設計制約です。**

---

## 📄 ライセンス

Tech Card Manager のライセンス：

**Apache License 2.0**

ライセンス全文：

[**LICENSE →**](../../LICENSE)

その他のプロジェクト文書：

* [NOTICE](../../NOTICE)
* [PRIVACY.md](../legal/PRIVACY.ja.md)
* [SECURITY.md](../../SECURITY.md)
* [TERMS.md](../legal/TERMS.ja.md)

作者：**侯雁泽**

---

## ⚠️ 免責事項

Tech Card Manager は独立して開発されたオープンソースプロジェクトです。

本プロジェクトは **Emby、IMDb、その他の第三者プラットフォームと公式に提携、承認、推奨されていません**。

第三者の名称、商標、データ、サービスはそれぞれの所有者に帰属します。

TCM は第三者の Technical Specifications データの出所やライセンス状態に責任を負いません。

ユーザーは自身のメディアメタデータ、第三者データ、関連サービスの利用が、該当する利用規約、ライセンス要件、法律に従うことを確認する責任があります。

---

## 💡 フィードバックと提案

Tech Card Manager は次の領域を中心に開発を続けます。

* 読み取り専用 Technical Specifications インデックス
* Emby Technical Specifications Card
* メディアライブラリ表示
* Web UI 統合
* Windows ユーザー体験
* Emby 互換性
* 安定性
* Coding Agents を利用した開発ワークフロー

カードレイアウト、メディア種別の互換性、Emby ページ統合、インデックス閲覧、Windows 利用、開発ワークフローについてアイデアがあれば、Issues からご参加ください。
