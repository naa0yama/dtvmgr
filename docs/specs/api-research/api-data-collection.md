# API データ収集・分析ツール設計

## 目的

しょぼかる / TMDB のマッチングロジックを確立するため、実データを収集してパターン分析を行う。

## フェーズ

```
Phase 1: データ収集ツール作成
Phase 2: サンプル収集 (50-100タイトル)
Phase 3: パターン分析・傾向把握
Phase 4: マッチングロジック実装
```

---

## しょぼかる API 仕様

> 参照: [db.php | しょぼいカレンダーのヘルプ](https://docs.cal.syoboi.jp/spec/db.php/)
> 参照: [レート制限](https://docs.cal.syoboi.jp/spec/rate_limit/)

### エンドポイント

| Command       | 用途               | 最大件数                       |
| ------------- | ------------------ | ------------------------------ |
| `TitleLookup` | タイトル情報取得   | 制限なし (TID=* で全件 4.5MB+) |
| `ProgLookup`  | 放送データ取得     | **5,000件/リクエスト**         |
| `ChLookup`    | チャンネル情報取得 | 制限なし                       |

### パラメータ書式

| パラメータ   | 書式                              | 例                                        |
| ------------ | --------------------------------- | ----------------------------------------- |
| `TID`        | 単一 / カンマ区切り / `*`         | `TID=6309`, `TID=6309,6451,7667`, `TID=*` |
| `ChID`       | 単一 / カンマ区切り / `*`         | `ChID=19`, `ChID=3,4,5,6,7`, `ChID=*`     |
| `Range`      | `YYYYMMDD_HHMMSS-YYYYMMDD_HHMMSS` | `Range=20240101_000000-20240401_000000`   |
| `LastUpdate` | `YYYYMMDD_HHMMSS-` (終端省略可)   | `LastUpdate=20240101_000000-`             |
| `JOIN`       | 追加テーブル結合                  | `JOIN=SubTitles` (STSubTitle 取得に必須)  |
| `Fields`     | 出力フィールド限定                | `Fields=TID,Title,TitleEN`                |

### レート制限

| 制限                 | 値                  | 備考                                        |
| -------------------- | ------------------- | ------------------------------------------- |
| 通常リクエスト       | **1リクエスト/秒**  | db, rss, rss2, json                         |
| 制限対象クライアント | 1リクエスト/10秒    | User-Agent 未設定、同一リクエスト繰り返し等 |
| 時間制限             | 500リクエスト/時    |                                             |
| 日次制限             | 10,000リクエスト/日 |                                             |

**User-Agent 必須:**

```
jlse-rs/0.1.0 (+https://github.com/naa0yama/JoinLogoScpTrialSetLinux)
```

### 効率的な収集戦略

#### 戦略 1: 期間ベース ProgLookup → TitleLookup

```
1. ProgLookup で 3ヶ月分の全放送データを一括取得
   GET db.php?Command=ProgLookup&Range=20240101_000000-20240401_000000&JOIN=SubTitles
   → 5,000件制限があるため、1ヶ月単位で分割

2. レスポンスからユニーク TID を抽出

3. TitleLookup でまとめて取得
   GET db.php?Command=TitleLookup&TID=6309,6451,7667,...
   → カンマ区切りで 50-100 TID ずつ (URL長制限考慮)
```

**リクエスト数見積もり (1クール分):**

- ProgLookup: 3リクエスト (月単位)
- TitleLookup: 1-2リクエスト (50-100 TID)
- **合計: 5リクエスト程度** ← レート制限内

#### 戦略 2: 差分更新 (LastUpdate)

```
# 前回収集以降の更新分のみ取得
GET db.php?Command=TitleLookup&LastUpdate=20240301_000000-
```

#### 戦略 3: 全タイトル取得 (初期構築)

```
# 注意: 4.5MB+ のレスポンス
GET db.php?Command=TitleLookup&TID=*&Fields=TID,Title,ShortTitle,TitleYomi,TitleEN,FirstYear,FirstMonth
```

1回で全アニメタイトルを取得可能。ただしレスポンスが巨大なため:

- gzip 圧縮を有効化 (`Accept-Encoding: gzip`)
- ストリーミングパース
- ローカル DB に保存後は差分更新

---

## Phase 1: データ収集ツール

### 1.1 ローカル DB スキーマ (DuckDB)

```sql
-- しょぼかるタイトル
CREATE TABLE syobocal_titles (
    tid INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    short_title TEXT,
    title_yomi TEXT,
    title_en TEXT,
    first_year INTEGER,
    first_month INTEGER,
    first_end_year INTEGER,
    first_end_month INTEGER,
    first_ch TEXT,
    cat INTEGER,
    subtitles TEXT,  -- raw *01*xxx format
    fetched_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- しょぼかる放送データ
CREATE TABLE syobocal_programs (
    pid INTEGER PRIMARY KEY,
    tid INTEGER NOT NULL,
    ch_id INTEGER NOT NULL,
    st_time TIMESTAMP NOT NULL,
    ed_time TIMESTAMP NOT NULL,
    count INTEGER,
    st_subtitle TEXT,
    flag INTEGER,
    fetched_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (tid) REFERENCES syobocal_titles(tid)
);

-- TMDB 検索結果
CREATE TABLE tmdb_search_results (
    id INTEGER PRIMARY KEY,  -- AUTO INCREMENT
    query TEXT NOT NULL,
    query_type TEXT NOT NULL,  -- 'title', 'title_en', 'title_stripped'
    syobocal_tid INTEGER,
    tmdb_id INTEGER,
    tmdb_name TEXT,
    tmdb_original_name TEXT,
    tmdb_first_air_date TEXT,
    tmdb_origin_country TEXT,  -- JSON array
    popularity REAL,
    vote_average REAL,
    total_results INTEGER,
    result_index INTEGER,  -- 検索結果内の順位 (0-based)
    fetched_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- TMDB シリーズ詳細
CREATE TABLE tmdb_series (
    tmdb_id INTEGER PRIMARY KEY,
    name TEXT,
    original_name TEXT,
    first_air_date TEXT,
    last_air_date TEXT,
    status TEXT,
    number_of_seasons INTEGER,
    number_of_episodes INTEGER,
    origin_country TEXT,  -- JSON array
    genres TEXT,  -- JSON array
    raw_json TEXT,  -- 全データ保存
    fetched_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- TMDB シーズン詳細
CREATE TABLE tmdb_seasons (
    id INTEGER PRIMARY KEY,  -- tmdb season id
    series_id INTEGER NOT NULL,
    season_number INTEGER NOT NULL,
    name TEXT,
    air_date TEXT,
    episode_count INTEGER,
    episodes TEXT,  -- JSON array of {episode_number, name, air_date}
    fetched_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (series_id) REFERENCES tmdb_series(tmdb_id),
    UNIQUE (series_id, season_number)
);

-- マッチング結果 (検証用)
CREATE TABLE matching_results (
    id INTEGER PRIMARY KEY,
    syobocal_tid INTEGER NOT NULL,
    syobocal_title TEXT,
    tmdb_id INTEGER,
    tmdb_name TEXT,
    match_method TEXT,  -- 'exact', 'stripped', 'title_en', 'manual', 'failed'
    confidence REAL,  -- 0.0-1.0
    season_number INTEGER,
    season_match_method TEXT,
    notes TEXT,
    verified BOOLEAN DEFAULT FALSE,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

### 1.2 収集 CLI コマンド

```bash
# === しょぼかる収集 ===

# タイトル取得 (単一/複数/全件)
jlse-api syobocal titles --tid 6309
jlse-api syobocal titles --tid 6309,6451,7667  # カンマ区切り
jlse-api syobocal titles --all                  # 全件 (初回のみ、4.5MB+)
jlse-api syobocal titles --since 2024-03-01     # 差分更新

# 放送データ取得 (期間指定、5000件/リクエスト制限あり)
jlse-api syobocal programs --range 2024-01-01,2024-03-31
jlse-api syobocal programs --range 2024-01-01,2024-03-31 --ch 19  # チャンネル絞り込み

# チャンネル一覧取得
jlse-api syobocal channels

# === TMDB 収集 ===

# 検索実行 (しょぼかる TID から自動検索)
jlse-api tmdb search --tid 6309              # 3段階フォールバック
jlse-api tmdb search --query "SPY×FAMILY"    # 直接検索

# シリーズ詳細取得
jlse-api tmdb series 120089
jlse-api tmdb seasons 120089

# === バッチ収集 (レート制限遵守) ===

# 期間ベース収集 (推奨)
#   1. ProgLookup で期間内の全放送データ取得
#   2. ユニーク TID を抽出
#   3. TitleLookup でまとめて取得
#   4. TMDB 検索実行
jlse-api collect --season 2024-winter  # 2024年1月期
jlse-api collect --range 2024-01-01,2024-03-31

# ドライラン (リクエスト数見積もり)
jlse-api collect --season 2024-winter --dry-run

# === ローカル DB ===

# クエリ実行
jlse-api query "SELECT * FROM syobocal_titles WHERE title LIKE '%FAMILY%'"

# 統計表示
jlse-api stats
```

### 1.3 レート制限遵守の実装

```rust
use std::time::{Duration, Instant};
use tokio::time::sleep;

pub struct SyobocalClient {
    last_request: Option<Instant>,
    min_interval: Duration,  // 1秒
    user_agent: String,
}

impl SyobocalClient {
    pub fn new() -> Self {
        Self {
            last_request: None,
            min_interval: Duration::from_secs(1),
            user_agent: "jlse-rs/0.1.0 (+https://github.com/naa0yama/JoinLogoScpTrialSetLinux)".into(),
        }
    }

    async fn rate_limit(&mut self) {
        if let Some(last) = self.last_request {
            let elapsed = last.elapsed();
            if elapsed < self.min_interval {
                sleep(self.min_interval - elapsed).await;
            }
        }
        self.last_request = Some(Instant::now());
    }

    pub async fn title_lookup(&mut self, tids: &[u32]) -> Result<Vec<Title>> {
        self.rate_limit().await;

        // TID をカンマ区切りで結合 (50件ずつ分割)
        let tid_str = tids.iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join(",");

        let url = format!(
            "https://cal.syoboi.jp/db.php?Command=TitleLookup&TID={}",
            tid_str
        );

        // リクエスト実行...
    }

    pub async fn prog_lookup(&mut self, range: &str, ch_id: Option<u32>) -> Result<Vec<Program>> {
        self.rate_limit().await;

        // 5000件制限を考慮してページング
        // ...
    }
}
```

### 1.4 収集フロー (レート制限遵守版)

```
┌─────────────────────────────────────────────────────────────┐
│ 1. しょぼかる ProgLookup (期間指定)                           │
│    GET db.php?Command=ProgLookup                            │
│         &Range=20240101_000000-20240201_000000              │
│         &JOIN=SubTitles                                     │
│    → syobocal_programs に保存                                │
│    ⚠️ 5,000件制限 → 月単位で分割                              │
│    ⚠️ 1リクエスト/秒 → rate_limit() で制御                    │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│ 2. ユニーク TID 抽出                                         │
│    SELECT DISTINCT tid FROM syobocal_programs               │
│    WHERE tid NOT IN (SELECT tid FROM syobocal_titles)       │
│    → 未取得の TID リスト生成                                  │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│ 3. しょぼかる TitleLookup (バッチ)                            │
│    GET db.php?Command=TitleLookup&TID=6309,6451,7667,...    │
│    → 50-100 TID ずつカンマ区切り                              │
│    → syobocal_titles に保存                                  │
│    ⚠️ 1リクエスト/秒                                         │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│ 4. TMDB 検索 (3段階フォールバック)                            │
│    対象: syobocal_titles で tmdb_id が NULL のもの            │
│                                                             │
│    for each title:                                          │
│      Step 1: Title そのまま検索                              │
│      Step 2: Title からサフィックス除去                       │
│      Step 3: TitleEN で検索                                  │
│      → tmdb_search_results に全結果保存                      │
│    ⚠️ TMDB: 40リクエスト/10秒 (0.25秒間隔)                   │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│ 5. TMDB シリーズ詳細取得                                      │
│    対象: 検索でマッチした tmdb_id で未取得のもの              │
│                                                             │
│    GET /tv/{id}                                             │
│    GET /tv/{id}/season/{n} (全シーズン)                      │
│    → tmdb_series, tmdb_seasons に保存                        │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│ 6. マッチング結果記録                                         │
│    → matching_results に保存                                 │
│    → match_method: 'exact', 'stripped', 'title_en'          │
│    → verified: FALSE (手動検証待ち)                          │
└─────────────────────────────────────────────────────────────┘
```

### 1.5 リクエスト数見積もり

**1クール (3ヶ月) 分の収集:**

| API                    | リクエスト数    | 所要時間  |
| ---------------------- | --------------- | --------- |
| しょぼかる ProgLookup  | 3 (月単位)      | 3秒       |
| しょぼかる TitleLookup | 2 (100 TID × 2) | 2秒       |
| TMDB search            | 100-200         | 25-50秒   |
| TMDB series/seasons    | 100-200         | 25-50秒   |
| **合計**               | ~400            | **約2分** |

✅ しょぼかる: 5リクエスト << 500リクエスト/時
✅ TMDB: 400リクエスト ≈ 10分相当 (40req/10秒制限内)

---

## Phase 2: サンプル収集計画

### 2.1 カバーすべきケース

| カテゴリ             | 例                       | 収集数目安 |
| -------------------- | ------------------------ | ---------- |
| **通常 (1クール)**   | 異世界系、ラブコメ       | 20         |
| **スプリットクール** | SPY×FAMILY, 鬼滅         | 10         |
| **長期シリーズ**     | ワンピース, 名探偵コナン | 5          |
| **続編 (Season N)**  | ごちうさ, リゼロ         | 10         |
| **タイトル変更続編** | ダンまち, SAO            | 5          |
| **記号入りタイトル** | ×, !, ?, ♪, ☆            | 10         |
| **英語タイトル**     | BLEACH, NARUTO           | 5          |
| **TMDB 未登録**      | マイナー深夜アニメ       | 5          |
| **再放送**           | 各種                     | 10         |
| **特番・総集編**     | Count=0 or 特殊          | 5          |

**合計: 85タイトル程度**

### 2.2 収集対象シーズン

```
2024年冬 (1月期): 最新データ検証
2024年秋 (10月期): 続編多め
2023年春 (4月期): SPY×FAMILY S2 等
2022年春 (4月期): SPY×FAMILY S1 等
```

### 2.3 サンプル TID リスト (初期)

```toml
# samples.toml
[samples]
# スプリットクール
spy_family_1 = 6309
spy_family_2 = 6451
kimetsu_yuukaku = 6165
kimetsu_katanakaji = 6816

# 続編シリーズ
gochiusa_1 = 3324
gochiusa_2 = 3893
gochiusa_3 = 5765
rezero_1 = 4130
rezero_2 = 5763

# 記号入り
yuusha_party = 7667 # 勇者パーティーに〜
bocchi = 6615 # ぼっち・ざ・ろっく!
oshi_no_ko = 6949 # 【推しの子】
lycoris = 6539 # リコリス・リコイル

# 長期
one_piece = 350
conan = 247

# 英語タイトル
bleach_tybw = 6471
frieren = 7180

# 2024年冬期 (最新)
# TID は別途取得
```

---

## Phase 3: 分析項目

### 3.1 タイトルマッチング分析

```sql
-- 検索方法別の成功率
SELECT
    match_method,
    COUNT(*) as total,
    SUM(CASE WHEN tmdb_id IS NOT NULL THEN 1 ELSE 0 END) as matched,
    ROUND(100.0 * SUM(CASE WHEN tmdb_id IS NOT NULL THEN 1 ELSE 0 END) / COUNT(*), 1) as rate
FROM matching_results
GROUP BY match_method;

-- 除去すべきサフィックスパターン
SELECT
    t.title,
    t.short_title,
    s.query,
    s.total_results
FROM syobocal_titles t
JOIN tmdb_search_results s ON t.tid = s.syobocal_tid
WHERE s.query_type = 'title' AND s.total_results = 0;

-- TMDB検索結果の順位分析
SELECT
    result_index,
    COUNT(*) as count
FROM tmdb_search_results
WHERE tmdb_id IS NOT NULL
GROUP BY result_index
ORDER BY result_index;
```

### 3.2 シーズンマッチング分析

```sql
-- シーズン特定方法の分布
SELECT
    season_match_method,
    COUNT(*) as count
FROM matching_results
WHERE tmdb_id IS NOT NULL
GROUP BY season_match_method;

-- Count と episode_number の対応
SELECT
    t.title,
    p.count as syobocal_count,
    m.season_number,
    -- TMDB episode_number との比較
FROM syobocal_programs p
JOIN syobocal_titles t ON p.tid = t.tid
JOIN matching_results m ON t.tid = m.syobocal_tid;
```

### 3.3 エッジケース抽出

```sql
-- 複数候補がある検索結果
SELECT
    query,
    COUNT(*) as candidates
FROM tmdb_search_results
WHERE total_results > 1
GROUP BY query
HAVING COUNT(*) > 1;

-- origin_country フィルタが必要なケース
SELECT DISTINCT
    t.title,
    s.tmdb_name,
    s.tmdb_origin_country
FROM syobocal_titles t
JOIN tmdb_search_results s ON t.tid = s.syobocal_tid
WHERE s.tmdb_origin_country NOT LIKE '%JP%';
```

---

## Phase 4: 出力

### 4.1 パターンカタログ

分析結果から以下を文書化:

1. **タイトル正規化ルール**
   - 除去すべきサフィックス一覧
   - 記号変換ルール (×→x 等)
   - 括弧処理ルール

2. **TMDB 検索戦略**
   - 検索順序の最適化
   - origin_country フィルタ条件
   - 複数候補時の選択基準

3. **シーズン特定ルール**
   - FirstYear/Month → air_date マッチング精度
   - Count 範囲によるシーズン特定の信頼度
   - フォールバック戦略

4. **エッジケース対応表**
   - 手動マッピングが必要なケース
   - TMDB 未登録時の処理

### 4.2 マッピングテーブル (手動)

```toml
# manual_mappings.toml
# 自動マッチングが困難なケースの手動マッピング

[[mapping]]
syobocal_tid = 6309
tmdb_id = 120089
season = 1
note = "SPY×FAMILY 第1期"

[[mapping]]
syobocal_tid = 6451
tmdb_id = 120089
season = 1 # TMDB では Season 1 が 25話構成
note = "SPY×FAMILY 第2クール (TMDB では Season 1 ep 13-25)"
```

---

## 実装優先順位

1. **DuckDB スキーマ作成** — 即時
2. **しょぼかる API クライアント** — TitleLookup, ProgLookup
3. **TMDB API クライアント** — search/tv, tv/{id}, tv/{id}/season/{n}
4. **CLI ツール** — jlse-api コマンド
5. **サンプル収集スクリプト** — バッチ実行
6. **分析クエリ集** — SQL ファイル

---

## 次のアクション

- [ ] DuckDB スキーマ SQL 作成
- [ ] Rust API クライアント雛形作成
- [ ] samples.toml 拡充 (2024年冬期 TID 追加)
- [ ] 収集実行 → 分析
