# しょぼいカレンダー API クライアント (`SyoboiClient`)

> 親ドキュメント: [PLAN.md](../../PLAN.md) (Section 6)
>
> 関連ドキュメント:
>
> - [エンコード後ファイル名自動生成](../filename-generation.md)
> - [一括リネーム・EPGStation 連携](../batch-rename-epgstation.md)
> - [API データ収集・分析ツール設計](../../api-research/api-data-collection.md)
> - [API レスポンス実例](../../api-research/api-spec.md)
> - [db.php パラメータ仕様](../../external/syoboi/db.php.md)
> - [レート制限仕様](../../external/syoboi/rate_limit.md)
> - [ChID 一覧](../../external/syoboi/chid-list.md)

---

## 1. 概要

しょぼいカレンダー(しょぼかる)は、日本のアニメ放送スケジュールを管理するデータベースサービスである。
`SyoboiClient` は、しょぼかる `db.php` エンドポイントへの HTTP リクエストを担い、
タイトル情報・放送データ・チャンネル情報を取得する `recmgr-api` crate 内のコンポーネントである。

| 項目           | 値                                                                             |
| -------------- | ------------------------------------------------------------------------------ |
| Base URL       | `https://cal.syoboi.jp/db.php`                                                 |
| 認証           | 不要(公開 API)                                                                 |
| User-Agent     | カスタム必須(例: `recmgr/0.1.0 (+https://github.com/naa0yama/recmgr)`)         |
| レスポンス形式 | XML(`db.php`)                                                                  |
| 文字エンコード | UTF-8                                                                          |
| gzip           | `Accept-Encoding: gzip` をサポート。`reqwest` の `gzip` feature で透過的に処理 |

---

## 2. API エンドポイント

すべてのリクエストは `db.php` に対して `Command` パラメータで操作を指定する。

### 2.1 TitleLookup

タイトルマスタデータを取得する。

| パラメータ   | 必須 | 書式                      | 説明                                           |
| ------------ | ---- | ------------------------- | ---------------------------------------------- |
| `Command`    | Yes  | `TitleLookup`             | 固定値                                         |
| `TID`        | Yes  | 単一 / カンマ区切り / `*` | `TID=6309`, `TID=6309,6451`, `TID=*`(全件)     |
| `LastUpdate` | No   | `YYYYMMDD_HHMMSS-` 等     | 差分更新用。範囲指定可                         |
| `Fields`     | No   | カンマ区切り              | 出力フィールド限定(例: `Fields=TID,Title,Cat`) |

**レスポンス構造:**

```xml
<TitleLookupResponse>
    <Result>
        <Code>200</Code>
        <Message></Message>
    </Result>
    <TitleItems>
        <TitleItem id="6309">
            <TID>6309</TID>
            <LastUpdate>2022-06-30 01:56:20</LastUpdate>
            <Title>SPY×FAMILY</Title>
            <ShortTitle></ShortTitle>
            <TitleYomi>すぱいふぁみりー</TitleYomi>
            <TitleEN>SPY FAMILY</TitleEN>
            <Comment>...</Comment>
            <Cat>10</Cat>
            <TitleFlag>0</TitleFlag>
            <FirstYear>2022</FirstYear>
            <FirstMonth>4</FirstMonth>
            <FirstEndYear>2022</FirstEndYear>
            <FirstEndMonth>6</FirstEndMonth>
            <FirstCh>テレビ東京</FirstCh>
            <Keywords></Keywords>
            <UserPoint>113</UserPoint>
            <UserPointRank>903</UserPointRank>
            <SubTitles>*01*オペレーション〈梟(ストリクス)〉
*02*妻役を確保せよ
...</SubTitles>
        </TitleItem>
    </TitleItems>
</TitleLookupResponse>
```

### 2.2 ProgLookup

放送時間データを取得する。**1 リクエストあたり最大 5,000 件**。

| パラメータ   | 必須 | 書式                              | 説明                                                            |
| ------------ | ---- | --------------------------------- | --------------------------------------------------------------- |
| `Command`    | Yes  | `ProgLookup`                      | 固定値                                                          |
| `TID`        | No   | カンマ区切り                      | 未指定で全タイトル                                              |
| `ChID`       | No   | カンマ区切り / `*`                | 未指定で全チャンネル                                            |
| `Range`      | No   | `YYYYMMDD_HHMMSS-YYYYMMDD_HHMMSS` | 開始・終了必須。検索条件: `Range0 < EdTime AND StTime < Range1` |
| `StTime`     | No   | `YYYYMMDD_HHMMSS-` 等             | `LastUpdate` と同じ書式                                         |
| `Count`      | No   | カンマ区切り                      | 話数指定                                                        |
| `LastUpdate` | No   | `YYYYMMDD_HHMMSS-` 等             | 更新日時範囲                                                    |
| `JOIN`       | No   | `SubTitles`                       | `STSubTitle` フィールドを追加(**常に指定推奨**)                 |
| `Fields`     | No   | カンマ区切り                      | 出力フィールド限定                                              |
| `PID`        | No   | カンマ区切り                      | PID 直接指定                                                    |

