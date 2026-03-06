# EIT Parser

> 親ドキュメント: [Architecture.md](./Architecture.md)

## ステータス

- **実装状態**: 完了
- **Rust モジュール**: `crates/dtvmgr-tsduck/src/eit.rs`

## 概要

`TSDuck` の `tstables` が出力する EIT (Event Information Table) XML をパースし、番組情報の構造体 `ProgramInfo` に変換する。サービス ID によるフィルタリング、重複除去機能を提供する。EPGStation 相当の番組情報(番組名、概要、出演者、あらすじ、ジャンル、映像/音声属性、duration)を TS ファイルから直接抽出できる。

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
      <extended_event_descriptor descriptor_number="0" last_descriptor_number="0" language_code="jpn">
        <item>
          <description>出演者</description>
          <name>出演者名</name>
        </item>
      </extended_event_descriptor>
      <content_descriptor>
        <content content_nibble_level_1="7" content_nibble_level_2="3"/>
      </content_descriptor>
      <component_descriptor stream_content="1" component_type="179" language_code="jpn">
        <text>1080i</text>
      </component_descriptor>
      <audio_component_descriptor stream_content="2" component_type="3" sampling_rate="7" language_code="jpn"/>
    </event>
  </EIT>
</tsduck>
```

### データ仕様

#### XML デシリアライズ型

| 型                         | 用途                                    | 可視性 |
| -------------------------- | --------------------------------------- | ------ |
| `TsduckXml`                | ルート `<tsduck>` 要素                  | `pub`  |
| `Table`                    | `<EIT>` テーブル                        | `pub`  |
| `Event`                    | `<event>` イベント                      | `pub`  |
| `ShortEventDescriptor`     | `<short_event_descriptor>` 記述子       | `pub`  |
| `ExtendedEventDescriptor`  | `<extended_event_descriptor>` 記述子    | `pub`  |
| `ExtendedEventItem`        | extended event 内の item                | `pub`  |
| `ContentDescriptor`        | `<content_descriptor>` ジャンル分類     | `pub`  |
| `ContentEntry`             | content 内の分類エントリ                | `pub`  |
| `ComponentDescriptor`      | `<component_descriptor>` 映像属性       | `pub`  |
| `AudioComponentDescriptor` | `<audio_component_descriptor>` 音声属性 | `pub`  |

#### `ProgramInfo` (出力型)

| フィールド                 | 型                      | 説明                         | 備考                                           |
| -------------------------- | ----------------------- | ---------------------------- | ---------------------------------------------- |
| `service_id`               | `u32`                   | サービス ID (数値)           | 16 進数 / 10 進数から変換                      |
| `event_id`                 | `String`                | イベント ID                  |                                                |
| `start_time`               | `String`                | 開始時刻                     | `"YYYY-MM-DD HH:MM:SS"`                        |
| `duration_sec`             | `u32`                   | 時間 (秒)                    | 秒単位の正確な duration                        |
| `duration_raw`             | `String`                | 時間 (生値)                  | `"HH:MM:SS"`                                   |
| `running_status`           | `String`                | 放送状態                     | `"running"`, `"not-running"`, `"undefined"`    |
| `program_name`             | `Option<String>`        | 番組名                       | `short_event_descriptor` の最初の `event_name` |
| `description`              | `Option<String>`        | 番組説明                     | `short_event_descriptor` の最初の `text`       |
| `table_type`               | `Option<String>`        | テーブル種別                 | `"pf"` (present/following) or `"schedule"`     |
| `raw_extended`             | `Vec<(String, String)>` | 拡張情報 KV                  | EIT 挿入順序を保持                             |
| `genre1`                   | `Option<u8>`            | 大ジャンル                   | content nibble level 1                         |
| `sub_genre1`               | `Option<u8>`            | サブジャンル                 | content nibble level 2                         |
| `video_stream_content`     | `Option<u8>`            | 映像 stream type             | component descriptor                           |
| `video_component_type`     | `Option<u8>`            | 映像 component type          | component descriptor                           |
| `audio_component_type`     | `Option<u8>`            | 音声 component type          | audio component descriptor                     |
| `audio_sampling_rate_code` | `Option<u8>`            | 音声サンプリングレートコード | ARIB STD-B10 コード値                          |

### EPGStation フィールドマッピング

| EPGStation フィールド  | EIT ソース                                           |
| ---------------------- | ---------------------------------------------------- |
| `name`                 | `short_event_descriptor/event_name`                  |
| `description`          | `short_event_descriptor/text`                        |
| `extended`             | `extended_event_descriptor` items 結合               |
| `rawExtended`          | `extended_event_descriptor` item pairs               |
| `genre1` / `subGenre1` | `content_descriptor` nibble levels                   |
| `videoStreamContent`   | `component_descriptor/@stream_content`               |
| `videoComponentType`   | `component_descriptor/@component_type`               |
| `videoResolution`      | component_type 上位 4bit デコード                    |
| `audioComponentType`   | `audio_component_descriptor/@component_type`         |
| `audioSamplingRate`    | `audio_component_descriptor/@sampling_rate` デコード |
| `duration` (sec)       | `event/@duration` パース                             |

### Rust 型定義

```rust
/// Parsed program information extracted from EIT data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramInfo {
    pub service_id: u32,
    pub event_id: String,
    pub start_time: String,
    pub duration_sec: u32,
    pub duration_raw: String,
    pub running_status: String,
    pub program_name: Option<String>,
    pub description: Option<String>,
    pub table_type: Option<String>,
    pub raw_extended: Vec<(String, String)>,
    pub genre1: Option<u8>,
    pub sub_genre1: Option<u8>,
    pub video_stream_content: Option<u8>,
    pub video_component_type: Option<u8>,
    pub audio_component_type: Option<u8>,
    pub audio_sampling_rate_code: Option<u8>,
}

