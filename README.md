# dtvmgr

![coverage](https://raw.githubusercontent.com/naa0yama/dtvmgr/badges/coverage.svg)
![test execution time](https://raw.githubusercontent.com/naa0yama/dtvmgr/badges/time.svg)

日本のテレビ放送 TS ファイルの CM 検出・エンコード管理 CLI ツール

## 概要

`dtvmgr` は、日本のテレビ放送を録画した TS ファイルに対して CM 検出パイプライン (`join_logo_scp`) の実行、ffmpeg エンコード、設定管理を行う Rust 製 CLI ツールです。Node.js 版 `join_logo_scp_trial` を Rust で再実装しています。

## クレート構成

```
crates/
├── dtvmgr-cli/      # CLI エントリーポイント・設定管理
├── dtvmgr-jlse/     # CM 検出パイプライン (チャンネル検出、パラメータ、エンコード、バリデーション)
├── dtvmgr-vmaf/     # VMAF ベース品質パラメータ探索 (補間二分探索)
├── dtvmgr-tsduck/   # TSDuck ラッパー (PAT/EIT パース、TS シーク)
├── dtvmgr-tui/      # TUI コンポーネント (パイプライン進捗表示、データブラウザ)
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
dtvmgr jlse run --input /path/to/recording.ts [--encode]

# エンコード時オプション
dtvmgr jlse run --input recording.ts --encode \
  --target cutcm-logo \
  --outdir /output/ \
  --skip-duration-check \
  --force

# チャプター追加を無効化 (デフォルトは追加)
dtvmgr jlse run --input recording.ts --encode --no-chapter

# TUI モード
dtvmgr jlse run --input recording.ts --encode --tui

# EPGStation モード (環境変数 INPUT/OUTPUT から自動取得)
dtvmgr jlse run --epgstation [--tui]
```

### TSDuck 解析

```bash
dtvmgr jlse tsduck --input /path/to/recording.ts
```

### しょぼいカレンダー

```bash
dtvmgr syoboi prog [--time-since ...] [--time-until ...]  # 番組スケジュール取得
dtvmgr syoboi titles [--tid ...]                           # タイトルデータ取得
dtvmgr syoboi channels select                              # チャンネル選択 (TUI)
dtvmgr syoboi channels list                                # 選択済みチャンネル一覧
```

### TMDB

```bash
dtvmgr tmdb search-tv --query "SPY×FAMILY"       # TV シリーズ検索
dtvmgr tmdb search-movie --query "..."            # 映画検索
dtvmgr tmdb tv-details --id 12345                 # TV シリーズ詳細
dtvmgr tmdb tv-season --id 12345 --season 1       # TV シーズン詳細
```

### ローカル DB

```bash
dtvmgr db sync [--time-since ...] [--time-until ...]  # しょぼいデータをローカル DB に同期
dtvmgr db list                                         # キャッシュ済みタイトル・番組一覧 (TUI)
dtvmgr db normalize                                    # タイトル正規化プレビュー (TUI)
dtvmgr db tmdb-lookup [--force]                        # TMDB 検索・結果保存
```

### EPGStation

```bash
dtvmgr epgstation encode [--keyword ...] [--limit 100]  # エンコードキュー
```

### シェル補完

```bash
dtvmgr completion bash    # bash 補完スクリプト生成
dtvmgr completion zsh     # zsh 補完スクリプト生成
dtvmgr completion fish    # fish 補完スクリプト生成
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
| `[jlse.encode.quality_search]`   | VMAF 品質探索設定                     |

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

### VMAF 品質探索

エンコード前に VMAF (Video Multi-Method Assessment Fusion) スコアを基準とした最適な品質パラメータ(CRF 等)を補間二分探索で自動決定します。TS からサンプルを抽出し、1080p にアップスケールした VMAF 測定を行い、目標スコアを満たす最適値を探索します。

対応エンコーダプリセット: `av1_qsv`, `libsvtav1`, `h264_qsv`, `hevc_qsv`, `libx264`, `libx265`

設定例:

```toml
[jlse.encode.quality_search]
enabled = true
target_vmaf = 95.0
max_encoded_percent = 80
min_vmaf_tolerance = 0.5
sample_duration_secs = 20
skip_secs = 30
sample_every_secs = 720
min_samples = 3
max_samples = 10
vmaf_subsample = 1
thorough = false
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

`RUST_LOG` 未設定時のデフォルトは `warn,dtvmgr=info` です(3rd-party crate は warn、dtvmgr crate は info)。

`dtvmgr=<level>` を指定すると、全 workspace crate に一括でログレベルを設定できます
(EnvFilter のプレフィックスマッチにより `dtvmgr_api`, `dtvmgr_cli`, `dtvmgr_db` 等すべてにマッチします)。

```bash
# dtvmgr 全体を trace (3rd-party は warn に抑制)
RUST_LOG=warn,dtvmgr=trace cargo run -- help

# 特定の crate だけレベルを変更
RUST_LOG=warn,dtvmgr=trace,dtvmgr_api=info cargo run -- help

# 3rd-party crate も含めて確認したい場合
RUST_LOG=trace RUST_BACKTRACE=1 cargo run -- help
```
