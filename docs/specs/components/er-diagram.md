# ER Diagram — Syoboi / TMDB Full Schema

> **Purpose**: DB 設計検討のための全フィールド ER 図。
> 次のステップで不要フィールド削除とリレーショナル設計を行う。

## Syoboi Calendar API

```mermaid
erDiagram
    SyoboiTitle {
        type tid PK "Title ID"
        string title "作品タイトル"
        string short_title "省略タイトル (nullable)"
        string title_yomi "タイトル読み仮名 (nullable)"
        string title_en "英語タイトル (nullable)"
        u32 cat "カテゴリ: 1=anime, 4=ova 等 (nullable)"
        u32 title_flag "タイトルフラグ (nullable)"
        u32 first_year "放送開始年 (nullable)"
        u32 first_month "放送開始月 (nullable)"
        string keywords "検索キーワード (nullable)"
        string sub_titles "各話サブタイトル一覧 raw text (nullable)"
        string last_update "最終更新日時"
        string UnderDropField "-----------------------------------------------"
        string comment "コメント (nullable)"
        u32 first_end_year "放送終了年 (nullable)"
        u32 first_end_month "放送終了月 (nullable)"
        string first_ch "初回放送チャンネル (nullable)"
        i32 user_point "ユーザー評価ポイント (nullable)"
        u32 user_point_rank "ユーザー評価ランク (nullable)"
    }

    SyoboiProgram {
        u32 pid PK "Program ID"
        u32 tid FK "Title ID (SyoboiTitle.tid)"
        u32 ch_id FK "Channel ID (SyoboiChannel.ch_id)"
        string st_time "放送開始日時"
        i32 st_offset "放送時間オフセット秒 (nullable)"
        string ed_time "放送終了日時"
        u32 count "話数 (nullable)"
        string sub_title "サブタイトル (nullable)"
        u32 flag "フラグ (nullable)"
        u32 deleted "削除フラグ: 0=active (nullable)"
        u32 warn "警告フラグ (nullable)"
        u32 revision "リビジョン番号 (nullable)"
        string last_update "最終更新日時 (nullable)"
        string st_sub_title "SubTitles JOIN 時のサブタイトル (nullable)"
        string UnderDropField "-----------------------------------------------"
        string prog_comment "番組コメント (nullable)"
    }

    SyoboiChannel {
        u32 ch_id PK "Channel ID"
        u32 ch_gid "チャンネルグループ ID (nullable)"
        string ch_name "チャンネル名"
        string ch_comment "チャンネルコメント (nullable)"
        string last_update "最終更新日時 (nullable)"
        string UnderDropField "-----------------------------------------------"
        string ch_iepg_name "iEPG 名 (nullable)"
        string ch_epg_url "EPG URL (nullable)"
        u32 ch_number "チャンネル番号 (nullable)"
    }

    SyoboiTitle ||--o{ SyoboiProgram : "has programs"
    SyoboiChannel ||--o{ SyoboiProgram : "broadcasts"
```

## TMDB API

