# Input AVS Generation

> 親ドキュメント: [Architecture.md](./Architecture.md)

## ステータス

- **Phase**: 2
- **実装状態**: 未実装
- **Node.js ソース**: `src/jlse.js` 内 `createAvs()` 関数
- **Rust モジュール**: `crates/dtvmgr-jlse/src/avs.rs`

## 概要

L-SMASH Works (LWLibavSource) ベースの AviSynth 入力スクリプトを生成する。TS ファイルパスと `stream_index` を受け取り、テンプレート文字列を出力ファイルに書き込む。

## 仕様

### テンプレート文字列

```avs
TSFilePath="<input_file_path>"
LWLibavVideoSource(TSFilePath, repeat=true, dominance=1)
AudioDub(last,LWLibavAudioSource(TSFilePath, stream_index=<index>, av_sync=true))
```

### `stream_index` の決定ロジック

| 条件 | `stream_index` | 理由                       |
| ---- | -------------- | -------------------------- |
| 通常 | `1`            | デフォルトの音声ストリーム |

### Rust 実装案

```rust
/// Generates an AviSynth input script for L-SMASH Works.
pub fn create_avs(output_path: &Path, input_file: &Path, stream_index: i32) -> Result<()> {
    let content = format!(
        "TSFilePath=\"{}\"\n\
         LWLibavVideoSource(TSFilePath, repeat=true, dominance=1)\n\
         AudioDub(last,LWLibavAudioSource(TSFilePath, stream_index={}, av_sync=true))\n",
        input_file.display(),
        stream_index
    );
    std::fs::write(output_path, content)
        .with_context(|| format!("failed to write AVS: {}", output_path.display()))?;
    Ok(())
}
```

## テスト方針

- テンプレート文字列の正確性: 生成された AVS ファイルが期待するフォーマットと一致すること
- `stream_index` の反映: 通常時は `1` が正しく出力されること

## 依存モジュール

- [settings.md](./settings.md) — `OutputPaths.input_avs` を出力先パスとして使用
