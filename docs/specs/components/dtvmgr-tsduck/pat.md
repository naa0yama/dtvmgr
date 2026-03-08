# PAT Parser

> 親ドキュメント: [Architecture.md](./Architecture.md)

## ステータス

- **実装状態**: 完了
- **Rust モジュール**: `crates/dtvmgr-tsduck/src/pat.rs`

## 概要

`TSDuck` の `tstables` が出力する PAT (Program Association Table) XML をパースし、録画対象のサービス ID を自動検出する。PAT は TS ストリームに含まれる全サービスの一覧を保持しており、先頭のサービス ID を録画ターゲットとして使用する。

## 仕様

### XML 構造

`tstables --pid 0 --xml-output -` の出力フォーマット:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<tsduck>
  <PAT transport_stream_id="0x7FE9" version="3">
    <service service_id="0x5C38" program_map_PID="0x0101"/>
    <service service_id="0x5C39" program_map_PID="0x0102"/>
  </PAT>
</tsduck>
```

### データ仕様

#### XML デシリアライズ型 (非公開)

| 型             | 用途                   |
| -------------- | ---------------------- |
| `TsduckPatXml` | ルート `<tsduck>` 要素 |
| `PatTable`     | `<PAT>` テーブル       |
| `PatService`   | `<service>` エントリ   |

### パブリック API

```rust
/// Extract the first service ID from PAT in TSDuck XML.
/// Returns `None` if no PAT table or service entry exists.
pub fn parse_pat_first_service_id(xml: &str) -> Result<Option<u32>>;
```

### 動作仕様

1. XML を `TsduckPatXml` にデシリアライズ
2. 全 PAT テーブルを走査し、最初に見つかった `<service>` の `service_id` を返却
3. PAT テーブルが存在しない、またはサービスが空の場合は `None` を返却
4. サービス ID のパースには `eit::parse_sid` を共用 (10 進数 / 16 進数対応)

### CLI での使用

`--sid` も `--channel` も指定されない場合に、PAT から録画対象のサービス ID を自動検出するフォールバックとして使用:

```
dtvmgr jlse tsduck -i recording.ts
```

→ PAT の先頭サービス ID で EIT をフィルタリング

## テスト方針

- 単一サービス: 正しい SID が返ること
- 複数サービス: 先頭の SID が返ること
- PAT なし: `None` が返ること
- 空 PAT: `None` が返ること
- 16 進数 / 10 進数 SID: 両形式が正しくパースされること

## 依存モジュール

- [eit.md](./eit.md) — `parse_sid` 関数を共用
- [command.md](./command.md) — XML 文字列を提供
