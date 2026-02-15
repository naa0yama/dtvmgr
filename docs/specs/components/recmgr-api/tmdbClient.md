# TMDB API クライアント (`TmdbClient`)

> 親ドキュメント: [PLAN.md](../../PLAN.md)
>
> 関連ドキュメント:
>
> - [TMDB エピソード照合ロジック](../tmdb-episode-matching.md)
> - [リネームパイプライン設計](../rename-pipeline.md)
> - [TMDB API ドキュメント](../../external/themoviedb/README.md)
> - [TMDB 認証](../../external/themoviedb/authentication-application.md)
> - [TMDB エラーコード](../../external/themoviedb/errors.md)
> - [TMDB レート制限](../../external/themoviedb/rate-limiting.md)

---

## 1. 概要

TMDB (The Movie Database) は映画・TV シリーズのメタデータを提供するサービスである。
`TmdbClient` は TMDB API v3 へ HTTP リクエストを送信し、
TV シリーズ検索・映画検索・シリーズ詳細・シーズン詳細を取得するコンポーネントである。

| 項目           | 値                                             |
| -------------- | ---------------------------------------------- |
| Base URL       | `https://api.themoviedb.org/3/`                |
| 認証           | Bearer Token (`Authorization: Bearer {token}`) |
| 環境変数       | `TMDB_API_TOKEN`                               |
| レスポンス形式 | JSON                                           |
| gzip           | `reqwest` の `gzip` feature で透過的に処理     |

---

## 2. API エンドポイント

### 2.1 search/tv

TV シリーズをキーワード検索する。

| パラメータ            | 必須 | 型     | 説明                                       |
| --------------------- | ---- | ------ | ------------------------------------------ |
| `query`               | Yes  | String | 検索キーワード                             |
| `language`            | No   | String | レスポンス言語 (デフォルト: `en-US`)       |
| `page`                | No   | u32    | ページ番号 (1-500, デフォルト: 1)          |
| `first_air_date_year` | No   | u32    | 初回放送年フィルタ                         |
| `year`                | No   | u32    | 年フィルタ                                 |
| `include_adult`       | No   | bool   | アダルトコンテンツ含有 (デフォルト: false) |

### 2.2 search/movie

映画をキーワード検索する。

| パラメータ             | 必須 | 型     | 説明                                       |
| ---------------------- | ---- | ------ | ------------------------------------------ |
| `query`                | Yes  | String | 検索キーワード                             |
| `language`             | No   | String | レスポンス言語 (デフォルト: `en-US`)       |
| `page`                 | No   | u32    | ページ番号 (1-500, デフォルト: 1)          |
| `primary_release_year` | No   | u32    | 公開年フィルタ                             |
| `year`                 | No   | u32    | 年フィルタ                                 |
| `region`               | No   | String | リージョンフィルタ (ISO 3166-1)            |
| `include_adult`        | No   | bool   | アダルトコンテンツ含有 (デフォルト: false) |

### 2.3 tv/{series_id}

TV シリーズの詳細情報を取得する。シーズン一覧を含む。

| パラメータ  | 必須 | 型     | 説明                                 |
| ----------- | ---- | ------ | ------------------------------------ |
| `series_id` | Yes  | u64    | TMDB シリーズ ID (URL パス)          |
| `language`  | No   | String | レスポンス言語 (デフォルト: `en-US`) |

### 2.4 tv/{series_id}/season/{season_number}

TV シーズンの詳細情報を取得する。エピソード一覧を含む。

| パラメータ      | 必須 | 型     | 説明                                 |
| --------------- | ---- | ------ | ------------------------------------ |
| `series_id`     | Yes  | u64    | TMDB シリーズ ID (URL パス)          |
| `season_number` | Yes  | u32    | シーズン番号 (URL パス)              |
| `language`      | No   | String | レスポンス言語 (デフォルト: `en-US`) |

---

## 3. レート制限

> 参照: [external/themoviedb/rate-limiting.md](../../external/themoviedb/rate-limiting.md)

| 制限種別     | 値                  | 備考                 |
| ------------ | ------------------- | -------------------- |
| 上限         | ~40 リクエスト / 秒 | 公式の明確な値はなし |
| min_interval | 25ms                | 安全マージンを含む   |

**429 レスポンス時の挙動:**

- 最大 3 回リトライ
- バックオフ: 1 秒 × リトライ回数 (1s, 2s, 3s)
- 3 回超過で `bail!` でエラー

---

## 4. `TmdbClient` 構造体

### 4.1 フィールド定義

```rust
pub struct TmdbClient {
    /// HTTP クライアント (reqwest、gzip 有効)
    http_client: Client,
    /// Base URL (`https://api.themoviedb.org/3/`)
    base_url: Url,
    /// Bearer API トークン
    api_token: String,
    /// レートリミッター
    rate_limiter: Arc<Mutex<TmdbRateLimiter>>,
}
```

### 4.2 ビルダーパターン

```rust
pub struct TmdbClientBuilder {
    base_url: Option<Url>,
    api_token: Option<String>,
    user_agent: Option<String>,
    min_interval: Option<Duration>,
}
```

**デフォルト値:**

| パラメータ     | デフォルト値                          |
| -------------- | ------------------------------------- |
| `base_url`     | `https://api.themoviedb.org/3/`       |
| `api_token`    | なし (**必須、未設定でビルドエラー**) |
| `user_agent`   | なし (**必須、未設定でビルドエラー**) |
| `min_interval` | `Duration::from_millis(25)`           |

