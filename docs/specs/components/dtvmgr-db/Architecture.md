# dtvmgr-db Architecture

## 概要

しょぼいカレンダー API および EPGStation API のレスポンスをローカルにキャッシュする SQLite データベースクレート。`rusqlite` (bundled SQLite) を使用し、オフライン参照・TUI 表示・TMDB マッピング保存を可能にする。

## ステータス

- **実装状態**: 完了
- **Rust クレート**: `crates/dtvmgr-db`

## モジュール構成

| モジュール    | 責務                                                  |
| ------------- | ----------------------------------------------------- |
| `connection`  | DB ファイルパス解決・接続オープン・マイグレーション実行 |
| `migrations`  | `PRAGMA user_version` によるスキーマバージョン管理     |
| `titles`      | タイトルキャッシュ CRUD と TMDB マッピング更新         |
| `programs`    | 番組(放送予定)キャッシュ CRUD                        |
| `channels`    | チャンネル / チャンネルグループキャッシュ CRUD         |
| `recorded`    | EPGStation 録画アイテム・動画ファイルキャッシュ CRUD   |

## テーブル一覧

| テーブル               | 主キー | 概要                                          |
| ---------------------- | ------ | --------------------------------------------- |
| `titles`               | `tid`  | しょぼいタイトル + TMDB マッピング情報         |
| `programs`             | `pid`  | しょぼい番組スケジュール                      |
| `channels`             | `ch_id`| しょぼいチャンネル                            |
| `channel_groups`       | `ch_gid`| しょぼいチャンネルグループ                   |
| `epg_recorded_items`   | `id`   | EPGStation 録画アイテム                       |
| `epg_video_files`      | `id`   | 録画に紐づく動画ファイル (CASCADE 削除)        |

## マイグレーション

- `PRAGMA user_version` でスキーマバージョンを管理 (現在 v7)
- `run_migrations()` で順次 `migrate_v1` ~ `migrate_v7` を適用
- 既にバージョンが最新の場合は書き込みをスキップ (読み取り専用 DB 対応)

## 公開 API

- `open_db(dir)` - DB 接続オープン + マイグレーション + 外部キー有効化
- `upsert_*` / `load_*` / `delete_*_not_in` - 各テーブルの CRUD 操作
- `filter_keywords` / `parse_keywords` - タイトルキーワード処理
- `update_tmdb_*` - TMDB マッピング・検索結果の更新
- `load_recorded_items_page` - ページネーション付き録画アイテム取得

## 依存関係

### Rust クレート

| クレート    | 用途                              |
| ----------- | --------------------------------- |
| `rusqlite`  | SQLite バインディング (bundled)    |
| `anyhow`    | エラーハンドリング                |
| `tracing`   | 関数レベルのトレーシング          |

### 内部依存

- `dtvmgr-cli` が DB 同期・TUI 表示・TMDB ルックアップで利用
- `dtvmgr-tui` がタイトル / 録画データの表示で参照
