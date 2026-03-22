# Parameter Detection

> 親ドキュメント: [Architecture.md](./Architecture.md)

## ステータス

- **Phase**: 1
- **実装状態**: 完了
- **Node.js ソース**: `src/param.js` (96行)
- **Rust モジュール**: `crates/dtvmgr-jlse/src/param.rs`

## 概要

チャンネル情報とファイル名から JL パラメータ (JL コマンドファイル名、フラグ、オプション) を検出する。`ChParamJL1.csv` と `ChParamJL2.csv` を順にマージして最終パラメータを決定する。

## 仕様

### CSV フォーマット: `ChParamJL1.csv` / `ChParamJL2.csv`

```csv
放送局略称,タイトル,JL_RUN,FLAGS,OPTIONS,#コメント表示用,#コメント
,,JL_フラグ指定.txt,@,@,,デフォルト設定。先頭@マークは設定クリア
NHK-G,,JL_NHK.txt,,,NHK用前後（他番組宣伝）カット,
```

- 7 列固定: `channel`, `title`, `jl_run`, `flags`, `options`, `comment_view`, `comment`
- 先頭 1 行はヘッダ (スキップ)
- `#` で始まる `channel` 値はコメント行としてスキップ

### 検索アルゴリズム

入力: `Vec<Param>` (JL1), `Vec<Param>` (JL2), `Option<Channel>`, ファイル名

1. 検索キーは `channel.short` (チャンネル未検出時は `"__normal"`)
2. CSV の各行について:
   - `channel` フィールドが `#` で始まる → スキップ
   - `channel` が検索キーと一致するか確認
   - 一致し、かつ `title` が指定されている場合:
     - `title` に正規表現メタ文字 (`.*+?|[]^`) が含まれる → 正規表現マッチ
     - それ以外 → 部分文字列マッチ (NFKC 正規化済み)
   - 一致し、`title` が空 → 無条件マッチ
3. マッチした行のフィールドをマージ:
   - 値が `"@"` → そのフィールドを空文字にクリア
   - 値が空 → 既存値を維持 (上書きしない)
   - 値が非空 → 上書き
4. どの行にもマッチしない → 最初の非コメント行 (デフォルト行) の値を使用
5. JL1 の結果に JL2 の結果をマージ (`Object.assign` 相当: JL2 が JL1 を上書き)

### `@` マーカーの動作

`flags` や `options` が `"@"` の場合、そのフィールドの値を空文字列にリセットする。これは前の CSV で設定された値を明示的にクリアするために使用される。

### JL1 + JL2 のマージロジック

```
JL1 検索 → result1 (HashMap<String, String>)
JL2 検索 → result2 (HashMap<String, String>)
最終結果 = result1 に result2 を上書きマージ
  "@" 値は空文字に変換
```

## 型定義

```rust
/// Raw parameter entry from `ChParamJL*.csv`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub channel: String,
    pub title: String,
    pub jl_run: String,
    pub flags: String,
    pub options: String,
    pub comment_view: String,
    pub comment: String,
}

/// Merged detection result from channel + filename matching.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DetectionParam {
    pub jl_run: String,
    pub flags: String,
    pub options: String,
}
```

## パブリック API

```rust
/// Loads parameter entries from a ChParamJL CSV file.
pub fn load_params(csv_path: &Path) -> Result<Vec<Param>>;

/// Detects JL parameters by matching channel and filename.
/// Searches JL1 first, then JL2, merging results.
pub fn detect_param(
    params_jl1: &[Param],
    params_jl2: &[Param],
    channel: Option<&Channel>,
    filename: &str,
) -> DetectionParam;
```

## テスト方針

- CSV パース: ヘッダスキップ、7 列の正確なマッピング
- チャンネル一致: `short` による検索キーマッチング
- タイトル正規表現マッチ: メタ文字を含む `title` で正規表現マッチが動作すること
- タイトル部分一致: メタ文字を含まない `title` で部分文字列マッチが動作すること
- `@` クリア: `"@"` 値が既存フィールドを空文字にリセットすること
- JL1 + JL2 マージ: JL2 の値が JL1 を上書きすること
- テストデータ: デフォルト行 + NHK + WOWOW 等の代表的なエントリをインラインで定義

## 依存モジュール

- [channel.md](./channel.md) — `Channel` 型を検索キーに使用