```mermaid
erDiagram
    TmdbTvSearchResult {
        u64 id PK "TMDB series ID"
        string name "ローカライズ名"
        string original_name "原題"
        string original_language "原語 ISO 639-1"
        string_arr origin_country "製作国 ISO 3166-1[]"
        string first_air_date "初回放送日 YYYY-MM-DD (nullable)"
        u32_arr genre_ids "ジャンル ID[]"
        string UnderDropField "-----------------------------------------------"
        string overview "あらすじ (nullable)"
        f64 popularity "人気スコア"
        f64 vote_average "平均評価"
        u32 vote_count "評価数"
        bool adult "成人向けフラグ"
        string poster_path "ポスター画像パス (nullable)"
        string backdrop_path "背景画像パス (nullable)"
    }

    TmdbMovieSearchResult {
        u64 id PK "TMDB movie ID"
        string title "ローカライズタイトル"
        string original_title "原題"
        string original_language "原語 ISO 639-1"
        string release_date "公開日 YYYY-MM-DD (nullable)"
        u32_arr genre_ids "ジャンル ID[]"
        string UnderDropField "-----------------------------------------------"
        string overview "あらすじ (nullable)"
        f64 popularity "人気スコア"
        f64 vote_average "平均評価"
        u32 vote_count "評価数"
        bool adult "成人向けフラグ"
        bool video "ビデオフラグ"
        string poster_path "ポスター画像パス (nullable)"
        string backdrop_path "背景画像パス (nullable)"
    }

    TmdbTvDetails {
        u64 id PK "TMDB series ID"
        string name "ローカライズ名"
        string original_name "原題"
        string original_language "原語 ISO 639-1"
        string_arr origin_country "製作国 ISO 3166-1[]"
        string first_air_date "初回放送日 (nullable)"
        string last_air_date "最終放送日 (nullable)"
        u32 number_of_episodes "総エピソード数"
        u32 number_of_seasons "総シーズン数"
        string status "放送状態: Returning Series, Ended 等 (nullable)"
        bool in_production "制作中フラグ"
        string UnderDropField "-----------------------------------------------"
        string overview "あらすじ (nullable)"
        f64 popularity "人気スコア"
        f64 vote_average "平均評価"
        string poster_path "ポスター画像パス (nullable)"
    }

    TmdbSeasonSummary {
        u64 id PK "TMDB season ID"
        u32 season_number "シーズン番号: 0=specials"
        u32 episode_count "エピソード数"
        string air_date "放送日 (nullable)"
        string name "シーズン名"
        string UnderDropField "-----------------------------------------------"
        string overview "シーズン概要 (nullable)"
        f64 vote_average "平均評価"
    }

    TmdbGenre {
        u32 id PK "Genre ID"
        string name "ジャンル名"
    }

    TmdbTvSeason {
        u64 id PK "TMDB season ID"
        string internal_id "MongoDB _id (nullable)"
        u32 season_number "シーズン番号"
        string name "シーズン名 (nullable)"
        string overview "シーズン概要 (nullable)"
        string air_date "放送日 (nullable)"
        string UnderDropField "-----------------------------------------------"
        f64 vote_average "平均評価"
    }

    TmdbEpisode {
        u64 id PK "TMDB episode ID"
        u64 show_id FK "TMDB series ID (TmdbTvDetails.id)"
        u32 season_number "シーズン番号"
        u32 episode_number "エピソード番号"
        string name "エピソード名"
        string air_date "放送日 (nullable)"
        u32 runtime "再生時間(分) (nullable)"
        string episode_type "タイプ: standard, finale 等 (nullable)"
        string UnderDropField "-----------------------------------------------"
        string overview "あらすじ (nullable)"
        f64 vote_average "平均評価"
    }

    TmdbTvDetails ||--o{ TmdbSeasonSummary : "has seasons"
    TmdbTvDetails ||--o{ TmdbGenre : "has genres"
    TmdbTvSeason ||--o{ TmdbEpisode : "has episodes"
    TmdbTvDetails ||--o{ TmdbTvSeason : "detail of season"
    TmdbTvDetails ||--o{ TmdbEpisode : "has episodes"
```

## Local DB Tables

> API レスポンスに TMDB マッチング情報を結合したローカル DB テーブル。
> cache として機能し、既存の放送中 TID/PID は対応済みのため TMDB API 呼び出しを大幅に削減できる。

```mermaid
erDiagram
    titles {
        u32 tid PK "SyoboiTitle.tid"
        u64 tmdb_series_id "TMDB series ID (nullable, cache)"
        u32 tmdb_season_number "TMDB season 番号 (nullable, cache)"
        string title "作品タイトル"
        string short_title "省略タイトル (nullable)"
        string title_yomi "タイトル読み仮名 (nullable)"
        string title_en "英語タイトル (nullable)"
        u32 cat "カテゴリ (nullable)"
        u32 title_flag "タイトルフラグ (nullable)"
        u32 first_year "放送開始年 (nullable)"
        u32 first_month "放送開始月 (nullable)"
        string keywords "検索キーワード (nullable)"
        string sub_titles "各話サブタイトル一覧 (nullable)"
        string last_update "最終更新日時"
    }

    programs {
        u32 pid PK "SyoboiProgram.pid"
        u32 tid FK "titles.tid"
        u32 ch_id FK "channels.ch_id"
        u64 tmdb_episode_id "TMDB episode ID (nullable, cache)"
        string st_time "放送開始日時"
        i32 st_offset "放送時間オフセット秒 (nullable)"
        string ed_time "放送終了日時"
        u32 count "話数 (nullable)"
        string sub_title "サブタイトル (nullable)"
        u32 flag "フラグ (nullable)"
        u32 deleted "削除フラグ (nullable)"
        u32 warn "警告フラグ (nullable)"
        u32 revision "リビジョン番号 (nullable)"
        string last_update "最終更新日時 (nullable)"
        string st_sub_title "SubTitles JOIN サブタイトル (nullable)"
    }

    channels {
        u32 ch_id PK "SyoboiChannel.ch_id"
        u32 ch_gid "チャンネルグループ ID (nullable)"
        string ch_name "チャンネル名"
        string ch_comment "チャンネルコメント (nullable)"
        string last_update "最終更新日時 (nullable)"
    }

    titles ||--o{ programs : "has programs"
    channels ||--o{ programs : "broadcasts"
```
