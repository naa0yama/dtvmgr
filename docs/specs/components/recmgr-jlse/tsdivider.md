# tsdivider Command Wrapper

> 親ドキュメント: [Architecture.md](./Architecture.md)

## ステータス

- **Phase**: 2
- **実装状態**: 未実装
- **Node.js ソース**: `src/command/tsdivider.js` (13行)
- **Rust モジュール**: `crates/dtvmgr-jlse/src/command/tsdivider.rs`

## 概要

TS ストリームを分割する前処理コマンド `tsdivider` のラッパー。`--tsdivider` オプション指定時のみ実行される任意ステップ。

## 仕様

### 引数構築

```
tsdivider -i <input_file> --overlap_front 0 -o <tsdivider_output>
```

| 引数              | 値                    | 説明               |
| ----------------- | --------------------- | ------------------ |
| `-i`              | `<input_file>`        | 入力 TS ファイル   |
| `--overlap_front` | `0`                   | 前方オーバーラップ |
| `-o`              | `<filename>_split.ts` | 出力ファイル       |

### 実行方式

元実装では同期実行 (`spawnSync`) で `stdio: "inherit"` (標準出力をそのまま表示)。

Rust では `tokio::process::Command` で非同期実行し、stdout/stderr を継承する。

## テスト方針

- 引数構築: 正しい引数配列が生成されること
- コマンド実行は統合テストで外部バイナリをモックして検証

## 依存モジュール

- [settings.md](./settings.md) — `BinaryPaths.tsdivider`, `OutputPaths.tsdivider_output`
- [chapter_exe.md](./chapter_exe.md) — 共通コマンド実行パターン
