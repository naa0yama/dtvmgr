# dtvmgr

![coverage](https://raw.githubusercontent.com/naa0yama/dtvmgr/badges/coverage.svg)
![test execution time](https://raw.githubusercontent.com/naa0yama/dtvmgr/badges/time.svg)

日本のテレビ放送 TS ファイルの CM 検出・エンコード管理 CLI ツール

## 概要

`dtvmgr` は、日本のテレビ放送を録画した TS ファイルに対して CM 検出パイプライン (`join_logo_scp`) の実行、ffmpeg エンコード、設定管理を行う Rust 製 CLI ツールです。Node.js 版 `join_logo_scp_trial` を Rust で再実装しています。

## クレート構成

```
crates/
├── dtvmgr-cli/      # CLI エントリーポイント・設定管理・TUI
├── dtvmgr-jlse/     # CM 検出パイプライン (チャンネル検出、パラメータ、エンコード、バリデーション)
├── dtvmgr-tsduck/   # TSDuck ラッパー (PAT/EIT パース、TS シーク)
├── dtvmgr-api/      # 外部 API クライアント (しょぼいカレンダー、TMDB)
└── dtvmgr-db/       # SQLite キャッシュ DB
```

## 必要要件

- Docker
- Visual Studio Code + Dev Containers 拡張機能

## セットアップ

1. リポジトリをクローン:

```bash
git clone https://github.com/naa0yama/dtvmgr.git
cd dtvmgr
```

2. VS Code でプロジェクトを開き、コマンドパレットから「Dev Containers: Reopen in Container」を選択

## CLI コマンド

### 設定初期化

```bash
dtvmgr init                          # デフォルト設定ファイルを生成
```

### CM 検出パイプライン

```bash
# チャンネル検出
dtvmgr jlse channel --input /path/to/recording.ts

# パラメータ検出
dtvmgr jlse param --input /path/to/recording.ts

# パイプライン実行
dtvmgr jlse run --input /path/to/recording.ts [--encode] [--filter]

# エンコード時オプション
dtvmgr jlse run --input recording.ts --encode \
  --add-chapter \
  --target cutcm_logo \
  --outdir /output/ \
  --skip-duration-check       # エンコード前の尺チェックをスキップ

# TUI モード
dtvmgr jlse run --input recording.ts --encode --tui

# EPGStation モード (環境変数 INPUT/OUTPUT から自動取得)
dtvmgr jlse run --epgstation [--tui]
```

### TSDuck 解析

```bash
dtvmgr jlse tsduck --input /path/to/recording.ts
```

## 設定ファイル

`dtvmgr init` で生成される TOML 設定ファイルには以下のセクションがあります:

| セクション                       | 内容                                  |
| -------------------------------- | ------------------------------------- |
| `[syoboi]`                       | しょぼいカレンダー連携 (チャンネル等) |
| `[tmdb]`                         | TMDB API 連携                         |
| `[normalize]`                    | タイトル正規化ルール                  |
| `[jlse.dirs]`                    | JL パイプラインのディレクトリ設定     |
| `[jlse.bins]`                    | 外部バイナリパス                      |
| `[jlse.encode]`                  | エンコード設定 (format, video, audio) |
| `[[jlse.encode.duration_check]]` | エンコード前尺チェックルール          |

### エンコード前尺チェック

エンコード前に、元の TS と CM カット後の AVS の尺比率を検証します。比率がしきい値を下回る場合、カットエラーの可能性があるためエンコードを中断します。

デフォルトルール:

| 番組尺   | 最低比率 |
| -------- | -------- |
| 10分以下 | 68%      |
| 11-49分  | 75%      |
| 50-90分  | 70%      |
| 91分以上 | 70%      |

設定ファイルでカスタムルールを定義できます:

```toml
[[jlse.encode.duration_check]]
min_min = 0
max_min = 10
min_percent = 68
```

## 開発

すべてのタスクは `mise run <task>` で実行します。

### 基本操作

```bash
mise run build            # デバッグビルド
mise run build:release    # リリースビルド
mise run test             # テスト実行
mise run test:watch       # TDD ウォッチモード
mise run test:doc         # ドキュメントテスト
```

### コード品質

```bash
mise run fmt              # フォーマット (cargo fmt + dprint)
mise run fmt:check        # フォーマットチェック
mise run clippy           # Lint
mise run clippy:strict    # Lint (warnings をエラー扱い)
mise run ast-grep         # ast-grep カスタムルールチェック
mise run coverage         # カバレッジ計測
mise run deny             # ライセンス・依存関係チェック
mise run miri             # 未定義動作検出
```

### コミット前チェック

```bash
mise run pre-commit       # fmt:check + clippy:strict + ast-grep + lint:gh
```

### OTel 対応

OTel はデフォルトで有効です。`OTEL_EXPORTER_OTLP_ENDPOINT` を設定すると OTLP エクスポートが有効になります。

```bash
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317 dtvmgr jlse run ...

# OTel なしでビルドする場合
cargo build -p dtvmgr-cli --no-default-features
```

## ライセンス

[AGPL-3.0-only](./LICENSE)

## Troubleshooting

### デバッグログ

```bash
RUST_LOG=trace RUST_BACKTRACE=1 cargo run -- help
```
