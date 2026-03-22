# join_logo_scp Command Wrapper

> 親ドキュメント: [Architecture.md](./Architecture.md)

## ステータス

- **Phase**: 2
- **実装状態**: 未実装
- **Node.js ソース**: `src/command/join_logo_frame.js` (47行)
- **Rust モジュール**: `crates/dtvmgr-jlse/src/command/join_logo_scp.rs`

## 概要

ロゴ検出結果と無音・シーンチェンジ情報を統合して CM 区間を決定する外部コマンド `join_logo_scp` のラッパー。構成解析結果 (`obs_jlscp.txt`) と Trim コマンド AVS (`obs_cut.avs`) を出力する。

## 仕様

### 引数構築

```
join_logo_scp -inlogo <logoframe_txt> -inscp <chapterexe_txt> -incmd <jl_command_file>
              -o <output_avs_cut> -oscp <jlscp_output> -flags <flags> [OPTIONS...]
```

| 引数      | 値                                | 説明                        |
| --------- | --------------------------------- | --------------------------- |
| `-inlogo` | `obs_logoframe.txt`               | logoframe 出力              |
| `-inscp`  | `obs_chapterexe.txt`              | chapter_exe 出力            |
| `-incmd`  | `<JL_DIR>/<param.JL_RUN>`         | JL コマンドファイル         |
| `-o`      | `obs_cut.avs`                     | Trim コマンド出力 AVS       |
| `-oscp`   | `obs_jlscp.txt`                   | 構成解析結果出力            |
| `-flags`  | `param.FLAGS`                     | フラグ文字列                |
| OPTIONS   | `param.OPTIONS.split(" ")` で分割 | 追加オプション (空白区切り) |

### OPTIONS 分割

`param.OPTIONS` が非空の場合、空白でスプリットして個別引数として追加。

### `obs_jlscp.txt` の行フォーマット

構成解析結果ファイル。各行は以下の正規表現でパースされる:

```
/^\s*(\d+)\s+(\d+)\s+(\d+)\s+([-\d]+)\s+(\d+).*:(\S+)/
```

| フィールド | 説明         | 例        |
| ---------- | ------------ | --------- |
| `$1`       | 開始フレーム | `0`       |
| `$2`       | 終了フレーム | `2696`    |
| `$3`       | 期間秒数     | `90`      |
| `$4`       | (分類値)     | `-1`      |
| `$5`       | (信頼度)     | `100`     |
| `$6`       | 構成コメント | `Sponsor` |

このフォーマットは [chapter.md](./chapter.md) でチャプター生成に使用される。

## テスト方針

- 引数構築: 正しい引数配列が生成されること
- OPTIONS 分割: 空白区切りで正しく個別引数に分割されること
- コマンド実行は統合テストで外部バイナリをモックして検証

## 依存モジュール

- [settings.md](./settings.md) — `BinaryPaths.join_logo_scp`, `OutputPaths`
- [param.md](./param.md) — `DetectionParam` (JL_RUN, FLAGS, OPTIONS)
- [chapter_exe.md](./chapter_exe.md) — 共通コマンド実行パターン
