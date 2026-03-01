# jlse CM 検出パイプライン設計

> 親ドキュメント: [IMPROVEMENT_PLAN.md](../IMPROVEMENT_PLAN.md)

## 背景と目的

### 概要

jlse (join_logo_scp trial) は日本のテレビ放送録画ファイル (TS/M2TS) から CM (コマーシャル) を検出・除去するパイプラインツールである。既存の Node.js 実装を Rust (`dtvmgr-jlse` クレート) に段階的に移植する。

### 元実装

- リポジトリ: `JoinLogoScpTrialSetLinux/modules/join_logo_scp_trial/`
- 言語: Node.js
- 主要ソース: `src/jlse.js`, `src/channel.js`, `src/param.js`, `src/settings.js`

---

## Phase 1: チャンネル検出 + パラメータ検出

### アーキテクチャ

```
dtvmgr jlse channel --input <file>
dtvmgr jlse param   --input <file>

crates/dtvmgr-jlse/src/
  lib.rs           # Public API re-exports
  types.rs         # Channel, Param, DetectionParam
  channel.rs       # ChList.csv parse + detection
  param.rs         # ChParamJL*.csv parse + detection
```

---

## 型定義

### `Channel`

`ChList.csv` の 1 レコード。放送局情報を表す。

| Field        | Type     | Description                             | Example          |
| ------------ | -------- | --------------------------------------- | ---------------- |
| `recognize`  | `String` | 放送局名 (認識用) - 全角日本語          | `"ＮＨＫＢＳ１"` |
| `install`    | `String` | 放送局名 (設定用) - 通常空              | `""`             |
| `short`      | `String` | 略称 - ロゴファイル名やパラメータ参照用 | `"BS1"`          |
| `service_id` | `String` | サービス ID                             | `"101"`          |

### `Param`

`ChParamJL1.csv` / `ChParamJL2.csv` の 1 レコード。

| Field          | Type     | Description                             | Example             |
| -------------- | -------- | --------------------------------------- | ------------------- |
| `channel`      | `String` | 放送局略称 (ChList の `short` に対応)   | `"NHK-G"`           |
| `title`        | `String` | タイトルパターン (部分一致 or 正規表現) | `""`                |
| `jl_run`       | `String` | JL コマンドファイル名                   | `"JL_NHK.txt"`      |
| `flags`        | `String` | フラグ文字列                            | `"fLOff,fHCWOWA"`   |
| `options`      | `String` | join_logo_scp 追加オプション            | `"-set divnext..."` |
| `comment_view` | `String` | 表示用コメント                          | `"NHK用..."`        |
| `comment`      | `String` | 内部コメント                            | `""`                |

### `DetectionParam`

チャンネルとファイル名からマージされた検出結果。

| Field     | Type     | Description           |
| --------- | -------- | --------------------- |
| `jl_run`  | `String` | JL コマンドファイル名 |
| `flags`   | `String` | フラグ文字列          |
| `options` | `String` | 追加オプション        |

---

## チャンネル検出 (`channel.rs`)

### CSV フォーマット: `ChList.csv`

```csv
# ヘッダコメント行 (スキップ)
放送局名（認識用）,放送局名（設定用）,略称,サービスID
ＮＨＫＢＳ１,,BS1,101
ＮＨＫＢＳプレミアム,,BSP,103
```

- 先頭 1 行 (コメント/ヘッダ) をスキップし、2 行目以降をパース
- `csv` クレートの `from` 相当で 2 行目からパースする (`csv::ReaderBuilder::has_headers(false)` + 手動スキップ、または `has_headers(true)` でカラム名マッピング)
- 4 列固定: `recognize`, `install`, `short`, `service_id`

### 検出アルゴリズム

入力: ファイルパスのベースネーム + オプションのチャンネル名
前処理: NFKC 正規化 (`unicode-normalization` クレート) で全角英数を半角に統一

