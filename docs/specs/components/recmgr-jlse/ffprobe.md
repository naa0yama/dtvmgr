# ffprobe Command Wrapper

> 親ドキュメント: [Architecture.md](./Architecture.md)

## ステータス

- **Phase**: 2
- **実装状態**: 未実装
- **Node.js ソース**: `src/command/ffprobe.js` (43行)
- **Rust モジュール**: `crates/dtvmgr-jlse/src/command/ffprobe.rs`

## 概要

映像・音声のメタ情報 (フレームレート、サンプルレート) を `ffprobe` コマンドで取得するラッパー。FFmpeg フィルタ生成やチャプター生成で使用される。

## 仕様

### 共通引数パターン

```
ffprobe -hide_banner -loglevel error -select_streams <stream>
        -show_entries <entries> -of default=noprint_wrappers=1:nokey=1 <filename>
```

### `getFrameRate`: フレームレート取得

- `stream`: `v:0`, `entries`: `stream=avg_frame_rate`
- 出力: `30000/1001` → `{ fpsNumerator: "30000", fpsDenominator: "1001" }`

### `getSampleRate`: サンプルレート取得

- `stream`: `a:0`, `entries`: `stream=sample_rate`
- 出力: `48000`

## 型定義

```rust
/// Video metadata from ffprobe.
pub struct VideoMetadata {
    /// Frame rate numerator (e.g. 30000).
    pub fps_numerator: u32,
    /// Frame rate denominator (e.g. 1001).
    pub fps_denominator: u32,
    /// Audio sample rate (e.g. 48000).
    pub sample_rate: Option<u32>,
}
```

## テスト方針

- 引数構築: 正しい引数配列が生成されること
- 出力パース: `30000/1001` 形式の文字列が正しく `fps_numerator` / `fps_denominator` に分解されること
- コマンド実行は統合テストで外部バイナリをモックして検証

## 依存モジュール

- [settings.md](./settings.md) — `BinaryPaths.ffprobe`
- [chapter_exe.md](./chapter_exe.md) — 共通コマンド実行パターン
