# dtvmgr-vmaf: vmaf モジュール

> 親ドキュメント: [Architecture.md](./Architecture.md)

## 概要

ffmpeg の `libvmaf` フィルタを使用して、エンコード済みファイルとロスレスリファレンス間の VMAF スコアを計測する。

## 1080p アップスケール

VMAF の標準モデル `vmaf_v0.6.1` は 1080p 視聴条件で訓練されている。入力解像度に関わらずキャリブレーションされたスコアを得るため、歪み映像・リファレンスの両方を `1920x1080` に `bicubic` アップスケールしてから計測する。

```text
[0:v]scale=1920:1080:flags=bicubic[distorted];
[1:v]scale=1920:1080:flags=bicubic[reference];
[distorted][reference]libvmaf=model=version=vmaf_v0.6.1:n_subsample={n_subsample}
```

### 設計判断

- SD / 720p ソースでもモデルの想定条件に合わせることで、スコアの一貫性を確保する
- アップスケールは計測パイプライン内で完結し、中間ファイルを生成しない

## n_subsample

`n_subsample` パラメータにより、N フレームごとにスコアを計算する。デフォルト `5` の場合、3 秒サンプル(29.97fps で約 90 フレーム)では約 18 フレームがスコアリング対象となる。

- `1` に設定すると全フレームをスコアリング(精度最大、速度最遅)
- `5` はスピードと精度のバランス点(ab-av1 準拠)

## スコアパース

ffmpeg は VMAF スコアを stderr に以下の形式で出力する:

```text
[Parsed_libvmaf_0 @ 0x...] VMAF score: 94.500000
```

`"VMAF score: "` プレフィックスを行内検索し、後続の浮動小数点数を `f32` としてパースする。スコア行が見つからない場合はエラーを返す。

### エラーハンドリング

- ffmpeg プロセスのスポーン失敗
- 非ゼロ終了コード
- スコア行が stderr に含まれない場合

いずれも `anyhow::Context` 付きのエラーとして上位に伝搬する。
