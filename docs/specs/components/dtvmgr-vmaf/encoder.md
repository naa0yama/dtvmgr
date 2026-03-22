# dtvmgr-vmaf: encoder モジュール

> 親ドキュメント: [Architecture.md](./Architecture.md)

## 概要

エンコーダごとのプリセット定義と、探索アルゴリズムが使用する整数品質空間(`q` 空間)への変換機構を提供する。

## EncoderConfig プリセット

ベンチマーク実測値に基づき、各エンコーダの探索範囲と初期ヒント値を設定する。`quality_hint` は VMAF 93 の典型的な到達点(実写とアニメの中間値)に設定し、探索の初回イテレーションを最適化する。

| プリセット    | コーデック  | パラメータ        | 範囲  | ヒント | 特記                         |
| ------------- | ----------- | ----------------- | ----- | ------ | ---------------------------- |
| `av1_qsv()`   | `av1_qsv`   | `-global_quality` | 18-35 | 25     | QSV look-ahead + extbrc      |
| `libsvtav1()` | `libsvtav1` | `-crf`            | 25-45 | 35     | preset 8, `yuv420p10le`      |
| `h264_qsv()`  | `h264_qsv`  | `-global_quality` | 20-32 | 27     | QSV look-ahead + extbrc      |
| `hevc_qsv()`  | `hevc_qsv`  | `-global_quality` | 18-28 | 23     | QSV look-ahead + extbrc      |
| `libx264()`   | `libx264`   | `-crf`            | 20-30 | 25     | preset medium, `yuv420p`     |
| `libx265()`   | `libx265`   | `-crf`            | 20-32 | 25     | preset medium, `yuv420p10le` |

## QualityParam enum

ffmpeg の品質パラメータフラグを抽象化する。`flag()` メソッドで対応するコマンドライン引数文字列を返す。

| バリアント      | フラグ            | 用途                        |
| --------------- | ----------------- | --------------------------- |
| `Crf`           | `-crf`            | libsvtav1, libx264, libx265 |
| `GlobalQuality` | `-global_quality` | av1_qsv, hevc_qsv, h264_qsv |
| `Qp`            | `-qp`             | librav1e, *_vulkan          |
| `Q`             | `-q`              | *_vaapi, mpeg2video         |
| `Cq`            | `-cq`             | *_nvenc                     |

## QualityConverter(q 空間抽象)

二分探索は浮動小数点の比較問題を避けるため、整数 `q` 空間で動作する。`QualityConverter` が品質値(CRF / ICQ)と `q` 値の相互変換を担う。

### 設計判断

- **低い `q` = 高品質** に正規化。`high_value_means_hq` が `true` のエンコーダでは `q` を符号反転して統一する
- `increment` (ステップサイズ)で割って整数化。現状は全プリセットが `1.0` ステップ
- `min_max_q()` は `high_value_means_hq` を考慮して `min_q < max_q` の順序を保証する

## vmaf_lerp_q 補間

観測済みの 2 点(目標 VMAF を挟む上下の `SearchSample`)から、目標 VMAF を達成する `q` 値を線形補間で推定する。

### 保証

- 結果は `[better.q + 1, worse.q - 1]` にクランプされ、少なくとも 1 ステップの進行を保証する
- 隣接ケース(`q_diff <= 1`)では `better.q` を直接返す(無限ループ防止)