> **注意:** `SubTitle` フィールドは空になることがある。`JOIN=SubTitles` を必ず追加し、
> `STSubTitle` を利用すること。サブタイトルテーブルはタイトルデータの `SubTitles` をパースして
> 生成されるため、サブタイトルが修正されても `ProgLookup` の `LastUpdate` は更新されない。

**レスポンス構造:**

```xml
<ProgLookupResponse>
    <ProgItems>
        <ProgItem id="574823">
            <LastUpdate>2022-03-10 01:24:53</LastUpdate>
            <PID>574823</PID>
            <TID>6309</TID>
            <StTime>2022-04-09 23:00:00</StTime>
            <StOffset>0</StOffset>
            <EdTime>2022-04-09 23:30:00</EdTime>
            <Count>1</Count>
            <SubTitle></SubTitle>
            <ProgComment></ProgComment>
            <Flag>2</Flag>
            <Deleted>0</Deleted>
            <Warn>1</Warn>
            <ChID>7</ChID>
            <Revision>0</Revision>
            <STSubTitle>オペレーション〈梟(ストリクス)〉</STSubTitle>
        </ProgItem>
        ...
    </ProgItems>
</ProgLookupResponse>
```

### 2.3 ChLookup

チャンネル(放送局)情報を取得する。

| パラメータ   | 必須 | 書式               | 説明             |
| ------------ | ---- | ------------------ | ---------------- |
| `Command`    | Yes  | `ChLookup`         | 固定値           |
| `ChID`       | No   | カンマ区切り / `*` | 未指定で全件     |
| `LastUpdate` | No   | 範囲指定           | 更新日時フィルタ |

**主な用途:** `channels.toml` の初期セットアップ時に全局一覧を取得する
(`filename-generation.md` Section 2.5 参照)。

---

## 3. レート制限

> 参照: [external/syoboi/rate_limit.md](../../external/syoboi/rate_limit.md)

| 制限種別             | 値                         | 備考                                                        |
| -------------------- | -------------------------- | ----------------------------------------------------------- |
| 通常リクエスト       | **1 リクエスト / 秒**      | `db`, `rss`, `rss2`, `json` 共通                            |
| 制限対象クライアント | 1 リクエスト / 10 秒       | User-Agent 未設定、同一データ高頻度取得、無駄なリクエスト等 |
| 時間制限             | **500 リクエスト / 時**    |                                                             |
| 日次制限             | **10,000 リクエスト / 日** |                                                             |

**レート制限適用時の挙動:**

- レスポンスが遅延する(サーバーサイドでスロットリング)
- クライアントがシングルスレッドの場合、遅延以上の影響はない
- レスポンスを待たずキャンセルして次のリクエストを送ると `429` が返される可能性がある

**制限対象にならないための要件:**

1. カスタム User-Agent を設定する(`curl/1.0.0` 等のデフォルトは不可)
2. 同じデータの重複リクエストを避ける
3. 1 リクエスト / 秒の間隔を遵守する
4. 時間・日次制限を超えない

---

## 4. `SyoboiClient` 構造体

### 4.1 フィールド定義

```rust
use std::sync::Arc;

use reqwest::Client;
use tokio::sync::Mutex;
use url::Url;

/// しょぼいカレンダー API クライアント
pub struct SyoboiClient {
    /// HTTP クライアント (reqwest、gzip 有効)
    http_client: Client,
    /// Base URL (`https://cal.syoboi.jp/db.php`)
    base_url: Url,
    /// レートリミッター
    rate_limiter: Arc<Mutex<SyoboiRateLimiter>>,
}
```

### 4.2 ビルダーパターン

```rust
pub struct SyoboiClientBuilder {
    base_url: Option<Url>,
    user_agent: Option<String>,
    min_interval: Option<Duration>,
    hourly_limit: Option<u32>,
    daily_limit: Option<u32>,
}

impl SyoboiClientBuilder {
    pub fn new() -> Self { /* ... */ }

    /// Base URL を上書きする (テスト時の wiremock URL 差し替え用)
    pub fn base_url(mut self, url: Url) -> Self { /* ... */ }

    /// User-Agent を設定する (必須)
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self { /* ... */ }

    /// 最小リクエスト間隔を設定する (デフォルト: 1 秒)
    pub fn min_interval(mut self, interval: Duration) -> Self { /* ... */ }

    /// 時間あたりのリクエスト上限 (デフォルト: 500)
    pub fn hourly_limit(mut self, limit: u32) -> Self { /* ... */ }

    /// 日あたりのリクエスト上限 (デフォルト: 10,000)
    pub fn daily_limit(mut self, limit: u32) -> Self { /* ... */ }

