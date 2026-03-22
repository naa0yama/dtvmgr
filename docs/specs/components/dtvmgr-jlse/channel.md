# Channel Detection

> 親ドキュメント: [Architecture.md](./Architecture.md)

## ステータス

- **Phase**: 1
- **実装状態**: 完了
- **Node.js ソース**: `src/channel.js` (130行)
- **Rust モジュール**: `crates/dtvmgr-jlse/src/channel.rs`

## 概要

TS ファイルのファイル名 (または指定チャンネル名) から放送局を検出する。`ChList.csv` のエントリに対して、NFKC 正規化と優先度ベースのマッチングを行う。

## 仕様

### CSV フォーマット: `ChList.csv`

```csv
放送局名（認識用）,放送局名（設定用）,略称,サービスID
ＮＨＫＢＳ１,,BS1,101
ＮＨＫＢＳプレミアム,,BSP,103
ＢＳ１１イレブン,,BS11,211
```

- 4 列固定: `recognize`, `install`, `short`, `service_id`
- 先頭 1 行はヘッダ (スキップ)
- `csv` クレートの `has_headers(true)` でパース

### 検出アルゴリズム

入力: ファイルパスのベースネーム + オプションのチャンネル名
前処理: NFKC 正規化 (`unicode-normalization` クレート) で全角英数を半角に統一

#### 1. チャンネル名指定時 (`--channel` or `CHNNELNAME` 環境変数)

チャンネル名が指定された場合、以下の順序で前方一致検索:

1. `recognize` (NFKC 正規化済み) で前方一致
2. `short` (NFKC 正規化済み) で前方一致
3. `service_id` で前方一致
4. チャンネル名から末尾以外の 1 桁孤立数字を除去して `recognize` で前方一致

一致しない場合はファイル名検索にフォールバック。

**孤立数字除去**: チャンネル名中の、前後が数字でなく末尾でもない 1 桁数字を削除する。
元実装の正規表現: `/(?<!\d)\d(?!\d|$)/g`

#### 2. PAT SID 逆引き (ファイル名検出のフォールバック前)

TS ファイルの PAT (Program Association Table, PID 0) から全 `service_id` を抽出し、`ChList.csv` の `service_id` 列と照合する。

- `dtvmgr-tsduck` の `extract_pat()` → `parse_pat_all_service_ids()` で SID リストを取得
- 複数 SID がある場合は順番に照合し、最初にマッチしたチャンネルを採用
- `tstables` が未インストール / PAT 抽出失敗時は `None` を返し、ファイル名検出にフォールバック (非致命的)

#### 3. ファイル名からの検出 (フォールバック)

**優先度 1** (即時 return):

| 対象         | パターン                                                             |
| ------------ | -------------------------------------------------------------------- |
| `recognize`  | `^{recognize}` or `_{recognize}`                                     |
| `short`      | `^{short}[_ ]` or `_{short}` or `{開き括弧}{short}{閉じ括弧/空白/_}` |
| `service_id` | `short` と同じパターンで `service_id` を使用                         |

**優先度 2** (候補記録、探索継続):

| 対象        | パターン                       |
| ----------- | ------------------------------ |
| `recognize` | `{開き括弧}{recognize}` が出現 |

**優先度 3** (より低い候補):

| 対象         | パターン                                   |
| ------------ | ------------------------------------------ |
| `short`      | `[ _]{short}{閉じ括弧/空白/_}` が出現      |
| `service_id` | `[ _]{service_id}{閉じ括弧/空白/_}` が出現 |

**優先度 4** (最低):

| 対象        | パターン                               |
| ----------- | -------------------------------------- |
| `recognize` | `_{recognize}` or `{recognize}` が出現 |

### 括弧文字セット

検出で使用される括弧文字:

- 開き括弧: `(`, `〔`, `[`, `{`, `〈`, `《`, `｢`, `『`, `【`, `≪`
- 閉じ括弧: `)`, `〕`, `]`, `}`, `〉`, `》`, `｣`, `』`, `】`, `≫`

## 型定義

```rust
/// Broadcast channel entry from `ChList.csv`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel {
    pub recognize: String,
    pub install: String,
    pub short: String,
    pub service_id: String,
}
```

## パブリック API

```rust
/// Loads channel entries from ChList.csv.
pub fn load_channels(csv_path: &Path) -> Result<Vec<Channel>>;

/// Detects the broadcast channel with PAT SID reverse lookup.
/// Priority: channel_name → PAT SID → filename pattern.
pub fn detect_channel_with_sid(
    channels: &[Channel],
    filepath: &str,
    channel_name: Option<&str>,
    pat_sids: Option<&[u32]>,
) -> Option<Channel>;

/// Detects the broadcast channel from a filename (without PAT SID).
/// Convenience wrapper around `detect_channel_with_sid(..., None)`.
pub fn detect_channel(
    channels: &[Channel],
    filepath: &str,
    channel_name: Option<&str>,
) -> Option<Channel>;

/// Looks up a channel by a single service ID.
pub fn lookup_channel_by_sid(channels: &[Channel], sid: u32) -> Option<Channel>;

/// Looks up a channel by multiple service IDs (first match wins).
pub fn lookup_channel_by_sids(channels: &[Channel], sids: &[u32]) -> Option<Channel>;
```

## テスト方針

- CSV パース: ヘッダスキップ、4 列の正確なマッピング
- 優先度別マッチング: 優先度 1〜4 の各パターンで正しいチャンネルが検出されること
- NFKC 正規化: 全角英数が半角に統一されてマッチすること
- 括弧内検出: 各種括弧文字内の `short` / `service_id` が検出されること
- チャンネル名指定: `--channel` 指定時に前方一致検索が正しく動作すること
- テストデータ: 少数のチャンネルエントリをインラインで定義

## 依存モジュール

なし (Phase 1 の基盤モジュール)
