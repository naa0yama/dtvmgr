# tsdivider Command Wrapper

> 親ドキュメント: [Architecture.md](./Architecture.md)

## ステータス

- **状態**: **廃止済み (removed)**
- 廃止理由: tsdivider による TS ストリーム分割の前処理は不要と判断し、コードベースから削除済み。
- 削除コミット以前の実装: `crates/dtvmgr-jlse/src/command/tsdivider.rs`

## 概要 (旧仕様)

TS ストリームを分割する前処理コマンド `tsdivider` のラッパー。`--tsdivider` オプション指定時のみ実行される任意ステップだった。

### 引数構築 (旧仕様)

```
tsdivider -i <input_file> --overlap_front 0 -o <tsdivider_output>
```

| 引数              | 値                    | 説明               |
| ----------------- | --------------------- | ------------------ |
| `-i`              | `<input_file>`        | 入力 TS ファイル   |
| `--overlap_front` | `0`                   | 前方オーバーラップ |
| `-o`              | `<filename>_split.ts` | 出力ファイル       |
