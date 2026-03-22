# AVS File Concatenation

> 親ドキュメント: [Architecture.md](./Architecture.md)

## ステータス

- **Phase**: 2
- **実装状態**: 未実装
- **Node.js ソース**: `src/output/avs.js` (37行)
- **Rust モジュール**: `crates/dtvmgr-jlse/src/output/avs.rs`

## 概要

複数の AVS ファイルを連結して、CM カット済みの入力 AVS ファイルを生成する。`in_cutcm.avs` (カットのみ) と `in_cutcm_logo.avs` (カット + ロゴ除去) の 2 ファイルを出力する。

## 仕様

### 出力ファイル

| 変数名                   | ファイル名          | 構成                                                |
| ------------------------ | ------------------- | --------------------------------------------------- |
| `OUTPUT_AVS_IN_CUT`      | `in_cutcm.avs`      | `in_org.avs` + `obs_cut.avs`                        |
| `OUTPUT_AVS_IN_CUT_LOGO` | `in_cutcm_logo.avs` | `in_org.avs` + `obs_logo_erase.avs` + `obs_cut.avs` |

### 連結ロジック

Node.js ではストリーミングパイプで連結 (`fs.createReadStream` → `pipe` → `fs.createWriteStream`)。

Rust では単純なファイル読み書きで実装可能:

```rust
pub fn concat_avs(output_path: &Path, input_files: &[&Path]) -> Result<()> {
    let mut output = std::fs::File::create(output_path)
        .with_context(|| format!("failed to create {}", output_path.display()))?;
    for input_file in input_files {
        let content = std::fs::read(input_file)
            .with_context(|| format!("failed to read {}", input_file.display()))?;
        output.write_all(&content)
            .with_context(|| format!("failed to write to {}", output_path.display()))?;
    }
    Ok(())
}
```

## テスト方針

- 連結の正確性: 複数ファイルが正しい順序で連結されること
- 2 ファイル構成 (`in_cutcm.avs`) と 3 ファイル構成 (`in_cutcm_logo.avs`) の両方を検証

## 依存モジュール

- [settings.md](./settings.md) — `OutputPaths.input_avs`, `OutputPaths.output_avs_cut`, `OutputPaths.logoframe_avs_output`, `OutputPaths.output_avs_in_cut`, `OutputPaths.output_avs_in_cut_logo`
