# ffmpeg Command Wrapper

> 親ドキュメント: [Architecture.md](./Architecture.md)

## ステータス

- **Phase**: 4
- **実装状態**: 未実装
- **Node.js ソース**: `src/command/ffmpeg.js` (64行)
- **Rust モジュール**: `crates/dtvmgr-jlse/src/command/ffmpeg.rs`

## 概要

最終エンコードを実行する外部コマンド `ffmpeg` のラッパー。AVS ファイルを入力として mp4 ファイルを生成し、オプションでチャプターメタデータを付与する。

## 仕様

### 引数構築

```
ffmpeg -hide_banner -y -ignore_unknown -i <avs_file>
       [-i <chapter_file> -map_metadata 1
        -metadata title=<HALF_WIDTH_NAME>
        -metadata comment=<description_block>
        -movflags +use_metadata_tags]
       [<ffmpeg_option>...]
       <output_dir>/<output_name>.mp4
```

### AVS ファイル選択

| `--target` 値 | 入力 AVS            |
| ------------- | ------------------- |
| `cutcm`       | `in_cutcm.avs`      |
| `cutcm_logo`  | `in_cutcm_logo.avs` |

### チャプター付与時 (`--addchapter`)

- 2 つ目の入力として `obs_chapter_cut.chapter.txt` を追加
- `-map_metadata 1` でチャプターメタデータをマッピング
- 環境変数からメタデータを取得:
  - `HALF_WIDTH_NAME` → `-metadata title=`
  - `HALF_WIDTH_DESCRIPTION` → `-metadata comment=` (Description セクション)
  - `HALF_WIDTH_EXTENDED` → `-metadata comment=` (Extended セクション)

### 追加 ffmpeg オプション

`--option` の値を空白でスプリットして引数に追加。

## テスト方針

- 引数構築: 正しい引数配列が生成されること
- AVS ファイル選択: `--target` 値に応じた正しい AVS ファイルが選択されること
- チャプター付与: `--addchapter` 時にメタデータ引数が追加されること
- コマンド実行は統合テストで外部バイナリをモックして検証

## 依存モジュール

- [settings.md](./settings.md) — `BinaryPaths.ffmpeg`, `OutputPaths`
- [pipeline.md](./pipeline.md) — `PipelineArgs` (target, add_chapter, ffmpeg_option)
- [chapter.md](./chapter.md) — チャプターファイル出力
- [chapter_exe.md](./chapter_exe.md) — 共通コマンド実行パターン
