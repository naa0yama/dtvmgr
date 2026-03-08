# Pre-Encode Duration Validation

> 親ドキュメント: [Architecture.md](./Architecture.md)

## ステータス

- **Phase**: 3
- **実装状態**: 実装済み
- **Rust モジュール**: `crates/dtvmgr-jlse/src/validate.rs`

## 概要

エンコード前に、元の TS ファイルと CM カット後の AVS ファイルの尺を比較し、カットエラーを検出する。比率がしきい値を下回る場合、エンコードを中断してユーザに通知する。

## 背景

CM 検出パイプラインでは、`join_logo_scp` がロゴ検出・無音検出を組み合わせて CM 区間を特定する。しかし、ロゴデータの不一致や放送波の異常により、コンテンツ部分まで誤ってカットされるケースがある。エンコード後に尺の異常に気づいた場合、復旧には再エンコードが必要となる。

このバリデーションにより、エンコード前の段階で異常を検出し、不要なエンコード処理を回避する。

## 仕様

### バリデーションフロー

```
TS ファイル (元の尺)
    ↓ ffprobe
ts_duration_secs
    ↓
AVS ファイル (CM カット後の尺)
    ↓ ffprobe
avs_duration_secs
    ↓
ratio = avs / ts × 100 (floor)
    ↓
ルール照合 → OK / エラー
```

### デフォルトルール

番組の尺 (分) に基づき、CM カット後の最低比率を定義する。

| 番組尺 (分) | 最低比率 | 根拠                                          |
| ----------- | -------- | --------------------------------------------- |
| 0-10        | 68%      | 短尺番組は CM 比率が高い (5分番組の CM 約30%) |
| 11-49       | 75%      | 30分番組の CM は約25% (22分本編 / 30分枠)     |
| 50-90       | 70%      | 1時間番組は特番等で CM 比率が変動しやすい     |
| 91-9999     | 70%      | 長時間番組 (映画特番等) も同様に変動幅を許容  |

### 判定ロジック

1. TS の尺が 0 以下 → エラー
2. `ratio_percent = floor(avs / ts × 100)`
3. `ts_minutes = round(ts / 60)`
4. ルールを先頭から順に照合 (`min_min <= ts_minutes <= max_min`)
5. マッチしたルール: `ratio_percent <= min_percent` → エラー (しきい値ちょうども不合格)
6. マッチしたルール: `ratio_percent > min_percent` → OK
7. どのルールにもマッチしない → OK (チェックをパス)

### 設定ファイル

`[[jlse.encode.duration_check]]` でカスタムルールを定義できる。省略時はデフォルトルールが適用される。

```toml
[[jlse.encode.duration_check]]
min_min = 0
max_min = 10
min_percent = 68

[[jlse.encode.duration_check]]
min_min = 11
max_min = 49
min_percent = 75
```

### CLI フラグ

`--skip-duration-check` でバリデーションをスキップできる。スキップした場合でも、進捗表示に AVS の尺が必要な場合は ffprobe が個別に呼び出される。

## 型定義

```rust
/// Duration check rule for pre-encode validation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DurationCheckRule {
    /// Lower bound of program duration range in minutes (inclusive).
    pub min_min: u32,
    /// Upper bound of program duration range in minutes (inclusive).
    pub max_min: u32,
    /// Minimum acceptable content percent (e.g. 70 = 70%).
    pub min_percent: u32,
}
```

## パイプライン統合

`pipeline.rs` の `run_pipeline` 関数内で、エンコードステップの直前に実行される。

- `check_pre_encode_duration` は ffprobe で取得した AVS の尺を戻り値として返す
- この値は後続の ffmpeg 進捗表示で再利用される (ffprobe の二重呼び出しを回避)

```
... → EIT 抽出 → [尺チェック] → ffmpeg エンコード → ...
```

## テスト方針

- 各番組尺カテゴリ (短尺/中尺/長尺/超長尺) で合格・不合格のテスト
- 境界値テスト (68% ちょうど → 不合格、69% → 合格)
- エッジケース (尺 0、負の値、空のルール、ルールにギャップがある場合)
- カスタムルールの適用テスト

## 依存モジュール

- `command::ffprobe` -- 尺の取得 (`get_duration`)
- `types::DurationCheckRule` -- ルール定義
