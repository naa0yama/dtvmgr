# dtvmgr-tsduck Architecture

> 親ドキュメント: [IMPROVEMENT_PLAN.md](../../IMPROVEMENT_PLAN.md)
>
> 関連ドキュメント:
>
> - [command.md](./command.md)
> - [eit.md](./eit.md)
> - [pat.md](./pat.md)
> - [seek.md](./seek.md)

## 概要

MPEG-TS 録画ファイルから番組情報を抽出するクレート。外部ツール `TSDuck` の `tstables` / `tsp` コマンドをラップし、出力される XML を構造化データにパースする。

## ステータス

- **実装状態**: 完了
- **Rust クレート**: `crates/dtvmgr-tsduck`

## モジュール構成

| モジュール | 責務                                                       |
| ---------- | ---------------------------------------------------------- |
| `command`  | `TSDuck` 外部コマンドの実行とキャプチャ (stdin パイプ対応) |
| `eit`      | EIT (Event Information Table) XML パース & 録画対象検出    |
| `pat`      | PAT (Program Association Table) XML パース                 |
| `seek`     | TS ファイル中間チャンク抽出 (パケット境界アライメント)     |

## 処理フロー

### 録画対象検出 (Amatsukaze 方式)

ファイル中間から EIT p/f を抽出し、`running_status` で録画対象を特定する:

```mermaid
flowchart TD
    A["入力 TS ファイル"] --> B["seek::extract_middle_chunk\n(中間 10 MiB)"]
    B --> C["command::extract_eit_from_chunk\n(stdin パイプ, TID 0x4E)"]
    C --> D["eit::parse_eit_xml"]
    D --> E["eit::detect_recording_target"]
    E --> F{"検出方法"}
    F -- "running_status=running" --> G["RunningStatus"]
    F -- "p/f 先頭イベント" --> H["FirstPfEvent"]
    F -- "全体先頭イベント" --> I["FirstEvent"]
```

### 全体フロー

```mermaid
flowchart TD
    A["入力 TS ファイル"] --> B{" SID 指定あり? "}
    B -- "--sid / --channel" --> C["command::extract_eit\n(PID 0x12)"]
    B -- "なし" --> D["command::extract_pat\n(PID 0)"]
    D --> E["pat::parse_pat_first_service_id"]
    E --> F["command::extract_eit\n(PID 0x12)"]
    C --> G["eit::parse_eit_xml_by_sid"]
    F --> H["eit::parse_eit_xml"]
    H --> I{" PAT SID あり? "}
    I -- "あり" --> J["service_id でフィルタ"]
    I -- "なし" --> K["全イベント返却"]
    G --> L["eit::dedup_programs"]
    J --> L
    K --> L
    L --> M["Vec&lt;ProgramInfo&gt;"]
    A --> N["録画対象検出\n(中間探索)"]
    N --> O["RecordingTarget"]
    O --> P["event_id 一致に\n[recording_target] マーカー"]
    M --> P
```

## 依存関係

### 外部ツール

| バイナリ   | 用途                       | PID                     |
| ---------- | -------------------------- | ----------------------- |
| `tstables` | テーブル XML 抽出          | `0` (PAT), `0x12` (EIT) |
| `tsp`      | サービス ID フィルタリング | —                       |

### Rust クレート

| クレート    | 用途                              |
| ----------- | --------------------------------- |
| `anyhow`    | エラーハンドリング                |
| `quick-xml` | XML デシリアライズ (`serde` 連携) |
| `serde`     | デシリアライズフレームワーク      |
| `tracing`   | コマンド実行ログ                  |

### 内部依存

- `dtvmgr-cli` がこのクレートを `jlse tsduck` サブコマンドで利用
- `dtvmgr-jlse` のチャンネル検出 (`channel.rs`) と連携して SID を解決

## テスト方針

- **引数構築**: 各 `build_*_args` 関数が正しい引数配列を返すこと
- **XML パース**: EIT / PAT の各パターン (10 進数 / 16 進数 SID、複数イベント、空テーブル) をインライン XML でテスト
- **コマンド実行**: シェルスクリプトモックで成功 / 失敗パスを検証
- **Miri**: コマンド実行テストと `tempfile` テストは `#[cfg_attr(miri, ignore)]` で除外。純粋パースロジックは Miri 互換
