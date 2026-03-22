# TMDB

- 検索対象: `ルパン三世`
  - ショボかる
    - URL: `https://cal.syoboi.jp/tid/1072`
    - タイトル: `ルパン三世 1stシリーズ`
    - よみ: `るぱんさんせい ふぁーすとしりーず`
    - 略名: `ルパン三世1`
    - 放送期間: `1971-10～1972-3`
  - TMDB
    - URL: `https://www.themoviedb.org/tv/31572?language=ja`
    - Original TV Show Language: `Japanese`
    - Original Name: `ルパン三世`
    - Translated Name (Japanese): ``
    - Year: `1971`
    - Alternative Names
      - JP `Lupin III`
      - JP `Rupan Sansei`

## ジャンル id

### Genre / TV List

```bash
GET 'https://api.themoviedb.org/3/genre/tv/list?language=ja'

{
  "genres": [
    {
      "id": 10759,
      "name": "Action & Adventure"
    },
    {
      "id": 16,
      "name": "アニメーション"
    },

    ...(snip)...
  ]
}
```

### Genre / Movie List

```bash
GET 'https://api.themoviedb.org/3/genre/movie/list?language=ja'

{
  "genres": [
    {
      "id": 28,
      "name": "アクション"
    },
    {
      "id": 12,
      "name": "アドベンチャー"
    },
    {
      "id": 16,
      "name": "アニメーション"
    },

    ...(snip)...

  ]
}
```

## Search Multi

```bash
GET 'https://api.themoviedb.org/3/search/multi?query=%E3%83%AB%E3%83%91%E3%83%B3%E4%B8%89%E4%B8%96&include_adult=false&language=ja-JP&page=1'

{
  "page": 1,
  "results": [
    {
      "id": 31572,
      "name": "ルパン三世",
      "original_name": "ルパン三世",
      "media_type": "tv",
      "original_language": "ja",
      "genre_ids": [
        16,
        10759,
        10765
      ],
      "popularity": 30.0644,
      "first_air_date": "1971-10-24",
      "origin_country": [
        "JP"
      ]
    },
    {
      "id": 241868,
      "title": "ルパン三世",
      "original_title": "ルパン三世",
      "media_type": "movie",
      "original_language": "ja",
      "genre_ids": [
        28,
        12,
        35,
        80
      ],
      "popularity": 1.0423,
      "release_date": "2014-08-30"
    }
    ...(snip)...
  ],
  "total_pages": 1,
  "total_results": 2
}
```

## TV Series

### TV Series / Details

- if に使える項目
  - `1971-10` in first_air_date
  - `16` in genres
  - `true` == in_production
  - `ja` in languages
  - `ルパン三世` == name
  - しょぼいの FirstEndYear / FirstEndMonth が last_air_date までに収まるか

```bash
GET 'https://api.themoviedb.org/3/tv/31572?language=ja-JP'

{
  "episode_run_time": [
    25
  ],
  "first_air_date": "1971-10-24",
  "genres": [
    {
      "id": 16,
      "name": "アニメーション"
    },
    {
      "id": 10759,
      "name": "Action & Adventure"
    },
    {
      "id": 10765,
      "name": "Sci-Fi & Fantasy"
    }
  ],
  "id": 31572,
  "in_production": true,
  "languages": [
    "ja"
  ],
  "last_air_date": "2022-03-27",
  "name": "ルパン三世",
  "networks": [
    {
      "id": 569,
      "logo_path": "/cIMyE9cw1W4kMFGxmC17HKTnVz9.png",
      "name": "YTV",
      "origin_country": "JP"
    }
  ],
  "number_of_episodes": 300,
  "number_of_seasons": 6,
  "origin_country": [
    "JP"
  ],
  "original_language": "ja",
  "original_name": "ルパン三世",
  "popularity": 30.4635,
  "seasons": [
    {
      "air_date": "2016-03-23",
      "episode_count": 3,
      "id": 235696,
      "name": "特別編",
      "season_number": 0,
      "vote_average": 0
    },
    {
      "air_date": "1971-10-24",
      "episode_count": 23,
      "id": 64308,
      "name": "シーズン1",
      "season_number": 1,
      "vote_average": 7.1
    },
    {
      "air_date": "1977-10-03",
      "episode_count": 155,
      "id": 43083,
      "name": "シーズン2",
      "season_number": 2,
      "vote_average": 7
    },
    {
      "air_date": "1984-03-03",
      "episode_count": 50,
      "id": 43085,
      "name": "シーズン3",
      "season_number": 3,
      "vote_average": 6.7
    },
    {
      "air_date": "2015-10-01",
      "episode_count": 24,
      "id": 70097,
      "name": "シーズン4",
      "season_number": 4,
      "vote_average": 7
    },
    {
      "air_date": "2018-04-04",
      "episode_count": 24,
      "id": 101713,
      "name": "PART5",
      "season_number": 5,
      "vote_average": 0
    },
    {
      "air_date": "2021-10-16",
      "episode_count": 24,
      "id": 196955,
      "name": "PART6",
      "season_number": 6,
      "vote_average": 4
    }
  ],
  "spoken_languages": [
    {
      "english_name": "Japanese",
      "iso_639_1": "ja",
      "name": "日本語"
    }
  ],
  "status": "Returning Series"
}
```

### TV Series / Alternative Titles

```bash
GET https://api.themoviedb.org/3/tv/31572/alternative_titles

{
  "id": 31572,
  "results": [
    {
      "iso_3166_1": "JP",
      "title": "Lupin III",
      "type": "romaji"
    },
    {
      "iso_3166_1": "JP",
      "title": "Rupan Sansei",
      "type": ""
    }
    ...(snip)...
  ]
}
```

## TV Seasons

### TV Seasons / Details

```bash
GET 'https://api.themoviedb.org/3/tv/31572/season/1?language=ja-JP'

{
  "_id": "54aa3ee09251415679000f53",
  "air_date": "1971-10-24",
  "episodes": [
    {
      "air_date": "1971-10-24",
      "episode_number": 1,
      "episode_type": "standard",
      "id": 1032762,
      "name": "ルパンは燃えているか・・・・?!",
      "production_code": "",
      "runtime": 25,
      "season_number": 1,
      "show_id": 31572,
      "vote_average": 6.2,
      "vote_count": 8,
      "crew": [],
      "guest_stars": []
    },
    {
      "air_date": "1971-10-31",
      "episode_number": 2,
      "episode_type": "standard",
      "id": 1032763,
      "name": "魔術師と呼ばれた男",
      "production_code": "",
      "runtime": 25,
      "season_number": 1,
      "show_id": 31572,
      "vote_average": 7.8,
      "vote_count": 4,
      "crew": [],
      "guest_stars": []
    },
    {
      "air_date": "1971-11-07",
      "episode_number": 3,
      "episode_type": "standard",
      "id": 1032764,
      "name": "さらば愛しき魔女",
      "production_code": "",
      "runtime": 25,
      "season_number": 1,
      "show_id": 31572,
      "vote_average": 7.7,
      "vote_count": 3,
      "crew": [],
      "guest_stars": []
    }

    ...(snip)...
  ],
  "name": "シーズン1",
  "networks": [
    {
      "id": 569,
      "name": "YTV",
      "origin_country": "JP"
    }
  ],
  "id": 64308,
  "season_number": 1,
  "vote_average": 7.1
}
```
