# リネームパイプライン設計

> 関連ドキュメント:
>
> - [TMDB クライアント仕様](recmgr-api/tmdbClient.md)
> - [しょぼかるクライアント仕様](recmgr-api/syoboiClient.md)
> - [TMDB エピソード照合ロジック](tmdb-episode-matching.md)
> - [ファイル名生成仕様](filename-generation.md)

---

## 1. 概要

録画データ(アニメ、映画、ドラマ)のファイル名から得られる情報を使い、
TMDB を検索して Plex/Emby 互換のファイル名にリネームする。

コンテンツ種別ごとに異なるパイプラインを用意する:

| 種別   | 情報ソース                     | TMDB 問合せ             | 出力形式                    |
| ------ | ------------------------------ | ----------------------- | --------------------------- |
| アニメ | ファイル名 → しょぼかる → TMDB | `search/tv` → `tv/{id}` | `<title> (Year) s01e06.ext` |
| ドラマ | ファイル名 → TMDB 直接         | `search/tv` → `tv/{id}` | `<title> (Year) s01e06.ext` |
| 映画   | ファイル名 → TMDB 直接         | `search/movie`          | `<title> (Year).ext`        |

---

## 2. 全体アーキテクチャ

```
┌─────────────────────────────────────────────────────┐
│ ファイル名                                           │
│ "2026-02-14T223000 [TOKYO MX1] タイトル #6.ts"      │
└──────────────────┬──────────────────────────────────┘
                   │ パース
                   ▼
         ┌─────────────────┐
         │ ParsedFilename   │
         │ - datetime       │
         │ - channel        │
         │ - title          │
         │ - episode_num    │
         └────────┬────────┘
                  │
        ┌─────────┴─────────┐
        │ コンテンツ種別判定  │ ← CLI フラグ or 自動判定
        └─┬───────┬───────┬─┘
          │       │       │
    ┌─────▼──┐ ┌──▼───┐ ┌▼──────┐
    │ アニメ  │ │ドラマ│ │ 映画  │
    │Pipeline│ │      │ │       │
    └─────┬──┘ └──┬───┘ └┬──────┘
          │       │       │
          ▼       ▼       ▼
    Plex/Emby 互換ファイル名
```

---

## 3. Pipeline 1: アニメ (しょぼかる → TMDB)

### 3.1 フロー

```
ファイル名パース
    │
    ▼
Step 1: しょぼかる ProgLookup
  params: Range=(datetime±30min), ChID=<channel→ChID変換>
  → SyoboiProgram (TID, Count, Flag, STSubTitle)
    │
    ▼
Step 2: 再放送判定
  Flag & 8 != 0 ?
  → Yes: 既存ファイルあり→Skip / なし→Step 3 へ
  → No: Step 3 へ
    │
    ▼
Step 3: しょぼかる TitleLookup
  → SyoboiTitle (Title, TitleEN, FirstYear, FirstMonth, SubTitles)
    │
    ▼
Step 4: TMDB Series ID 特定
  3段階フォールバック検索:
  (1) Title そのまま
  (2) strip_season_suffix(Title)
  (3) TitleEN
  → filter: origin_country="JP"
    │
    ▼
Step 5: シーズン特定
  - 単一シーズン → 即確定
  - スプリットクール → Count範囲照合
  - 複数シーズン → air_date照合 → SubTitles一括照合 → エピソード数照合
    │
    ▼
Step 6: エピソード番号
  episode_number = Count
  (常に直接マッピング、オフセット不要)
    │
    ▼
"<title> (Year) s{SS}e{EE}.ext"
```

### 3.2 再放送の課題と対策

| 課題                     | 原因                            | 対策                                                  |
| ------------------------ | ------------------------------- | ----------------------------------------------------- |
| TMDB `air_date` 不一致   | TMDB は初回放送日を保持         | `air_date` を主要照合手段にしない。Count ベースで照合 |
| 同一エピソードの重複処理 | 再放送でも同じ TID/Count を返す | Flag bit 3 で再放送検出 → 既存ファイル確認 → Skip     |

### 3.3 チャンネル名 → ChID マッピング

ファイル名の放送局名をしょぼかるの `ChID` に変換する必要がある。
起動時に `lookup_channels(None)` で全チャンネルを取得し、
`ch_name` / `ch_iepg_name` でマッチング。キャッシュして再利用する。

---

## 4. Pipeline 2: ドラマ (TMDB 直接)

```
ファイル名パース → title, episode_num
    │
    ▼
TMDB search/tv(query=title, language="ja-JP")
    │
    ▼
filter: origin_country=["JP"], original_language="ja"
    │
    ▼
TMDB tv/{id} → シーズン構造確認
    │
    ▼
シーズン特定 (air_date or 最新シーズン)
    │
    ▼
"<title> (Year) s{SS}e{EE}.ext"
```

アニメと異なりしょぼかるを経由しない。
タイトル正規化の必要性も低い(ドラマは続編タイトルが明確)。

---

## 5. Pipeline 3: 映画 (TMDB 直接)

```
ファイル名パース → title
    │
    ▼
TMDB search/movie(query=title, language="ja-JP")
    │
    ▼
filter: original_language="ja" (日本映画の場合)
    │
    ▼
"<title> (Year).ext"
```

エピソード/シーズンの概念がないため最もシンプル。

---

## 6. 実装フェーズ

### Phase 1 (完了): TMDB クライアントモジュール + CLI

- `src/libs/tmdb/` モジュール一式 (4 エンドポイント)
- CLI サブコマンド (`dtvmgr tmdb search-tv` 等)
- テスト (unit + wiremock)

### Phase 2: アニメパイプライン照合ロジック

`tmdb-episode-matching.md` に設計済み。

### Phase 3: ドラマ・映画パイプライン + リネームコマンド

Pipeline 2, 3 の実装 + `dtvmgr rename` コマンド。
