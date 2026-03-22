# logoframe Command Wrapper

> 親ドキュメント: [Architecture.md](./Architecture.md)

## ステータス

- **Phase**: 2
- **実装状態**: 未実装
- **Node.js ソース**: `src/command/logoframe.js` (96行)
- **Rust モジュール**: `crates/dtvmgr-jlse/src/command/logoframe.rs`

## 概要

放送局ロゴの表示フレームを検出する外部コマンド `logoframe` のラッパー。チャンネル情報に基づいてロゴファイルを選択し、入力 AVS ファイルからロゴ検出結果を生成する。

## 仕様

### 引数構築

```
logoframe <avs_file> -oa <logoframe_txt_output> -o <logoframe_avs_output> -logo <logo_file>
```

| 引数    | 値                   | 説明                           |
| ------- | -------------------- | ------------------------------ |
| 第1引数 | `in_org.avs`         | 入力 AVS ファイル              |
| `-oa`   | `obs_logoframe.txt`  | ロゴ検出結果テキスト           |
| `-o`    | `obs_logo_erase.avs` | ロゴ除去用 AVS                 |
| `-logo` | `<logo_file_or_dir>` | ロゴファイルまたはディレクトリ |

### ロゴ選択アルゴリズム

```mermaid
flowchart TD
    START([チャンネル情報]) --> HAS_CH{チャンネル<br/>検出済み?}
    HAS_CH -->|No| ALL_LOGO[全ロゴファイルを<br/>入力<br/>logo/ ディレクトリ]
    HAS_CH -->|Yes| INSTALL{install.lgd<br/>存在?}
    INSTALL -->|Yes| USE_INSTALL([install.lgd を使用])
    INSTALL -->|No| SHORT{short.lgd<br/>存在?}
    SHORT -->|Yes| USE_SHORT([short.lgd を使用])
    SHORT -->|No| RECOGNIZE{recognize.lgd<br/>存在?}
    RECOGNIZE -->|Yes| USE_RECOGNIZE([recognize.lgd を使用])
    RECOGNIZE -->|No| SID{SID{service_id}.lgd<br/>存在?}
    SID -->|Yes| USE_SID([SID*.lgd を使用])
    SID -->|No| ALL_LOGO

    subgraph "ロゴファイル検索"
        INSTALL
        SHORT
        RECOGNIZE
        SID
    end
```

### ロゴファイル検索の詳細 (`getLogo` 関数)

1. `<name>.lgd` の存在チェック
2. `<name>.lgd2` の存在チェック
3. `<name>` が `SID` で始まる場合:
   - `<name>-1.lgd`, `<name>-2.lgd`, ... を探索
   - 最大番号のファイルを選択 (例: `SID103-3.lgd` が最新)

### ロゴ選択の優先順位 (`selectLogo` 関数)

1. `channel.install` (通常空なのでスキップされる)
2. `channel.short` (例: `BS11.lgd`)
3. `channel.recognize` (例: `ＢＳ１１イレブン.lgd`)
4. `SID{channel.service_id}` (例: `SID211.lgd` → `SID211-3.lgd`)
5. 全てに失敗 → `LOGO_PATH` ディレクトリ全体を指定

## テスト方針

- ロゴ選択: チャンネル情報に基づいて正しいロゴファイルが選択されること
- `getLogo` 関数: `.lgd`, `.lgd2`, SID 連番パターンの検索が正しく動作すること
- フォールバック: ロゴファイルが見つからない場合にディレクトリ全体が指定されること
- コマンド引数: 正しい引数配列が生成されること

## 依存モジュール

- [settings.md](./settings.md) — `BinaryPaths.logoframe`, `OutputPaths`, `JlseConfig.logo_dir`
- [channel.md](./channel.md) — `Channel` 型からロゴファイル名を決定
- [chapter_exe.md](./chapter_exe.md) — 共通コマンド実行パターン
