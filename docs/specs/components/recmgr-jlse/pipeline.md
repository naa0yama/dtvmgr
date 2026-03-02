# Pipeline Orchestration

> 親ドキュメント: [Architecture.md](./Architecture.md)

## ステータス

- **Phase**: 3
- **実装状態**: 未実装
- **Node.js ソース**: `src/jlse.js` (165行)
- **Rust モジュール**: `crates/dtvmgr-jlse/src/pipeline.rs` + CLI

## 概要

全ステップを順次実行するパイプラインオーケストレーター。CLI 引数をパースし、チャンネル検出からエンコードまでの全処理を制御する。

## 仕様

### CLI 引数 (yargs → clap マッピング)

| yargs オプション | 短縮   | 型        | デフォルト     | 説明                         | clap 案                 |
| ---------------- | ------ | --------- | -------------- | ---------------------------- | ----------------------- |
| `--input`        | `-i`   | `string`  | (必須)         | 入力 TS ファイルパス         | `--input` / `-i`        |
| `--filter`       | `-f`   | `boolean` | `false`        | ffmpeg フィルタ出力を有効化  | `--filter` / `-f`       |
| `--addchapter`   | `-ac`  | `boolean` | `false`        | エンコード時にチャプター付与 | `--add-chapter`         |
| `--channel`      | `-c`   | `boolean` | `false`        | 環境変数 `CHNNELNAME` を参照 | `--channel <name>`      |
| `--encode`       | `-e`   | `boolean` | `false`        | ffmpeg エンコードを有効化    | `--encode` / `-e`       |
| `--target`       | `-t`   | `choice`  | `"cutcm_logo"` | エンコード対象 AVS           | `--target <cutcm/logo>` |
| `--tsdivider`    | `-tsd` | `boolean` | `false`        | tsdivider による前処理       | `--tsdivider`           |
| `--option`       | `-o`   | `string`  | `""`           | ffmpeg 追加オプション        | `--ffmpeg-option`       |
| `--outdir`       | `-d`   | `string`  | `""`           | エンコード出力先ディレクトリ | `--outdir`              |
| `--outname`      | `-n`   | `string`  | `""`           | エンコード出力ファイル名     | `--outname`             |
| `--remove`       | `-r`   | `boolean` | `false`        | 処理後に中間ファイルを削除   | `--remove` / `-r`       |

### パイプライン実行順序

パイプライン全体フローは [Architecture.md](./Architecture.md) の mermaid 図を参照。

1. 入力ファイルの拡張子チェック (`.ts` / `.m2ts`)
2. チャンネル検出 ([channel.md](./channel.md))
3. パラメータ検出 ([param.md](./param.md))
4. (任意) tsdivider ([tsdivider.md](./tsdivider.md))
5. 入力 AVS 生成 ([avs.md](./avs.md))
6. chapter_exe ([chapter_exe.md](./chapter_exe.md))
7. logoframe ([logoframe.md](./logoframe.md))
8. join_logo_scp ([join_logo_scp.md](./join_logo_scp.md))
9. AVS 連結 ([output_avs.md](./output_avs.md))
10. チャプター生成 ([chapter.md](./chapter.md))
11. (任意) FFmpeg フィルタ生成 ([ffmpeg_filter.md](./ffmpeg_filter.md))
12. (任意) ffmpeg エンコード ([ffmpeg.md](./ffmpeg.md))
13. (任意) 中間ファイル削除

### CLI サブコマンド

#### `dtvmgr jlse channel`

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

#### `dtvmgr jlse param`

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

#### `dtvmgr jlse run` (Phase 3)

完全なパイプラインを実行。

```
dtvmgr jlse run --input /path/to/recording.ts [--encode] [--filter] [--tsdivider]
```

## 型定義

```rust
/// CLI arguments for the jlse pipeline.
pub struct PipelineArgs {
    pub input: PathBuf,
    pub filter: bool,
    pub add_chapter: bool,
    pub channel_name: Option<String>,
    pub encode: bool,
    pub target: AvsTarget,
    pub tsdivider: bool,
    pub ffmpeg_option: Option<String>,
    pub out_dir: Option<PathBuf>,
    pub out_name: Option<String>,
    pub remove: bool,
}

/// Encode target AVS selection.
pub enum AvsTarget {
    /// `in_cutcm.avs` (cut only)
    CutCm,
    /// `in_cutcm_logo.avs` (cut + logo removal)
    CutCmLogo,
}
```

## テスト方針

- CLI 引数パース: `clap` でのパース結果が正しいこと
- パイプライン全体の結合テスト (外部バイナリはモック)

## 依存モジュール

- 全モジュールに依存 (オーケストレーター)
