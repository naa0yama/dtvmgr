# EIT Parser

> 親ドキュメント: [Architecture.md](./Architecture.md)

## ステータス

- **実装状態**: 完了
- **Rust モジュール**: `crates/dtvmgr-tsduck/src/eit.rs`

## 概要

`TSDuck` の `tstables` が出力する EIT (Event Information Table) XML をパースし、番組情報の構造体 `ProgramInfo` に変換する。サービス ID によるフィルタリング、重複除去機能を提供する。

## 仕様

### XML 構造

`tstables --pid 0x12 --xml-output -` の出力フォーマット:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<tsduck>
  <EIT service_id="65024" transport_stream_id="10153"
       original_network_id="12345" type="pf">
    <event event_id="1001" start_time="2024-12-31 15:00:00"
           duration="00:06:00" running_status="running">
      <short_event_descriptor language_code="jpn">
        <event_name>番組タイトル</event_name>
        <text>番組説明</text>
      </short_event_descriptor>
    </event>
  </EIT>
</tsduck>
```

### データ仕様

#### XML デシリアライズ型

| 型                     | 用途                              | 可視性 |
| ---------------------- | --------------------------------- | ------ |
| `TsduckXml`            | ルート `<tsduck>` 要素            | `pub`  |
| `Table`                | `<EIT>` テーブル                  | `pub`  |
| `Event`                | `<event>` イベント                | `pub`  |
| `ShortEventDescriptor` | `<short_event_descriptor>` 記述子 | `pub`  |

#### `ProgramInfo` (出力型)

| フィールド       | 型               | 説明               | 備考                                           |
| ---------------- | ---------------- | ------------------ | ---------------------------------------------- |
| `service_id`     | `u32`            | サービス ID (数値) | 16 進数 / 10 進数から変換                      |
| `event_id`       | `String`         | イベント ID        |                                                |
| `start_time`     | `String`         | 開始時刻           | `"YYYY-MM-DD HH:MM:SS"`                        |
| `duration_min`   | `u32`            | 時間 (分)          | 秒は切り捨て                                   |
| `duration_raw`   | `String`         | 時間 (生値)        | `"HH:MM:SS"`                                   |
| `running_status` | `String`         | 放送状態           | `"running"`, `"not-running"`, `"undefined"`    |
| `program_name`   | `Option<String>` | 番組名             | `short_event_descriptor` の最初の `event_name` |
| `table_type`     | `Option<String>` | テーブル種別       | `"pf"` (present/following) or `"schedule"`     |

### Rust 型定義

```rust
/// Parsed program information extracted from EIT data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramInfo {
    pub service_id: u32,
    pub event_id: String,
    pub start_time: String,
    pub duration_min: u32,
    pub duration_raw: String,
    pub running_status: String,
    pub program_name: Option<String>,
    pub table_type: Option<String>,
}
```

### パブリック API

```rust
/// Parse all EIT events from TSDuck XML output.
pub fn parse_eit_xml(xml: &str) -> Result<Vec<ProgramInfo>>;

/// Parse EIT events filtered by target service ID.
pub fn parse_eit_xml_by_sid(xml: &str, target_sid: &str) -> Result<Vec<ProgramInfo>>;

/// Load and parse EIT XML from a file.
pub fn load(path: &Path) -> Result<Vec<ProgramInfo>>;

/// Load and parse EIT XML from a file, filtered by service ID.
pub fn load_by_sid(path: &Path, target_sid: &str) -> Result<Vec<ProgramInfo>>;

/// Parse `HH:MM:SS` duration string to total minutes.
pub fn parse_duration_to_min(duration: &str) -> Result<u32>;

/// Deduplicate programs by `(service_id, event_id)` pair.
pub fn dedup_programs(programs: Vec<ProgramInfo>) -> Vec<ProgramInfo>;
```

### サービス ID パース

`parse_sid` (`pub(crate)`) は 10 進数と 16 進数 (`0x` / `0X` プレフィックス) の両形式に対応:

| 入力       | 出力    |
| ---------- | ------- |
| `"65024"`  | `65024` |
| `"0xFE00"` | `65024` |
| `"0X5C38"` | `23608` |

### 重複除去

`dedup_programs` は `(service_id, event_id)` のペアで重複を判定し、最初の出現を保持する。EIT では `pf` テーブルと `schedule` テーブルに同一イベントが含まれることがあるため、この処理が必要。

### 録画対象検出

```rust
/// Detected recording target with detection method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingTarget {
    pub program: ProgramInfo,
    pub detection_method: DetectionMethod,
}

/// How the recording target was detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectionMethod {
    RunningStatus,   // running_status == "running" in p/f table
    FirstPfEvent,    // first event in p/f table (no running status)
    FirstEvent,      // first event overall (no p/f tables)
}

/// Detect the recording target from middle-of-file EIT programs.
pub fn detect_recording_target(programs: &[ProgramInfo]) -> Option<RecordingTarget>;
```

#### 検出優先度

1. **`RunningStatus`**: `table_type == "pf"` かつ `running_status == "running"` のイベント。最も信頼性が高い
2. **`FirstPfEvent`**: `running` がないが `table_type == "pf"` のテーブルが存在する場合、その先頭イベント
3. **`FirstEvent`**: p/f テーブルが存在しない場合、全イベントの先頭 (フォールバック)

#### ロジック詳細

ファイル中間から抽出した EIT p/f には通常、「今放送中の番組」と「次の番組」の2イベントが含まれる。`running_status` が `"running"` のイベントが録画時点で実際に放送中だった番組であり、これが録画対象。

## テスト方針

- `parse_duration_to_min`: 各種フォーマット (30 分、1 時間、0 分、不正値)
- `parse_sid`: 10 進数、16 進数 (小文字 / 大文字プレフィックス)、不正値
- `parse_eit_xml`: 基本パース、16 進数 SID、複数イベント、空テーブル、`running_status` 欠損
- `parse_eit_xml_by_sid`: SID フィルタ、不一致、16 進数ターゲット
- `load` / `load_by_sid`: ファイル I/O (Miri 除外)
- `dedup_programs`: 重複除去、異 SID の保持、空入力
- `detect_recording_target`: running/first_pf/first/empty/multiple パターン

## 依存モジュール

- [command.md](./command.md) — XML 文字列を提供