    /// クライアントをビルドする
    /// User-Agent 未設定の場合はエラーを返す
    pub fn build(self) -> Result<SyoboiClient> { /* ... */ }
}
```

**デフォルト値:**

| パラメータ     | デフォルト値                         |
| -------------- | ------------------------------------ |
| `base_url`     | `https://cal.syoboi.jp/db.php`       |
| `user_agent`   | なし(**必須、未設定でビルドエラー**) |
| `min_interval` | `Duration::from_secs(1)`             |
| `hourly_limit` | `500`                                |
| `daily_limit`  | `10_000`                             |

---

## 5. `SyoboiApi` トレイト

テスト時にモック差し替えを可能にするため、API 操作をトレイトとして抽象化する。

```rust
#[trait_variant::make(SyoboiApi: Send)]
pub trait LocalSyoboiApi {
    /// タイトル情報を取得する
    async fn lookup_titles(&self, tids: &[u32]) -> Result<Vec<SyoboiTitle>>;

    /// 放送データを取得する (5,000 件上限あり)
    async fn lookup_programs(&self, params: &ProgLookupParams) -> Result<Vec<SyoboiProgram>>;

    /// チャンネル情報を取得する
    async fn lookup_channels(&self, ch_ids: Option<&[u32]>) -> Result<Vec<SyoboiChannel>>;
}
```

`SyoboiClient` がこのトレイトを実装する。呼び出し元は `impl SyoboiApi` や
`dyn SyoboiApi` で依存し、テスト時はモック実装を注入できる。

### async トレイトの実装方針

`trait_variant` crate を使い、`Send` bound 付きの async トレイトを生成する。
これにより `tokio::spawn` 等からの呼び出しにも対応できる。

---

## 6. レスポンス型

XML レスポンスをデシリアライズした後の Rust 構造体。

### 6.1 `SyoboiTitle`

```rust
use serde::Deserialize;

/// TitleLookup レスポンスの 1 タイトル
#[derive(Debug, Clone, Deserialize)]
pub struct SyoboiTitle {
    /// タイトル ID
    #[serde(rename = "TID")]
    pub tid: u32,
    /// 最終更新日時 (例: "2022-06-30 01:56:20")
    #[serde(rename = "LastUpdate")]
    pub last_update: String,
    /// タイトル名
    #[serde(rename = "Title")]
    pub title: String,
    /// 短縮タイトル (空の場合あり)
    #[serde(rename = "ShortTitle")]
    pub short_title: Option<String>,
    /// タイトル読み (ひらがな)
    #[serde(rename = "TitleYomi")]
    pub title_yomi: Option<String>,
    /// 英語タイトル (空の場合あり)
    #[serde(rename = "TitleEN")]
    pub title_en: Option<String>,
    /// コメント (スタッフ・キャスト等の自由記述)
    #[serde(rename = "Comment")]
    pub comment: Option<String>,
    /// カテゴリ (10=アニメ 等)
    #[serde(rename = "Cat")]
    pub cat: Option<u32>,
    /// タイトルフラグ
    #[serde(rename = "TitleFlag")]
    pub title_flag: Option<u32>,
    /// 初回放送年
    #[serde(rename = "FirstYear")]
    pub first_year: Option<u32>,
    /// 初回放送月
    #[serde(rename = "FirstMonth")]
    pub first_month: Option<u32>,
    /// 最終放送年
    #[serde(rename = "FirstEndYear")]
    pub first_end_year: Option<u32>,
    /// 最終放送月
    #[serde(rename = "FirstEndMonth")]
    pub first_end_month: Option<u32>,
    /// 最速放送局
    #[serde(rename = "FirstCh")]
    pub first_ch: Option<String>,
    /// キーワード
    #[serde(rename = "Keywords")]
    pub keywords: Option<String>,
    /// ユーザーポイント
    #[serde(rename = "UserPoint")]
    pub user_point: Option<i32>,
    /// ユーザーポイントランク
    #[serde(rename = "UserPointRank")]
    pub user_point_rank: Option<u32>,
    /// サブタイトル一覧 (生テキスト: "*01*サブタイ\n*02*サブタイ" 形式)
    #[serde(rename = "SubTitles")]
    pub sub_titles: Option<String>,
}
```

### 6.2 `SyoboiProgram`

```rust
/// ProgLookup レスポンスの 1 番組
#[derive(Debug, Clone, Deserialize)]
pub struct SyoboiProgram {
    /// 番組 ID
    #[serde(rename = "PID")]
    pub pid: u32,
    /// タイトル ID
    #[serde(rename = "TID")]
    pub tid: u32,
    /// 放送開始日時 (例: "2022-04-09 23:00:00")
    #[serde(rename = "StTime")]
    pub st_time: String,
    /// 開始オフセット (秒)
    #[serde(rename = "StOffset")]
    pub st_offset: Option<i32>,
    /// 放送終了日時
    #[serde(rename = "EdTime")]
    pub ed_time: String,
    /// 話数 (0 = 特番/未設定)
    #[serde(rename = "Count")]
    pub count: Option<u32>,
    /// サブタイトル (空の場合あり、STSubTitle を優先すること)
    #[serde(rename = "SubTitle")]
    pub sub_title: Option<String>,
    /// 番組コメント
    #[serde(rename = "ProgComment")]
    pub prog_comment: Option<String>,
    /// フラグ (ビットマスク: 2=初回, 等)
    #[serde(rename = "Flag")]
    pub flag: Option<u32>,
    /// 削除フラグ
    #[serde(rename = "Deleted")]
    pub deleted: Option<u32>,
    /// 警告フラグ
    #[serde(rename = "Warn")]
    pub warn: Option<u32>,
    /// チャンネル ID
    #[serde(rename = "ChID")]
    pub ch_id: u32,
    /// リビジョン
    #[serde(rename = "Revision")]
    pub revision: Option<u32>,
    /// 最終更新日時
    #[serde(rename = "LastUpdate")]
    pub last_update: Option<String>,
    /// サブタイトル (SubTitles テーブル結合、JOIN=SubTitles 指定時のみ)
    #[serde(rename = "STSubTitle")]
    pub st_sub_title: Option<String>,
}
```

