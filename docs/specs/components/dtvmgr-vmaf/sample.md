# dtvmgr-vmaf: sample モジュール

> 親ドキュメント: [Architecture.md](./Architecture.md)

## 概要

入力 TS ファイルからサンプルを抽出し、VMAF 比較用のロスレス FFV1 リファレンスを生成する。ab-av1 と同じギャップベースの均等分散アルゴリズムでサンプル位置を決定する。

## compute_sample_positions アルゴリズム

### 入力

- `ContentSegment` のリスト(CM カット後の本編区間、秒単位)
- `SampleConfig`(サンプリングパラメータ)

### 処理

1. 全コンテンツ区間の合計時間を算出
2. `skip_secs` を先頭・末尾から除外して有効範囲を決定
3. `effective_duration / sample_every_secs` の `ceil` でサンプル数を算出し、`[min_samples, max_samples]` にクランプ
4. ギャップベースの均等分散でサンプル位置を配置:
   - `gap = (effective_duration - sample_duration * count) / (count + 1)`
   - `position[n] = effective_start + gap * (n+1) + sample_duration * n`

### 設計判断

- `skip_secs` はオープニング / エンディングの定型映像を避けるため(デフォルト `120` 秒)
- 有効範囲が 0 以下の場合は空のベクタを返す(短尺コンテンツの安全処理)
- `gap` が負になる場合は `0.0` にクランプ(サンプルが重なることを許容)

## content_offset_to_absolute マッピング

コンテンツ相対オフセット(全本編区間の先頭からの秒数)を、TS ファイル内の絶対タイムスタンプに変換する。複数セグメントを順に走査し、オフセットが含まれるセグメントの `start_secs` に残余を加算する。

オフセットが全セグメントを超過した場合は、最終セグメントの `end_secs` を返す(安全なフォールバック)。

## extract_samples フロー

各サンプル位置に対して 2 段階の ffmpeg 呼び出しを実行する:

1. **ストリームコピー抽出**: `ffmpeg -ss {start} -t {duration} -i {input} -c:v copy -an {output.ts}`
   - 再エンコードなしで高速抽出
   - 音声・字幕は除外(`-an -sn`)
2. **FFV1 リファレンス生成**: `ffmpeg -y {extra_input_args...} -i {sample.ts} -vf {reference_filter} -c:v ffv1 -an {output.mkv}`
   - `extra_input_args`(`-init_hw_device`, `-filter_hw_device` 等の HW デバイス初期化引数)は `-i` の前に挿入される
   - `reference_filter` は `SearchConfig::reference_filter` が `Some` の場合はその値、`None` の場合は `video_filter` にフォールバックする
   - HW フィルタ(QSV VPP 等)使用時は、VPP が出力する HW サーフェスフレームを `hwdownload` でシステムメモリに転送してから CPU のみの FFV1 エンコーダに渡す必要がある。ピクセルフォーマットは ffmpeg が HW サーフェスから自動ネゴシエーションする
   - ビデオフィルタチェーン(デインタレース、スケール等)を適用してからロスレスエンコード
   - VMAF 計測時の「理想的な出力」として使用

## SampleConfig パラメータ

| パラメータ          | デフォルト | 説明                                              |
| ------------------- | ---------- | ------------------------------------------------- |
| `duration_secs`     | `3.0`      | 各サンプルの長さ(秒)                              |
| `skip_secs`         | `120.0`    | 先頭・末尾からスキップする秒数                    |
| `sample_every_secs` | `720.0`    | コンテンツ N 秒ごとに 1 サンプル(12 分)           |
| `min_samples`       | `5`        | 最小サンプル数                                    |
| `max_samples`       | `15`       | 最大サンプル数                                    |
| `vmaf_subsample`    | `5`        | VMAF の `n_subsample`(N フレームごとにスコア計算) |