#### 1. チャンネル名指定時 (`--channel` or `CHNNELNAME` 環境変数)

チャンネル名が指定された場合、以下の順序で前方一致検索:

1. `recognize` (NFKC 正規化済み) で前方一致
2. `short` (NFKC 正規化済み) で前方一致
3. `service_id` で前方一致
4. チャンネル名から末尾以外の 1 桁数字を除去して `recognize` で前方一致

一致しない場合はファイル名検索にフォールバック。

#### 2. ファイル名からの検出 (デフォルト)

優先度 1 (即時 return):

| 対象         | パターン                                                                                               |
| ------------ | ------------------------------------------------------------------------------------------------------ |
| `recognize`  | `^{recognize}` or `_` の後に `{recognize}`                                                             |
| `short`      | `^{short}[_ ]` or `_` の後 or 括弧の後 (`[({〔[{〈《｢『【≪]`) に `short` が出現し、後ろが括弧/空白/`_` |
| `service_id` | `short` と同じパターンで `service_id` を使用                                                           |

優先度 2 (候補記録、探索継続):

| 対象        | パターン                        |
| ----------- | ------------------------------- |
| `recognize` | 括弧の直後に `recognize` が出現 |

優先度 3 (より低い候補):

| 対象         | パターン                                                 |
| ------------ | -------------------------------------------------------- |
| `short`      | `[ _]` の後に `short` が出現し、後ろが括弧/空白/`_`      |
| `service_id` | `[ _]` の後に `service_id` が出現し、後ろが括弧/空白/`_` |

優先度 4 (最低):

| 対象        | パターン                             |
| ----------- | ------------------------------------ |
| `recognize` | `_` or 空白の後に `recognize` が出現 |

### 括弧文字セット

検出で使用される括弧文字:

- 開き括弧: `(`, `〔`, `[`, `{`, `〈`, `《`, `｢`, `『`, `【`, `≪`
- 閉じ括弧: `)`, `〕`, `]`, `}`, `〉`, `》`, `｣`, `』`, `】`, `≫`

### パブリック API

```rust
/// Loads channel entries from ChList.csv.
pub fn load_channels(csv_path: &Path) -> Result<Vec<Channel>>;

/// Detects the broadcast channel from a filename.
///
/// Returns `None` if no channel matches.
pub fn detect_channel(
    channels: &[Channel],
    filename: &str,
    channel_name: Option<&str>,
) -> Option<Channel>;
```

---

## パラメータ検出 (`param.rs`)

### CSV フォーマット: `ChParamJL1.csv` / `ChParamJL2.csv`

```csv
# ヘッダコメント行 (スキップ)
放送局略称,タイトル,JL_RUN,FLAGS,OPTIONS,#コメント表示用,#コメント
,,JL_フラグ指定.txt,@,@,,デフォルト設定。先頭@マークは設定クリア
NHK-G,,JL_NHK.txt,,,NHK用前後（他番組宣伝）カット,
```

- 先頭 1 行 (コメント/ヘッダ) をスキップ
- 7 列固定: `channel`, `title`, `jl_run`, `flags`, `options`, `comment_view`, `comment`
- `#` で始まる `channel` 値はコメント行としてスキップ

### 検出アルゴリズム

入力: `Vec<Param>` (JL1), `Vec<Param>` (JL2), `Option<Channel>`, ファイル名

1. 検索キーは `channel.short` (チャンネル未検出時は `"__normal"`)
2. CSV の各行について:
   - `channel` フィールドが `#` で始まる → スキップ
   - `channel` が検索キーと一致するか確認
   - 一致し、かつ `title` が指定されている場合:
     - `title` に正規表現メタ文字 (`.*+?|[]^`) が含まれる → 正規表現マッチ
     - それ以外 → 部分文字列マッチ (NFKC 正規化済み)
   - 一致し、`title` が空 → 無条件マッチ