---

## 5. `TmdbApi` トレイト

```rust
#[trait_variant::make(TmdbApi: Send)]
pub trait LocalTmdbApi {
    async fn search_tv(&self, params: &SearchTvParams) -> Result<TmdbSearchTvResponse>;
    async fn search_movie(&self, params: &SearchMovieParams) -> Result<TmdbSearchMovieResponse>;
    async fn tv_details(&self, series_id: u64, language: &str) -> Result<TmdbTvDetails>;
    async fn tv_season(&self, series_id: u64, season_number: u32, language: &str) -> Result<TmdbTvSeason>;
}
```

---

## 6. レスポンス型

JSON レスポンスを `serde::Deserialize` でデシリアライズした Rust 構造体。
`types.rs` に定義。主要な型:

- `TmdbSearchTvResponse` / `TmdbTvSearchResult`
- `TmdbSearchMovieResponse` / `TmdbMovieSearchResult`
- `TmdbTvDetails` / `TmdbSeasonSummary` / `TmdbGenre`
- `TmdbTvSeason` / `TmdbEpisode`
- `TmdbErrorResponse`

---

## 7. 認証

TMDB API v3 は Bearer Token 認証を使用する。

```
Authorization: Bearer {TMDB_API_TOKEN}
```

トークンは TMDB アカウントの API 設定ページから「API Read Access Token」として取得する。
CLI では環境変数 `TMDB_API_TOKEN` から読み込む。

---

## 8. エラーハンドリング

TMDB API のエラーレスポンスは以下の JSON 構造を持つ:

```json
{
	"status_code": 7,
	"status_message": "Invalid API key: You must be granted a valid key.",
	"success": false
}
```

クライアントは HTTP ステータスに応じて:

| HTTP Status | 処理                                         |
| ----------- | -------------------------------------------- |
| 200         | JSON パース → 型にデシリアライズ             |
| 429         | リトライ (最大 3 回、バックオフ付き)         |
| その他      | エラーボディを `TmdbErrorResponse` でパース  |
|             | パース失敗時は生テキストをエラーメッセージに |

---

## 9. モジュール構成

```
src/libs/tmdb/
├── mod.rs              # モジュール定義 + re-exports
├── api.rs              # TmdbApi トレイト
├── client.rs           # TmdbClient + TmdbClientBuilder + テスト
├── types.rs            # JSON レスポンス型 + 検索パラメータ型
└── rate_limiter.rs     # 単層レートリミッター (~40 req/s)
```

---

## 10. CLI サブコマンド

```
dtvmgr tmdb search-tv --query "SPY×FAMILY" [--language ja-JP] [--year 2022]
dtvmgr tmdb search-movie --query "すずめの戸締まり" [--language ja-JP]
dtvmgr tmdb tv-details --id 120089 [--language ja-JP]
dtvmgr tmdb tv-season --id 120089 --season 1 [--language ja-JP]
```

すべて `TMDB_API_TOKEN` 環境変数が必要。

---

## 11. テスト

### 11.1 テストフィクスチャ

```
fixtures/tmdb/
├── search_tv_spy_family.json      # SPY×FAMILY search/tv レスポンス
├── search_tv_empty.json           # 結果 0 件の search/tv レスポンス
├── search_movie_suzume.json       # すずめの戸締まり search/movie レスポンス
├── tv_details_120089.json         # SPY×FAMILY tv/{id} レスポンス
└── tv_season_120089_1.json        # SPY×FAMILY tv/{id}/season/1 レスポンス
```

### 11.2 テスト項目

| テスト種別         | 内容                                       |
| ------------------ | ------------------------------------------ |
| Builder テスト     | `api_token` / `user_agent` 未設定でエラー  |
| JSON パーステスト  | fixture → struct デシリアライズ検証        |
| wiremock テスト    | Bearer ヘッダー送信、各エンドポイント検証  |
| エラーテスト       | 401 → `TmdbErrorResponse` パース           |
| 429 リトライテスト | `MAX_RETRIES + 1` 回のリクエスト後にエラー |
| レート制限テスト   | min_interval が遵守されることを確認        |

---

## 12. 依存関係

| Crate           | バージョン | 用途                             |
| --------------- | ---------- | -------------------------------- |
| `reqwest`       | 0.13       | HTTP クライアント (gzip, json)   |
| `serde`         | 1          | JSON デシリアライズ              |
| `serde_json`    | 1          | JSON パース                      |
| `tokio`         | 1          | 非同期ランタイム (time, sync)    |
| `url`           | 2          | URL 型管理                       |
| `tracing`       | 0.1        | 構造化ログ                       |
| `anyhow`        | 1          | エラーハンドリング               |
| `trait-variant` | 0.1        | `Send` bound 付き async トレイト |
| `wiremock`      | 0.6        | (dev) HTTP モックサーバー        |

新規クレート追加なし。すべて既存依存で対応。