### 6.3 `SyoboiChannel`

```rust
/// ChLookup レスポンスの 1 チャンネル
#[derive(Debug, Clone, Deserialize)]
pub struct SyoboiChannel {
    /// チャンネル ID
    #[serde(rename = "ChID")]
    pub ch_id: u32,
    /// チャンネルグループ ID
    #[serde(rename = "ChGID")]
    pub ch_gid: Option<u32>,
    /// チャンネル名
    #[serde(rename = "ChName")]
    pub ch_name: String,
    /// チャンネルコメント
    #[serde(rename = "ChComment")]
    pub ch_comment: Option<String>,
    /// チャンネル URL
    #[serde(rename = "ChURL")]
    pub ch_url: Option<String>,
    /// 最終更新日時
    #[serde(rename = "LastUpdate")]
    pub last_update: Option<String>,
}
```

---

## 7. 検索パラメータ型

### 7.1 `ProgLookupParams`

```rust
use chrono::NaiveDateTime;

/// ProgLookup のリクエストパラメータ
#[derive(Debug, Clone)]
pub struct ProgLookupParams {
    /// タイトル ID フィルタ (None = 全タイトル)
    pub tids: Option<Vec<u32>>,
    /// チャンネル ID フィルタ (None = 全チャンネル)
    pub ch_ids: Option<Vec<u32>>,
    /// 時間範囲 (Range パラメータ)
    pub range: Option<TimeRange>,
    /// 開始時刻フィルタ (StTime パラメータ)
    pub st_time: Option<String>,
    /// 更新日時フィルタ
    pub last_update: Option<String>,
    /// SubTitles テーブル結合 (デフォルト: true)
    pub join_sub_titles: bool,
    /// 出力フィールド限定
    pub fields: Option<Vec<String>>,
}

impl Default for ProgLookupParams {
    fn default() -> Self {
        Self {
            tids: None,
            ch_ids: None,
            range: None,
            st_time: None,
            last_update: None,
            join_sub_titles: true, // STSubTitle 取得のため常に true 推奨
            fields: None,
        }
    }
}
```

### 7.2 `TimeRange`

```rust
/// ProgLookup の Range パラメータ
#[derive(Debug, Clone)]
pub struct TimeRange {
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
}

impl TimeRange {
    pub fn new(start: NaiveDateTime, end: NaiveDateTime) -> Self {
        Self { start, end }
    }

    /// しょぼかる Range 形式にフォーマットする
    /// 例: "20240101_000000-20240201_000000"
    pub fn to_syoboi_format(&self) -> String {
        format!(
            "{}-{}",
            self.start.format("%Y%m%d_%H%M%S"),
            self.end.format("%Y%m%d_%H%M%S"),
        )
    }
}
```

---

## 8. XML パース戦略

### 8.1 パース方式

`quick-xml` の `serde` feature を使い、XML レスポンスを直接 Rust 構造体にデシリアライズする。

```toml
# Cargo.toml
[dependencies]
quick-xml = { version = "0.37", features = ["serialize"] }
serde = { version = "1", features = ["derive"] }
```

### 8.2 レスポンスラッパー型

各 API レスポンスの XML ルート要素に対応するラッパー構造体を定義する。

```rust
/// TitleLookup レスポンス全体
#[derive(Debug, Deserialize)]
#[serde(rename = "TitleLookupResponse")]
struct TitleLookupResponse {
    #[serde(rename = "Result")]
    result: ApiResult,
    #[serde(rename = "TitleItems")]
    title_items: TitleItems,
}

#[derive(Debug, Deserialize)]
struct TitleItems {
    #[serde(rename = "TitleItem", default)]
    items: Vec<SyoboiTitle>,
}

/// ProgLookup レスポンス全体
#[derive(Debug, Deserialize)]
#[serde(rename = "ProgLookupResponse")]
struct ProgLookupResponse {
    #[serde(rename = "ProgItems")]
    prog_items: ProgItems,
}

#[derive(Debug, Deserialize)]
struct ProgItems {
    #[serde(rename = "ProgItem", default)]
    items: Vec<SyoboiProgram>,
}

/// API ステータス
#[derive(Debug, Deserialize)]
struct ApiResult {
    #[serde(rename = "Code")]
    code: u32,
    #[serde(rename = "Message")]
    message: Option<String>,
}
```