impl ProgramInfo {
    /// Duration in minutes (truncates seconds).
    pub fn duration_min(&self) -> u32;
    /// Decoded video resolution (e.g. `"1080i"`). Derived from `video_component_type`.
    pub fn video_resolution(&self) -> Option<&'static str>;
    /// Decoded audio sampling rate in Hz. Derived from `audio_sampling_rate_code`.
    pub fn audio_sampling_rate(&self) -> Option<u32>;
    /// EPGStation-compatible extended text. Derived from `raw_extended`.
    pub fn extended(&self) -> Option<String>;
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

/// Parse `HH:MM:SS` duration string to total seconds.
pub fn parse_duration_to_sec(duration: &str) -> Result<u32>;

/// Parse `HH:MM:SS` duration string to total minutes (seconds truncated).
pub fn parse_duration_to_min(duration: &str) -> Result<u32>;

/// Decode ARIB STD-B10 audio sampling rate code to Hz.
pub fn decode_sampling_rate(code: u8) -> Option<u32>;

/// Decode ARIB STD-B10 component type to video resolution string.
pub fn decode_video_resolution(component_type: u8) -> Option<&'static str>;

/// Build extended text and raw key-value pairs from extended event descriptors.
pub fn build_extended_fields(descriptors: &[ExtendedEventDescriptor]) -> (Option<String>, Vec<(String, String)>);

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

### デコードヘルパー

#### 映像解像度デコード (`decode_video_resolution`)

`component_type` の上位 4bit から解像度文字列を返す (ARIB STD-B10):

| 上位 4bit | 解像度  |
| --------- | ------- |
| `0x0`     | `480i`  |
| `0x9`     | `2160p` |
| `0xA`     | `480p`  |
| `0xB`     | `1080i` |
| `0xC`     | `720p`  |
| `0xD`     | `240p`  |
| `0xE`     | `1080p` |

#### 音声サンプリングレートデコード (`decode_sampling_rate`)

ARIB STD-B10 コードから Hz 値を返す:

| コード | Hz    |
| ------ | ----- |
| 1      | 16000 |
| 2      | 22050 |
| 3      | 24000 |
| 5      | 32000 |
| 6      | 44100 |
| 7      | 48000 |

### 拡張情報テキスト構築 (`build_extended_fields`)

`extended_event_descriptor` の item リストから EPGStation 互換のテキストと生 KV マップを構築する:

- `descriptor_number` 順にソート
- 空 `description` の item は前の item の値に結合 (EIT 継続セマンティクス)
- 同一キーは値を結合
- 挿入順序を保持 (`Vec<(String, String)>`)
- テキスト形式: `◇{key}\n{value}` (キーが既に `◇` で始まる場合は二重付与しない)

## テスト方針

- `parse_duration_to_sec`: 秒変換 4 パターン (30min, 1h, 30sec, mixed)
- `parse_duration_to_min`: 各種フォーマット (30 分、1 時間、0 分、不正値)
- `parse_sid`: 10 進数、16 進数 (小文字 / 大文字プレフィックス)、不正値
- `parse_eit_xml`: 基本パース、16 進数 SID、複数イベント、空テーブル、`running_status` 欠損
- `parse_eit_xml` (全デスクリプタ): 全フィールド検証、複数 extended 結合、オプション descriptor なし
- `parse_eit_xml_by_sid`: SID フィルタ、不一致、16 進数ターゲット
- `load` / `load_by_sid`: ファイル I/O (Miri 除外)
- `dedup_programs`: 重複除去、異 SID の保持、空入力
- `detect_recording_target`: running/first_pf/first/empty/multiple パターン
- `decode_sampling_rate`: 既知コード (48000, 32000)、未知コード (0)
- `decode_video_resolution`: 既知 type (1080i, 720p)、未知 type
- `build_extended_fields`: 空入力、重複キー結合、`◇` プレフィックス二重付与防止

## 依存モジュール

- [command.md](./command.md) — XML 文字列を提供
