# Pipeline Orchestration

> 親ドキュメント: [Architecture.md](./Architecture.md)

## ステータス

- **Phase**: 3
- **実装状態**: 実装済み
- **Node.js ソース**: `src/jlse.js` (165行)
- **Rust モジュール**: `crates/dtvmgr-jlse/src/pipeline.rs` + CLI

## 概要

全ステップを順次実行するパイプラインオーケストレーター。CLI 引数をパースし、チャンネル検出からエンコードまでの全処理を制御する。

## 仕様

### CLI 引数 (yargs → clap マッピング)

| yargs オプション | 短縮  | 型        | デフォルト     | 説明                               | clap 案                 |
| ---------------- | ----- | --------- | -------------- | ---------------------------------- | ----------------------- |
| `--input`        | `-i`  | `string`  | (必須)         | 入力 TS ファイルパス               | `--input` / `-i`        |
| `--filter`       | `-f`  | `boolean` | `false`        | ffmpeg フィルタ出力を有効化        | `--filter` / `-f`       |
| `--addchapter`   | `-ac` | `boolean` | `false`        | エンコード時にチャプター付与       | `--add-chapter`         |
| `--channel`      | `-c`  | `boolean` | `false`        | 環境変数 `CHNNELNAME` を参照       | `--channel <name>`      |
| `--encode`       | `-e`  | `boolean` | `false`        | ffmpeg エンコードを有効化          | `--encode` / `-e`       |
| `--target`       | `-t`  | `choice`  | `"cutcm_logo"` | エンコード対象 AVS                 | `--target <cutcm/logo>` |
| `--option`       | `-o`  | `string`  | `""`           | ffmpeg 追加オプション              | `--ffmpeg-option`       |
| `--outdir`       | `-d`  | `string`  | `""`           | エンコード出力先ディレクトリ       | `--outdir`              |
| `--outname`      | `-n`  | `string`  | `""`           | エンコード出力ファイル名           | `--outname`             |
| `--remove`       | `-r`  | `boolean` | `false`        | 処理後に中間ファイルを削除         | `--remove` / `-r`       |
| (なし)           |       | `boolean` | `false`        | TUI 進捗表示                       | `--tui`                 |
| (なし)           |       | `boolean` | `false`        | EPGStation モード                  | `--epgstation`          |
| (なし)           |       | `boolean` | `false`        | エンコード前尺チェックをスキップ   | `--skip-duration-check` |
| (なし)           |       | `boolean` | `false`        | ステップキャッシュを無視して再実行 | `--force`               |

### パイプライン実行順序

パイプライン全体フローは [Architecture.md](./Architecture.md) の mermaid 図を参照。

1. 入力ファイルの拡張子チェック (`.ts` / `.m2ts`)
2. チャンネル検出 ([channel.md](./channel.md))
3. パラメータ検出 ([param.md](./param.md))
4. `obs_param.txt` 書き出し
5. 入力 AVS 生成 ([avs.md](./avs.md))
6. chapter_exe ([chapter_exe.md](./chapter_exe.md)) ※ステップキャッシュ対象
7. logoframe ([logoframe.md](./logoframe.md)) ※ステップキャッシュ対象
8. join_logo_scp ([join_logo_scp.md](./join_logo_scp.md)) ※ステップキャッシュ対象
9. AVS 連結 ([output_avs.md](./output_avs.md))
10. チャプター生成 ([chapter.md](./chapter.md))
11. (任意) FFmpeg フィルタ生成 ([ffmpeg_filter.md](./ffmpeg_filter.md))
    11.5. (任意) VMAF 品質探索 (`quality_search.enabled` 時)
12. (任意) エンコード
    - 12a. EIT 抽出 (MKV メタデータ用)
    - 12b. エンコード前尺チェック ([validate.md](./validate.md))
    - 12c. ffmpeg エンコード ([ffmpeg.md](./ffmpeg.md))
13. (任意) 中間ファイル削除

**ステップキャッシュ**: `chapter_exe`, `logoframe`, `join_logo_scp` の出力はキャッシュされ、再実行時にスキップされる。`--force` フラグでキャッシュを無視して再実行可能。

**動的ステージ数**: 品質探索が有効な場合はステージ数が 5 に増加する(無効時は 4)。パイプラインサマリは構造化ログ(入力/出力のネスト JSON)として出力される。

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

#### `dtvmgr jlse run`

完全なパイプラインを実行。