### 8.3 デシリアライズ処理

```rust
use quick_xml::de::from_str;

impl SyoboiClient {
    /// XML レスポンスをパースする
    fn parse_title_response(&self, xml: &str) -> Result<Vec<SyoboiTitle>> {
        let response: TitleLookupResponse = from_str(xml)
            .context("TitleLookup XML のパースに失敗")?;
        Ok(response.title_items.items)
    }

    fn parse_prog_response(&self, xml: &str) -> Result<Vec<SyoboiProgram>> {
        let response: ProgLookupResponse = from_str(xml)
            .context("ProgLookup XML のパースに失敗")?;
        Ok(response.prog_items.items)
    }
}
```

### 8.4 空要素の扱い

しょぼかるの XML では空要素が `<SubTitle></SubTitle>` として返される。
`serde` のデフォルトでは空文字列 `""` としてデシリアライズされるため、
`Option<String>` フィールドで空文字を `None` に変換するカスタムデシリアライザが必要。

```rust
use serde::Deserializer;

/// 空文字列を None に変換するデシリアライザ
fn deserialize_empty_string_as_none<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(deserializer)?;
    Ok(s.filter(|s| !s.is_empty()))
}
```

この関数を `#[serde(deserialize_with = "deserialize_empty_string_as_none")]` で
空になりうるフィールド(`SubTitle`, `ShortTitle`, `TitleEN`, `Keywords` 等)に適用する。

### 8.5 `SubTitles` フィールドのパース

`TitleLookup` の `SubTitles` フィールドは `*{話数}*{サブタイトル}` が改行区切りで格納される。

```
*01*オペレーション〈梟(ストリクス)〉
*02*妻役を確保せよ
*03*受験対策をせよ
```

パースユーティリティ:

```rust
/// SubTitles テキストから話数とサブタイトルのペアを抽出する
pub fn parse_sub_titles(raw: &str) -> Vec<(u32, String)> {
    let re = regex::Regex::new(r"\*(\d+)\*(.+)").expect("正規表現のコンパイルに失敗");
    raw.lines()
        .filter_map(|line| {
            let caps = re.captures(line.trim())?;
            let count: u32 = caps[1].parse().ok()?;
            let subtitle = caps[2].to_string();
            Some((count, subtitle))
        })
        .collect()
}
```

---

## 9. 月単位チャンク分割

### 9.1 背景

`ProgLookup` は 1 リクエストあたり **最大 5,000 件** しか返さない。
全チャンネル・広範囲の期間を指定すると上限に達する可能性がある
(参考: 2008 年は全体で 28,000 件、4 月だけで 2,335 件)。

### 9.2 分割戦略

期間ベースのリクエストは月単位にチャンクを分割し、5,000 件上限に収まるようにする。
加えて `ChID` パラメータで受信可能局に絞り込むことで、1 チャンクあたりの件数を大幅に削減する。

```rust
use chrono::{Datelike, Months, NaiveDate, NaiveDateTime, NaiveTime};

/// 期間を月単位チャンクに分割する
pub fn split_into_monthly_chunks(
    start: NaiveDateTime,
    end: NaiveDateTime,
) -> Vec<TimeRange> {
    let mut chunks = Vec::new();
    let mut current = start;

    while current < end {
        // 翌月 1 日 00:00:00
        let next_month_start = (current.date() + Months::new(1))
            .with_day(1)
            .unwrap_or(current.date());
        let next_month = NaiveDateTime::new(next_month_start, NaiveTime::MIN);
        let chunk_end = next_month.min(end);

        chunks.push(TimeRange::new(current, chunk_end));
        current = chunk_end;
    }

    chunks
}
```

### 9.3 Bulk リクエストフロー

`batch-rename-epgstation.md` Section 2.10.2 で規定される一括取得フローで使用する。

```rust
/// 期間内の全放送データを月単位で取得する
async fn fetch_programs_bulk(
    client: &impl SyoboiApi,
    start: NaiveDateTime,
    end: NaiveDateTime,
    ch_ids: &[u32],
) -> Result<Vec<SyoboiProgram>> {
    let chunks = split_into_monthly_chunks(start, end);
    let mut all_programs = Vec::new();

    for chunk in &chunks {
        let params = ProgLookupParams {
            ch_ids: Some(ch_ids.to_vec()),
            range: Some(chunk.clone()),
            join_sub_titles: true,
            ..Default::default()
        };
        let programs = client.lookup_programs(&params).await
            .with_context(|| format!(
                "ProgLookup 失敗: {} - {}",
                chunk.start, chunk.end
            ))?;

        tracing::info!(
            chunk_start = %chunk.start,
            chunk_end = %chunk.end,
            count = programs.len(),
            "ProgLookup チャンク取得完了"
        );

        all_programs.extend(programs);
    }

    Ok(all_programs)
}
```

