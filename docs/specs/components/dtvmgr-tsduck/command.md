# TSDuck Command Wrappers

> 親ドキュメント: [Architecture.md](./Architecture.md)

## ステータス

- **実装状態**: 完了
- **Rust モジュール**: `crates/dtvmgr-tsduck/src/command.rs`

## 概要

`TSDuck` の `tstables` / `tsp` コマンドを同期的に実行し、標準出力のキャプチャやステータスコード検証を行うラッパー群。

## 仕様

### ヘルパー関数 (非公開)

| 関数          | 方式                | stdin | stdout  | stderr  | 戻り値           |
| ------------- | ------------------- | ----- | ------- | ------- | ---------------- |
| `run`         | `Command::status()` | —     | inherit | inherit | `Result<()>`     |
| `run_capture` | `Command::output()` | —     | capture | inherit | `Result<String>` |

全関数ともコマンドの終了コードが非ゼロの場合 `bail!` でエラーを返す。

### パブリック API

#### EIT 抽出

```rust
/// Extract EIT XML from a TS file using `tstables`.
pub fn extract_eit(binary: &Path, input_file: &Path) -> Result<String>;

/// Build the argument list for `tstables` EIT extraction.
pub fn build_eit_args(input_file: &Path) -> Vec<String>;
```

コマンド: `tstables --japan --pid 0x12 --xml-output - <input>`

#### PAT 抽出

```rust
/// Extract PAT XML from a TS file using `tstables`.
pub fn extract_pat(binary: &Path, input_file: &Path) -> Result<String>;

/// Build the argument list for `tstables` PAT extraction.
pub fn build_pat_args(input_file: &Path) -> Vec<String>;
```

コマンド: `tstables --japan --pid 0 --xml-output - <input>`

PAT は PID `0` 上の小さなテーブルのため、大容量録画でも高速に完了する。

#### EIT p/f 抽出

```rust
/// Extract EIT p/f XML from a TS file using `tstables`.
pub fn extract_eit_pf(binary: &Path, input_file: &Path) -> Result<String>;

/// Build the argument list for `tstables` EIT p/f extraction.
pub fn build_eit_pf_args(input_file: &Path) -> Vec<String>;
```

コマンド: `tstables --japan --pid 0x12 --tid 0x4E --max-tables 4 --xml-output - <input>`

- `--tid 0x4E`: EIT p/f actual のみ (schedule 除外)
- `--max-tables 4`: 早期終了 (p/f は通常2テーブル)

#### EIT p/f チャンク抽出

```rust
/// Extract EIT p/f XML from an in-memory TS chunk using `tstables`.
pub fn extract_eit_from_chunk(binary: &Path, chunk: &[u8]) -> Result<String>;
```

- `tstables` は stdin 非対応のため、内部で `tempfile::NamedTempFile` にチャンクを書き出し `extract_eit_pf` を呼ぶ
- 一時ファイルは `NamedTempFile` の `Drop` で自動削除される

#### サービスフィルタリング

```rust
/// Filter a TS file by service ID using `tsp`.
pub fn filter_service(
    binary: &Path,
    input_file: &Path,
    output_file: &Path,
    sid: &str,
) -> Result<()>;

/// Build the argument list for `tsp` service filtering.
pub fn build_filter_service_args(
    input_file: &Path,
    output_file: &Path,
    sid: &str,
) -> Vec<String>;
```

コマンド: `tsp --japan -I file <input> -P zap <sid> -O file <output>`

### コマンド引数一覧

| コマンド   | 引数           | 値           | 説明                       |
| ---------- | -------------- | ------------ | -------------------------- |
| `tstables` | `--japan`      | —            | 日本向け ARIB 仕様で解釈   |
| `tstables` | `--pid`        | `0` / `0x12` | 抽出対象 PID               |
| `tstables` | `--xml-output` | `-`          | XML を標準出力に出力       |
| `tstables` | `--tid`        | `0x4E`       | EIT p/f actual テーブル ID |
| `tstables` | `--max-tables` | `4`          | 最大テーブル数 (早期終了)  |
| `tsp`      | `--japan`      | —            | 日本向け ARIB 仕様         |
| `tsp`      | `-I file`      | `<path>`     | ファイル入力プラグイン     |
| `tsp`      | `-P zap`       | `<sid>`      | サービス ID でフィルタ     |
| `tsp`      | `-O file`      | `<path>`     | ファイル出力プラグイン     |

## テスト方針

- 引数構築: `build_eit_args`, `build_pat_args`, `build_eit_pf_args`, `build_filter_service_args` の各出力を検証
- コマンド実行: `write_script` ヘルパーでシェルスクリプトモックを作成し、成功 (`exit 0`) / 失敗 (`exit 1`) パスを検証
- Miri: コマンド実行テスト (`tempfile` + `fork`/`exec`) は `#[cfg_attr(miri, ignore)]` で除外

## 依存モジュール

- [seek.md](./seek.md) — `extract_eit_from_chunk` の入力チャンクを提供
