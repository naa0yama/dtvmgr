# Syoboi チャンネル選択 TUI — 設計書

## Context

`dtvmgr` CLI に Syoboi Calendar のチャンネル ID (`chid`) を対話的に選択するサブコマンドを追加する。
Web UI では約 288 チャンネルが 24 グループ(テレビ 関東、BSデジタル、インターネット等)に分類されており、チェックボックスでグループ単位 / 個別選択が可能。
これと同等の体験を TUI で実現する。

## 技術スタック

| 用途            | ライブラリ                    | 理由                                                 |
| --------------- | ----------------------------- | ---------------------------------------------------- |
| TUI             | `ratatui` + `crossterm`       | 2ペインレイアウト、将来の TUI 画面にも再利用         |
| DB (キャッシュ) | `rusqlite` (bundled)          | API レスポンスのキャッシュ。同期 API が TUI と好相性 |
| 設定ファイル    | `toml`                        | ユーザー設定(選択チャンネル等)の永続化。Rust 標準    |
| Runtime         | `tokio` `current_thread` 維持 | API rate limiter でシーケンシャル、変更不要          |

### 技術選定の背景

- **ratatui**: Rust TUI のデファクト (DL 1190 万超)。`inquire`/`dialoguer` は 280 項目のグループ一括選択に対応不可
- **rusqlite**: `current_thread` tokio と互換。`sqlx` は `current_thread` でパニック。年間 1 万件程度のデータに async DB は不要
- **TOML**: `serde_yaml` は deprecated。Rust プロジェクトの設定ファイル標準
- **`current_thread` 維持**: API rate limiter でシーケンシャル実行、DB は同期で十分、変更メリットなし

## データの役割分担

| 保存先          | 内容                                | 用途                                         |
| --------------- | ----------------------------------- | -------------------------------------------- |
| **config.toml** | 選択済み ch_id リスト、ユーザー設定 | ユーザーが決めた設定。手動編集も可           |
| **rusqlite DB** | channels, channel_groups キャッシュ | API レスポンスのキャッシュ。TUI 表示用データ |

```toml
# ~/.config/dtvmgr/config.toml (デフォルト)
# --dir オプションで任意のディレクトリに変更可能

[channels]
selected = [1, 2, 3, 4, 5, 6, 7, 8, 14, 19, 187]
```

## TUI レイアウト (2 ペイン)

```
┌─ Channel Selector ────────────────────────────────────────────────┐
│  Filter: [_______________]                    Selected: 10 / 288  │
├─ Groups ──────────────────┬─ Channels ────────────────────────────┤
│                           │                                       │
│   テレビ 全国        0/6  │  [ ]   1  NHK総合                    │
│   テレビ 北海道      0/5  │  [ ]   2  NHK Eテレ                  │
│   テレビ 東北        0/7  │  [ ]  64  NHK Eテレ2                 │
│ ▸ テレビ 関東      10/12  │  [ ]  65  NHK Eテレ3                 │
│   テレビ 甲信越      0/8  │  [ ] 192  NHKワンセグ2               │
│   テレビ 北陸        0/6  │  [ ] 245  J:COMテレビ                │
│   テレビ 東海       0/11  │                                       │
│   テレビ 近畿       0/10  │                                       │
│   ...                     │                                       │
├───────────────────────────┴───────────────────────────────────────┤
│ Tab: pane switch   ↑↓/j/k: move   Space: toggle                  │
│ a: select all in group   /: filter   Enter: confirm   q: cancel   │
└───────────────────────────────────────────────────────────────────┘
```

### キー操作

| キー         | 動作                                               |
| ------------ | -------------------------------------------------- |
| `Tab`        | 左ペイン(グループ) ↔ 右ペイン(チャンネル)切替      |
| `↑↓` / `j/k` | カーソル移動                                       |
| `Space`      | 左ペイン: グループ全体トグル、右ペイン: 個別トグル |
| `a` / `A`    | グループ全選択 / 全解除                            |
| `/`          | フィルター入力モード                               |
| `Enter`      | 確定                                               |
| `q` / `Esc`  | キャンセル                                         |

## データフロー

```
1. Syoboi API (async):
   - lookup_channel_groups(None) --> Vec<SyoboiChannelGroup>  (グループ名 + 表示順)
   - lookup_channels(None)       --> Vec<SyoboiChannel>       (チャンネル一覧)
2. ch_gid でグルーピング、ChGroupOrder で表示順ソート
3. rusqlite に channel_groups / channels テーブルとして保存 (キャッシュ)
4. config.toml から選択済み ch_id を読み込み → TUI 初期状態に反映
5. TUI 起動 (同期イベントループ) --> ユーザーが選択
6. 確定時に config.toml の [channels].selected を更新
7. syoboi prog 等のコマンドで config.toml の selected を --ch-ids デフォルトとして利用
```

## API

### ChGroupLookup (新規実装)

```
GET https://cal.syoboi.jp/db.php?Command=ChGroupLookup&ChGID=*
```

```xml
<ChGroupLookupResponse>
    <ChGroupItems>
        <ChGroupItem id="1">
            <ChGID>1</ChGID>
            <ChGroupName>テレビ 関東</ChGroupName>
            <ChGroupComment></ChGroupComment>
            <ChGroupOrder>1200</ChGroupOrder>
        </ChGroupItem>
    </ChGroupItems>
</ChGroupLookupResponse>
```

### ChLookup (既存)

```
GET https://cal.syoboi.jp/db.php?Command=ChLookup
```

`SyoboiChannel` (`ch_id`, `ch_gid`, `ch_name`, etc.)

## DB スキーマ (キャッシュ用)

```sql
CREATE TABLE IF NOT EXISTS channel_groups (
    ch_gid          INTEGER PRIMARY KEY,
    ch_group_name   TEXT NOT NULL,
    ch_group_order  INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS channels (
    ch_id    INTEGER PRIMARY KEY,
    ch_gid   INTEGER REFERENCES channel_groups(ch_gid),
    ch_name  TEXT NOT NULL
);
```

## CLI コマンド

```
dtvmgr syoboi channels select              # TUI 起動、チャンネル選択 → config.toml 保存
dtvmgr syoboi channels list                # 選択済みチャンネルを表示
dtvmgr --dir ./myproject syoboi channels select   # 指定ディレクトリの config.toml を使用
```
