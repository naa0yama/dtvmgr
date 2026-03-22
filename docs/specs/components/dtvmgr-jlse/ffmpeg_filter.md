# FFmpeg Filter Generation

> 親ドキュメント: [Architecture.md](./Architecture.md)

## ステータス

- **Phase**: 4
- **実装状態**: 未実装
- **Node.js ソース**: `src/output/ffmpeg_filter.js` (46行)
- **Rust モジュール**: `crates/dtvmgr-jlse/src/output/ffmpeg_filter.rs`

## 概要

`obs_cut.avs` の Trim 情報から ffmpeg の `filter_complex` 文字列を生成する。フレーム番号をフレームレートで割って時間に変換し、`trim`/`atrim`/`setpts`/`asetpts`/`concat` フィルタを構築する。

## 仕様

### 処理手順

1. `obs_cut.avs` から `Trim(start,end)` をパース
2. 開始フレームが `MIN_START_FRAME` (30) 未満の場合は `30` にクランプ
3. `ffprobe` でフレームレート (分子/分母) を取得
4. 各 Trim セグメントの開始・終了時間を計算:
   - `startTime = start * fpsDenominator / fpsNumerator`
   - `endTime = end * fpsDenominator / fpsNumerator`
5. `trim`/`atrim`/`setpts`/`asetpts` フィルタ文字列を生成
6. `concat` フィルタで結合

### `Trim()` コマンドフォーマット

```
Trim(100,500)Trim(800,1200)
```

- 1 行に複数の `Trim()` コマンドが連結される
- `Trim(start,end)`: start, end はフレーム番号 (0-indexed)

### 出力例

```
[0:v]trim=1.001:16.683,setpts=PTS-STARTPTS[v0];[0:a]atrim=1.001:16.683,asetpts=PTS-STARTPTS[a0];[0:v]trim=26.693:38.372,setpts=PTS-STARTPTS[v1];[0:a]atrim=26.693:38.372,asetpts=PTS-STARTPTS[a1];[v0][a0][v1][a1]concat=n=2:v=1:a=1[video][audio];
```

### MIN_START_FRAME

開始フレームが `30` 未満の場合は `30` にクランプする。これは先頭付近のフレームがデコード不安定になる問題を回避するための安全マージン。

## テスト方針

- filter 文字列の構文正確性: 生成された filter_complex 文字列が ffmpeg のフォーマットに準拠すること
- フレーム→時間変換: `start * fpsDenominator / fpsNumerator` の計算精度
- `MIN_START_FRAME` クランプ: 30 未満のフレームが 30 にクランプされること
- 複数セグメント: 複数 Trim がある場合に正しい `concat=n=N` が生成されること

## 依存モジュール

- [settings.md](./settings.md) — `OutputPaths.output_avs_cut`, `OutputPaths.output_filter_cut`
- [ffprobe.md](./ffprobe.md) — `VideoMetadata` (フレームレート取得)
- [chapter.md](./chapter.md) — `TrimSegment` 型 (Trim パース共有)
