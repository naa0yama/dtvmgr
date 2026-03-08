# Settings

> 親ドキュメント: [Architecture.md](./Architecture.md)

## ステータス

- **Phase**: 2
- **実装状態**: 実装済み
- **Node.js ソース**: `src/settings.js` (44行)
- **Rust モジュール**: `crates/dtvmgr-jlse/src/settings.rs`

## 概要

外部バイナリのパスと出力ファイルパスを管理する。`dtvmgr.toml` から JL ディレクトリ・ロゴディレクトリ・結果ディレクトリを読み込み、パイプライン実行時に全出力パスを初期化する。

## 仕様

### パス定数

元実装では `settings.js` で以下のパスを管理する:

| 分類         | 変数名               | 値                                |
| ------------ | -------------------- | --------------------------------- |
| CSV データ   | `CHANNEL_LIST`       | `<module>/JL/data/ChList.csv`     |
|              | `PARAM_LIST_1`       | `<module>/JL/data/ChParamJL1.csv` |
|              | `PARAM_LIST_2`       | `<module>/JL/data/ChParamJL2.csv` |
| バイナリ     | `LOGOFRAME_COMMAND`  | `<module>/bin/logoframe`          |
|              | `CHAPTEREXE_COMMAND` | `<module>/bin/chapter_exe`        |
|              | `JLSCP_COMMAND`      | `<module>/bin/join_logo_scp`      |
|              | `FFPROBE_COMMAND`    | `/usr/local/bin/ffprobe`          |
|              | `FFMPEG_COMMAND`     | `/usr/local/bin/ffmpeg`           |
| ディレクトリ | `JL_DIR`             | `<module>/JL`                     |
|              | `LOGO_PATH`          | `<module>/logo`                   |

### `OutputPaths` 構造体定義

`init(filename)` 関数で初期化される出力パス。全 15 ファイルを管理する:

```rust
/// All output file paths for a single processing run.
pub struct OutputPaths {
    /// Base output directory: `<result_dir>/<filename>/`
    pub save_dir: PathBuf,
    /// Input AVS file: `in_org.avs`
    pub input_avs: PathBuf,
    /// chapter_exe output: `obs_chapterexe.txt`
    pub chapterexe_output: PathBuf,
    /// logoframe text output: `obs_logoframe.txt`
    pub logoframe_txt_output: PathBuf,
    /// logoframe AVS output: `obs_logo_erase.avs`
    pub logoframe_avs_output: PathBuf,
    /// Merged parameter info: `obs_param.txt`
    pub obs_param_path: PathBuf,
    /// join_logo_scp structure output: `obs_jlscp.txt`
    pub jlscp_output: PathBuf,
    /// Cut AVS (Trim commands): `obs_cut.avs`
    pub output_avs_cut: PathBuf,
    /// Concatenated cut AVS: `in_cutcm.avs`
    pub output_avs_in_cut: PathBuf,
    /// Concatenated cut+logo AVS: `in_cutcm_logo.avs`
    pub output_avs_in_cut_logo: PathBuf,
    /// FFmpeg filter output: `ffmpeg.filter`
    pub output_filter_cut: PathBuf,
    /// Chapter ORG (all sections): `obs_chapter_org.chapter.txt`
    pub file_txt_cpt_org: PathBuf,
    /// Chapter CUT (non-cut only): `obs_chapter_cut.chapter.txt`
    pub file_txt_cpt_cut: PathBuf,
    /// Chapter TVTPlay format: `obs_chapter_tvtplay.chapter`
    pub file_txt_cpt_tvt: PathBuf,
}
```

### ディレクトリ初期化ロジック

`init_output_paths()` は以下を行う:

1. `<result_dir>/<filename>/` ディレクトリを作成 (Node.js: `fs.ensureDirSync`)
2. 上記の全パスをファイル名から生成して `OutputPaths` を返す

```rust
pub fn init_output_paths(result_dir: &Path, filename: &str) -> Result<OutputPaths> {
    let save_dir = result_dir.join(filename);
    std::fs::create_dir_all(&save_dir)
        .with_context(|| format!("failed to create output dir: {}", save_dir.display()))?;
    // ... 各フィールドを save_dir.join(...) で構築 ...
    Ok(OutputPaths { /* ... */ })
}
```

### 設定 (`dtvmgr.toml`)

`AppConfig` に `jlse: Option<JlseConfig>` フィールドを追加:

```toml
[jlse.dirs]
jl = "/path/to/JL" # JL/ ディレクトリ (ChList.csv, ChParamJL*.csv, JL コマンドファイル)
logo = "/path/to/logo" # logo/ ディレクトリ (*.lgd ファイル)
result = "/path/to/result" # result/ 出力先

[jlse.bins] # 省略可。未指定キーはデフォルト値を使用
# chapter_exe = "/custom/bin/chapter_exe"
# ffmpeg = "/usr/bin/ffmpeg"
```

`[jlse.bins]` セクションは省略可能。未指定のフィールドは以下のデフォルト導出を使用する:

- JL 系バイナリ (`logoframe`, `chapter_exe`, `join_logo_scp`): `<dirs.jl>/../bin/<name>`
- `ffprobe`, `ffmpeg`: `/usr/local/bin/<name>`

## 型定義

```rust
/// Configuration for the jlse CM detection pipeline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JlseConfig {
    /// Directory paths (JL, logo, result).
    pub dirs: JlseDirs,
    /// Binary path overrides. Omit to use defaults.
    #[serde(default)]
    pub bins: JlseBins,
}

/// Required directory paths for the pipeline.
pub struct JlseDirs {
    pub jl: PathBuf,
    pub logo: PathBuf,
    pub result: PathBuf,
}

/// Optional binary path overrides. `None` fields use default derivation.
#[derive(Default)]
pub struct JlseBins {
    pub logoframe: Option<PathBuf>,
    pub chapter_exe: Option<PathBuf>,
    pub join_logo_scp: Option<PathBuf>,
    pub ffprobe: Option<PathBuf>,
    pub ffmpeg: Option<PathBuf>,
}

/// Paths to external binary commands (resolved from config).
pub struct BinaryPaths {
    pub logoframe: PathBuf,
    pub chapter_exe: PathBuf,
    pub join_logo_scp: PathBuf,
    pub ffprobe: PathBuf,
    pub ffmpeg: PathBuf,
}
```

## テスト方針

- パス生成の正確性: 各フィールドが `save_dir.join(...)` で正しく構築されること
- ディレクトリ作成: `create_dir_all` が呼ばれること (tmpdir でテスト)

## 依存モジュール

なし (他モジュールがこのモジュールに依存する)
