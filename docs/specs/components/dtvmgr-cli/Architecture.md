# dtvmgr-cli Architecture

## 概要

`dtvmgr` の CLI エントリポイント。`clap` でサブコマンドを定義し、各クレートの機能を統合する。TOML 設定ファイルの読み書きと `OTel` トレーシングの初期化も担当する。

## ステータス

- **実装状態**: 完了
- **Rust クレート**: `crates/dtvmgr-cli`

## サブコマンド構成

| コマンド                        | 概要                                               |
| ------------------------------- | -------------------------------------------------- |
| `init`                          | デフォルトテンプレートで設定ファイルを生成         |
| `syoboi prog`                   | しょぼいカレンダー API から番組表を取得            |
| `syoboi titles`                 | しょぼいカレンダー API からタイトル一覧を取得      |
| `syoboi channels select`        | TUI でチャンネルを対話選択                         |
| `syoboi channels list`          | 選択済みチャンネルを一覧表示                       |
| `tmdb search-tv / search-movie` | TMDB で TV / 映画を検索                            |
| `tmdb tv-details / tv-season`   | TMDB の TV 詳細 / シーズン情報を取得               |
| `db sync`                       | しょぼいデータをローカル DB に同期                 |
| `db list`                       | キャッシュ済みタイトル / 番組を TUI で閲覧         |
| `db normalize`                  | タイトル正規化結果を TUI でプレビュー              |
| `db tmdb-lookup`                | キャッシュ済みタイトルの TMDB 検索・マッピング保存 |
| `jlse run`                      | CM 検出パイプライン実行 (FFmpeg エンコード対応)    |
| `jlse channel`                  | ファイル名から放送チャンネルを検出                 |
| `jlse param`                    | チャンネル・ファイル名から JL パラメータを検出     |
| `jlse tsduck`                   | TSDuck で EIT 番組情報を抽出・表示                 |
| `epgstation encode`             | EPGStation 録画を TUI で選択しエンコードキュー投入 |
| `completion`                    | シェル補完スクリプトを生成                         |

## 設定管理

- `AppConfig` 構造体が TOML 設定ファイル全体を表現する
- セクション: `syoboi`, `tmdb`, `epgstation`, `normalize`, `jlse`
- `init` サブコマンドで `to_commented_toml()` によりコメント付きテンプレートを生成
- デフォルトパス: `~/.config/dtvmgr/config.toml`

## OTel 統合

- `otel` feature フラグで有効化 (デフォルト有効)
- `OTEL_EXPORTER_OTLP_ENDPOINT` 設定時に OTLP エクスポートを起動
- `tracing-opentelemetry` + `opentelemetry-otlp` でトレースとメトリクスを送信
- CLI メトリクス: DB 同期レコード数、TMDB ルックアップ結果、ストレージ使用量

## 依存関係

### 内部クレート

| クレート        | 用途                                |
| --------------- | ----------------------------------- |
| `dtvmgr-api`    | しょぼいカレンダー / EPGStation API |
| `dtvmgr-db`     | SQLite キャッシュ DB                |
| `dtvmgr-jlse`   | CM 検出パイプライン                 |
| `dtvmgr-tmdb`   | TMDB API クライアント               |
| `dtvmgr-tsduck` | TSDuck 連携                         |
| `dtvmgr-tui`    | TUI コンポーネント                  |

### 主要外部クレート

| クレート  | 用途                       |
| --------- | -------------------------- |
| `clap`    | コマンドライン引数パース   |
| `toml`    | 設定ファイル読み書き       |
| `tracing` | 構造化ログ / OTel トレース |
| `anyhow`  | エラーハンドリング         |