**ChID 指定のメリット:**

- 全国 50+ 局 → ユーザーの受信可能局(関東圏なら 8 局程度)に絞り込み
- 1 チャンクあたりのデータ量を大幅削減
- `channels.toml` の ChID 設定を再利用

---

## 10. gzip 圧縮

`reqwest` の `gzip` feature を有効化し、透過的に圧縮レスポンスを処理する。

```toml
# Cargo.toml
[dependencies]
reqwest = { version = "0.12", features = ["gzip"] }
```

`reqwest` はデフォルトで `Accept-Encoding: gzip` ヘッダを送信し、
レスポンスの `Content-Encoding: gzip` を検出して自動的に展開する。

特に `TitleLookup` の `TID=*` (全件取得、4.5MB+)や広範囲の `ProgLookup` で
ネットワーク転送量を大幅に削減できる。

---

## 11. しょぼかる拡張レートリミッター

### 11.1 概要

しょぼかるの 3 階層レート制限(秒間・時間・日次)を遵守するための専用レートリミッター。
汎用的な秒間レートリミッターでは不十分であり、時間/日次カウンタを独自に管理する。

### 11.2 構造体

```rust
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// しょぼかる API 専用レートリミッター
pub struct SyoboiRateLimiter {
    /// 最小リクエスト間隔 (デフォルト: 1 秒)
    min_interval: Duration,
    /// 最後のリクエスト時刻
    last_request: Option<Instant>,
    /// 時間あたりの上限 (デフォルト: 500)
    hourly_limit: u32,
    /// 日あたりの上限 (デフォルト: 10,000)
    daily_limit: u32,
    /// 直近 1 時間のリクエストタイムスタンプ
    hourly_window: VecDeque<Instant>,
    /// 直近 1 日のリクエストタイムスタンプ
    daily_window: VecDeque<Instant>,
}
```

### 11.3 待機ロジック

```rust
impl SyoboiRateLimiter {
    /// 次のリクエストまで待機する
    /// 3 階層のレート制限をすべて満たすまで sleep する
    pub async fn wait(&mut self) {
        let now = Instant::now();

        // 1. 期限切れのタイムスタンプを除去
        self.cleanup_windows(now);

        // 2. 秒間制限: 前回リクエストから min_interval 以上経過するまで待機
        if let Some(last) = self.last_request {
            let elapsed = now.duration_since(last);
            if elapsed < self.min_interval {
                tokio::time::sleep(self.min_interval - elapsed).await;
            }
        }

        // 3. 時間制限: hourly_window が hourly_limit に達していたら、
        //    最古のタイムスタンプから 1 時間経過するまで待機
        if self.hourly_window.len() >= self.hourly_limit as usize {
            if let Some(&oldest) = self.hourly_window.front() {
                let wait_until = oldest + Duration::from_secs(3600);
                let now = Instant::now();
                if now < wait_until {
                    tracing::warn!(
                        remaining_secs = (wait_until - now).as_secs(),
                        "時間レート制限に到達。待機中..."
                    );
                    tokio::time::sleep(wait_until - now).await;
                }
            }
        }

        // 4. 日次制限: 同様のロジック (86400 秒)
        if self.daily_window.len() >= self.daily_limit as usize {
            if let Some(&oldest) = self.daily_window.front() {
                let wait_until = oldest + Duration::from_secs(86400);
                let now = Instant::now();
                if now < wait_until {
                    tracing::warn!(
                        remaining_secs = (wait_until - now).as_secs(),
                        "日次レート制限に到達。待機中..."
                    );
                    tokio::time::sleep(wait_until - now).await;
                }
            }
        }

        // 5. タイムスタンプを記録
        let now = Instant::now();
        self.last_request = Some(now);
        self.hourly_window.push_back(now);
        self.daily_window.push_back(now);
    }

    /// スライディングウィンドウから期限切れエントリを除去
    fn cleanup_windows(&mut self, now: Instant) {
        let hour_ago = now.checked_sub(Duration::from_secs(3600));
        let day_ago = now.checked_sub(Duration::from_secs(86400));

        if let Some(hour_ago) = hour_ago {
            while self.hourly_window.front().is_some_and(|&t| t < hour_ago) {
                self.hourly_window.pop_front();
            }
        }

        if let Some(day_ago) = day_ago {
            while self.daily_window.front().is_some_and(|&t| t < day_ago) {
                self.daily_window.pop_front();
            }
        }
    }
}
```

### 11.4 使用パターン

`SyoboiClient` の各 API メソッドはリクエスト送信前に必ず `rate_limiter.wait()` を呼び出す。

```rust
impl SyoboiApi for SyoboiClient {
    async fn lookup_titles(&self, tids: &[u32]) -> Result<Vec<SyoboiTitle>> {
        // レート制限待機
        self.rate_limiter.lock().await.wait().await;

        // HTTP リクエスト送信
        let response = self.http_client
            .get(self.base_url.clone())
            .query(&[
                ("Command", "TitleLookup"),
                ("TID", &tids.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(",")),
            ])
            .send()
            .await
            .context("TitleLookup リクエストの送信に失敗")?;

        let xml = response.text().await
            .context("TitleLookup レスポンスの読み取りに失敗")?;

        self.parse_title_response(&xml)
    }
}
```