3. マッチした行のフィールドをマージ:
   - 値が `"@"` → そのフィールドを空文字にクリア
   - 値が空 → 既存値を維持 (上書きしない)
   - 値が非空 → 上書き
4. どの行にもマッチしない → 1 行目 (デフォルト行) の値を使用
5. JL1 の結果に JL2 の結果をマージ (`Object.assign` 相当)

### `@` マーカーの動作

`flags` や `options` が `"@"` の場合、そのフィールドの値を空文字列にリセットする。これは前の CSV で設定された値を明示的にクリアするために使用される。

### パブリック API

```rust
/// Loads parameter entries from a ChParamJL CSV file.
pub fn load_params(csv_path: &Path) -> Result<Vec<Param>>;

/// Detects JL parameters by matching channel and filename.
///
/// Searches JL1 first, then JL2, merging results.
pub fn detect_param(
    params_jl1: &[Param],
    params_jl2: &[Param],
    channel: Option<&Channel>,
    filename: &str,
) -> DetectionParam;
```

---

## 設定 (`dtvmgr.toml`)

`AppConfig` に `jlse: Option<JlseConfig>` フィールドを追加:

```toml
[jlse]
jl_dir = "/path/to/JL" # JL/ ディレクトリ (ChList.csv, ChParamJL*.csv, JL コマンドファイル)
logo_dir = "/path/to/logo" # logo/ ディレクトリ (*.lgd ファイル)
result_dir = "/path/to/result" # result/ 出力先
```

```rust
/// Configuration for the jlse CM detection pipeline.
pub struct JlseConfig {
    pub jl_dir: PathBuf,
    pub logo_dir: PathBuf,
    pub result_dir: PathBuf,
}
```

---

## CLI サブコマンド

### `dtvmgr jlse channel`

ファイル名からチャンネルを検出して表示。

```
dtvmgr jlse channel --input /path/to/[BS11]番組名.ts
dtvmgr jlse channel --input /path/to/番組名.ts --channel NHK-G
```

出力例:

```
recognize: ＢＳ１１イレブン
install:
short: BS11
service_id: 211
```

### `dtvmgr jlse param`

チャンネル + ファイル名からパラメータを検出して表示。

```
dtvmgr jlse param --input /path/to/[NHK-G]番組名.ts
```

出力例:

```
jl_run: JL_NHK.txt
flags:
options:
```

---

## 後続 Phase ロードマップ

### Phase 2: 外部コマンド実行 + AVS 生成

- `command/` モジュール: `chapter_exe`, `logoframe`, `join_logo_scp` の非同期実行
- `output/avs.rs`: `in_org.avs` テンプレート生成、AVS ファイル連結
- `tokio::process::Command` による非同期プロセス管理

### Phase 3: チャプター生成 + パイプライン統合

- `output/chapter.rs`: `obs_cut.avs` の `Trim()` パース、3 フォーマット出力 (ORG/CUT/TVT)
- `pipeline.rs`: 全ステップのオーケストレーション
- `dtvmgr jlse run` サブコマンド

### Phase 4: オプション機能

- `tsdivider`: TS 分割前処理
- `ffmpeg_filter.rs`: FFmpeg `filter_complex` 文字列生成
- `ffmpeg.rs`: エンコード実行
- クリーンアップ (`--remove`)

---

## テスト方針

### ユニットテスト

| 対象         | テスト内容                                                        |
| ------------ | ----------------------------------------------------------------- |
| `channel.rs` | CSV パース、優先度別マッチング、NFKC 正規化、括弧内検出           |
| `param.rs`   | CSV パース、チャンネル一致、タイトル正規表現/部分一致、`@` クリア |
| `types.rs`   | `Debug`/`Clone` derive 確認                                       |

### テストデータ

- `channel.rs`: 少数のチャンネルエントリをインラインで定義
- `param.rs`: デフォルト行 + NHK + WOWOW 等の代表的なエントリをインラインで定義