```
dtvmgr jlse run --input /path/to/recording.ts [--encode] [--filter]
dtvmgr jlse run --input /path/to/recording.ts --encode --tui
dtvmgr jlse run --epgstation [--tui]
dtvmgr jlse run --input /path/to/recording.ts --encode --skip-duration-check
dtvmgr jlse run --input /path/to/recording.ts --encode --force
```

## 型定義

```rust
/// Pipeline execution context.
pub struct PipelineContext {
    pub input: PathBuf,
    pub channel_name: Option<String>,
    pub config: JlseConfig,
    pub filter: bool,
    pub encode: bool,
    pub target: AvsTarget,
    pub add_chapter: bool,
    pub ffmpeg_option: Option<String>,
    pub out_dir: Option<PathBuf>,
    pub out_name: Option<String>,
    pub remove: bool,
    pub progress_mode: Option<ProgressMode>,
    pub skip_duration_check: bool,
    pub force: bool,
    pub out_extension: Option<String>,
}

/// Encode target AVS selection.
pub enum AvsTarget {
    /// `in_cutcm.avs` (cut only)
    CutCm,
    /// `in_cutcm_logo.avs` (cut + logo removal)
    CutCmLogo,
}
```

## VMAF 品質探索

`[jlse.encode.quality_search]` が有効な場合、フィルタ生成後・エンコード前に VMAF ベースの品質パラメータ探索(Step 11.5)を実行する。`dtvmgr-vmaf` クレートを利用し、補間二分探索アルゴリズムで目標 VMAF スコアを満たす最適な CRF 値を自動決定する。

- TS からサンプルを `-c:v copy` で抽出
- 1080p にアップスケールした VMAF 測定
- `n_subsample` によるVMAF 計算の高速化をサポート
- 対応エンコーダ: `av1_qsv`, `libsvtav1`, `h264_qsv`, `hevc_qsv`, `libx264`, `libx265`

### キャッシュ動作

品質探索の結果は `obs_quality_search.json` に保存される。次回実行時にキャッシュファイルが存在する場合、探索はスキップされ保存済み結果を使用する。

キャッシュはコンフィグスナップショット(エンコーダ設定・ `obs_cut.avs` の内容を含む)と照合され、設定が変更されている場合は自動的に無効化されて探索が再実行される。`--force` フラグを指定するとキャッシュを無視して常に再実行する。

### QSV VPP HW エンコード対応

`filter_hw_device` が設定されている場合、VMAF 品質探索は HW アクセラレーションを活用してサンプルのエンコードとリファレンス生成を行う。以下の 3 つのヘルパー関数がフィルタチェーンを構築する:

- **`build_vmaf_hw_input_args`**: `JlseEncode` の `input.init_hw_device` / `input.filter_hw_device` を抽出し、`-init_hw_device` / `-filter_hw_device` 引数を生成する。HW フィルタデバイスが未設定の場合は空の `Vec` を返す
- **`build_vmaf_video_filter`**: `filter_hw_device` が設定されている場合は `prepare_hw_filter` に委譲し、`format=nv12,hwupload=extra_hw_frames=64` の先頭付加と `vpp_qsv` セグメントへの `format={pix_fmt}` インジェクションを行う。SW フィルタの場合はそのまま返す
- **`build_vmaf_reference_filter`**: HW フィルタ使用時に `{video_filter},hwdownload,format={fmt}` を返す。FFV1 は CPU のみのエンコーダであるため、QSV VPP が出力する HW サーフェスフレームを `hwdownload` でシステムメモリに戻す必要がある。`hwdownload` 後に明示的な `format=` が必要であり、`pix_fmt` が設定されている場合はその値を、未設定の場合は `nv12` をデフォルトとして使用する。HW フィルタが未設定の場合は `None` を返し、呼び出し側は `video_filter` にフォールバックする

### VPP フィルタバリデーション

`validate_encode_config` はパイプラインのステップ実行前に `validate_vpp_no_format` を呼び出し、VPP フィルタセグメント内の冗長な `:format=` パラメータを検出してエラーにする。`filter_hw_device` が設定されている場合、ピクセルフォーマットは `[jlse.encode.video] pix_fmt` が唯一の真のソースであり、VPP 内の `format=` は冗長かつ不整合の原因となる。

## テスト方針

- CLI 引数パース: `clap` でのパース結果が正しいこと
- パイプライン全体の結合テスト (外部バイナリはモック)
- VMAF 品質探索: モックベースのパイプラインテスト

## 依存モジュール

- 全モジュールに依存 (オーケストレーター)