---

## 12. テスト

### 12.1 XML パース単体テスト

実 API レスポンス(`api-spec.md` 参照)をフィクスチャとして保存し、パースの正確性を検証する。

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn test_parse_title_lookup_response() {
        // Arrange
        let xml = include_str!("../../fixtures/syoboi/title_lookup_6309.xml");

        // Act
        let client = SyoboiClient::builder()
            .user_agent("test/0.0.0")
            .build()
            .unwrap();
        let titles = client.parse_title_response(xml).unwrap();

        // Assert
        assert_eq!(titles.len(), 1);
        assert_eq!(titles[0].tid, 6309);
        assert_eq!(titles[0].title, "SPY×FAMILY");
        assert_eq!(titles[0].title_en.as_deref(), Some("SPY FAMILY"));
        assert_eq!(titles[0].first_year, Some(2022));
        assert_eq!(titles[0].first_month, Some(4));
        assert!(titles[0].sub_titles.as_ref().unwrap().contains("*01*"));
    }

    #[test]
    fn test_parse_prog_lookup_response() {
        // Arrange
        let xml = include_str!("../../fixtures/syoboi/prog_lookup_6309.xml");

        // Act
        let client = SyoboiClient::builder()
            .user_agent("test/0.0.0")
            .build()
            .unwrap();
        let programs = client.parse_prog_response(xml).unwrap();

        // Assert
        assert!(!programs.is_empty());
        let first = &programs[0];
        assert_eq!(first.tid, 6309);
        assert_eq!(first.ch_id, 7);
        assert_eq!(first.count, Some(1));
        assert_eq!(
            first.st_sub_title.as_deref(),
            Some("オペレーション〈梟(ストリクス)〉")
        );
    }

    #[test]
    fn test_parse_sub_titles() {
        // Arrange
        let raw = "*01*オペレーション〈梟(ストリクス)〉\n*02*妻役を確保せよ\n*03*受験対策をせよ";

        // Act
        let parsed = parse_sub_titles(raw);

        // Assert
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0], (1, "オペレーション〈梟(ストリクス)〉".to_string()));
        assert_eq!(parsed[1], (2, "妻役を確保せよ".to_string()));
        assert_eq!(parsed[2], (3, "受験対策をせよ".to_string()));
    }

    #[test]
    fn test_parse_empty_sub_title_as_none() {
        // Arrange: SubTitle が空の XML
        let xml = r#"
        <ProgLookupResponse>
            <ProgItems>
                <ProgItem id="574823">
                    <PID>574823</PID>
                    <TID>6309</TID>
                    <StTime>2022-04-09 23:00:00</StTime>
                    <EdTime>2022-04-09 23:30:00</EdTime>
                    <Count>1</Count>
                    <SubTitle></SubTitle>
                    <ChID>7</ChID>
                    <STSubTitle>オペレーション〈梟(ストリクス)〉</STSubTitle>
                </ProgItem>
            </ProgItems>
        </ProgLookupResponse>
        "#;

        // Act
        let client = SyoboiClient::builder()
            .user_agent("test/0.0.0")
            .build()
            .unwrap();
        let programs = client.parse_prog_response(xml).unwrap();

        // Assert
        assert_eq!(programs[0].sub_title, None); // 空文字 → None
        assert_eq!(
            programs[0].st_sub_title.as_deref(),
            Some("オペレーション〈梟(ストリクス)〉")
        );
    }

    #[test]
    fn test_time_range_format() {
        // Arrange
        let start = NaiveDate::from_ymd_opt(2024, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        let end = NaiveDate::from_ymd_opt(2024, 2, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();

        // Act
        let range = TimeRange::new(start, end);

        // Assert
        assert_eq!(range.to_syoboi_format(), "20240101_000000-20240201_000000");
    }
}
```

### 12.2 `wiremock` 結合テスト

`wiremock` で HTTP サーバーをモックし、レート制限・リトライ・エラーハンドリングを検証する。

```rust
#[cfg(test)]
mod integration_tests {
    #![allow(clippy::unwrap_used)]

    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_title_lookup_via_http() {
        // Arrange
        let mock_server = MockServer::start().await;
        let xml_body = include_str!("../../fixtures/syoboi/title_lookup_6309.xml");

        Mock::given(method("GET"))
            .and(path("/db.php"))
            .and(query_param("Command", "TitleLookup"))
            .and(query_param("TID", "6309"))
            .respond_with(ResponseTemplate::new(200).set_body_string(xml_body))
            .mount(&mock_server)
            .await;

        let client = SyoboiClient::builder()
            .base_url(mock_server.uri().parse().unwrap())
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(0)) // テスト時はレート制限無効化
            .build()
            .unwrap();

        // Act
        let titles = client.lookup_titles(&[6309]).await.unwrap();

        // Assert
        assert_eq!(titles.len(), 1);
        assert_eq!(titles[0].title, "SPY×FAMILY");
    }

    #[tokio::test]
    async fn test_rate_limiter_enforces_interval() {
        // Arrange
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<TitleLookupResponse><Result><Code>200</Code></Result><TitleItems></TitleItems></TitleLookupResponse>"))
            .expect(2)
            .mount(&mock_server)
            .await;

        let client = SyoboiClient::builder()
            .base_url(mock_server.uri().parse().unwrap())
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(100))
            .build()
            .unwrap();

        // Act
        let start = Instant::now();
        client.lookup_titles(&[1]).await.unwrap();
        client.lookup_titles(&[2]).await.unwrap();
        let elapsed = start.elapsed();

        // Assert: 2 リクエスト間に最低 100ms の間隔が確保されること
        assert!(elapsed >= Duration::from_millis(100));
    }

    #[tokio::test]
    async fn test_user_agent_is_sent() {
        // Arrange
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(wiremock::matchers::header("User-Agent", "recmgr/0.1.0"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<TitleLookupResponse><Result><Code>200</Code></Result><TitleItems></TitleItems></TitleLookupResponse>"))
            .expect(1)
            .mount(&mock_server)
            .await;

        let client = SyoboiClient::builder()
            .base_url(mock_server.uri().parse().unwrap())
            .user_agent("recmgr/0.1.0")
            .min_interval(Duration::from_millis(0))
            .build()
            .unwrap();

        // Act & Assert (Mock の expect(1) で User-Agent ヘッダの送信を検証)
        client.lookup_titles(&[1]).await.unwrap();
    }
}
```

### 12.3 テストフィクスチャ構成

```
fixtures/
└── syoboi/
    ├── title_lookup_6309.xml    # SPY×FAMILY TitleLookup レスポンス
    ├── title_lookup_6451.xml    # SPY×FAMILY(第2クール) TitleLookup レスポンス
    ├── prog_lookup_6309.xml     # SPY×FAMILY ProgLookup レスポンス
    ├── prog_lookup_range.xml    # 期間指定 ProgLookup レスポンス (複数タイトル混在)
    ├── ch_lookup_all.xml        # ChLookup 全件レスポンス
    └── empty_response.xml       # 結果 0 件のレスポンス
```

フィクスチャは `api-spec.md` に記録された実レスポンスから作成する。

---

## 13. 依存関係

| Crate           | バージョン | 用途                                            |
| --------------- | ---------- | ----------------------------------------------- |
| `reqwest`       | 0.12       | HTTP クライアント。`gzip` feature で圧縮対応    |
| `quick-xml`     | 0.37       | XML パーサ。`serialize` feature で serde 連携   |
| `serde`         | 1          | シリアライズ / デシリアライズ。`derive` feature |
| `tokio`         | 1          | 非同期ランタイム。`time` feature (sleep 用)     |
| `chrono`        | 0.4        | 日時操作。`TimeRange` のフォーマットに使用      |
| `url`           | 2          | URL 型。Base URL の型安全な管理                 |
| `regex`         | 1          | `SubTitles` フィールドのパースに使用            |
| `tracing`       | 0.1        | 構造化ログ出力                                  |
| `anyhow`        | 1          | アプリケーションレベルのエラーハンドリング      |
| `trait-variant` | 0.1        | `Send` bound 付き async トレイト生成            |
| `wiremock`      | 0.6        | (dev) HTTP モックサーバー                       |

---

## 14. 検討事項・未決定事項

- [ ] `quick-xml` の `serde` デシリアライズで `<TitleItem id="6309">` の `id` 属性をどう扱うか(`#[serde(rename = "@id")]` vs 無視)
- [ ] `SyoboiRateLimiter` のスライディングウィンドウを `VecDeque<Instant>` で管理するとメモリ効率は十分か(日次 10,000 件で約 160KB、問題なしと想定)
- [ ] ProgLookup で 5,000 件ちょうど返された場合の検知方法(件数チェックで警告ログを出力し、期間をさらに分割するか)
- [ ] `TitleLookup` の `TID=*` (全件取得)時のストリーミングパース対応(4.5MB+ の XML を一括メモリ展開するか、`quick-xml` の `Reader` で逐次処理するか)
- [ ] `ChLookup` レスポンスのキャッシュ戦略(チャンネル情報は変更頻度が低いため、ローカルファイルキャッシュで十分か)
- [ ] HTTP リトライ戦略(`reqwest-retry` crate の導入 vs 自前実装。429 レスポンス時の指数バックオフ)
- [ ] `SyoboiClient` をスレッドセーフにするための `Arc<Mutex<SyoboiRateLimiter>>` のオーバーヘッド(単一タスクからの順次呼び出しが主用途であれば `Rc<RefCell<...>>` でも十分か)
- [ ] crate 共通エラー型(`ApiError`)との統合方針(別ファイル `error.md` で後日策定)
