# TS Seek & Chunk Extraction

> 親ドキュメント: [Architecture.md](./Architecture.md)

## ステータス

- **実装状態**: 実装中
- **Rust モジュール**: `crates/dtvmgr-tsduck/src/seek.rs`

## 概要

MPEG-TS 録画ファイルの中間地点から TS パケット境界にアラインされたチャンクを抽出する。Amatsukaze 方式の録画対象検出で使用し、ファイル中間の EIT p/f から録画対象番組を特定する。

## 背景: なぜ中間を読むのか

録画ファイルには、録画開始/終了のタイミングにより最大3番組の EIT が含まれる:

```
TS ファイル
┌──────────┬──────────────────────────────┬──────────┐
│ 前番組の末尾 │     録画対象の番組(本体)       │ 後番組の冒頭 │
└──────────┴──────────────────────────────┴──────────┘
  ← 数秒 →   ← ─────── 大部分 ─────── →   ← 数秒 →
```

ファイルの中間地点はほぼ確実に録画対象番組の放送中であるため、この位置から EIT p/f を抽出すれば `running_status` で対象を特定できる。

## 仕様

### 定数

| 定数                 | 値                          | 説明                                 |
| -------------------- | --------------------------- | ------------------------------------ |
| `TS_PACKET_SIZE`     | `188` (bytes)               | MPEG-TS パケットサイズ               |
| `TS_SYNC_BYTE`       | `0x47`                      | パケット先頭の同期バイト             |
| `DEFAULT_CHUNK_SIZE` | `10 * 1024 * 1024` (10 MiB) | 抽出チャンクサイズ (≈4.7秒 @17Mbps)  |
| `MIN_FILE_SIZE`      | `1024 * 1024` (1 MiB)       | 中間シークの最小ファイルサイズ       |
| `SYNC_VERIFY_COUNT`  | `8`                         | パケット境界検証に必要な連続一致回数 |

### パブリック API

```rust
/// Extract an aligned TS chunk from the middle of a file.
///
/// For files smaller than `MIN_FILE_SIZE`, returns the entire file contents.
/// For larger files, seeks to the midpoint, finds a valid packet boundary,
/// and extracts `chunk_size` bytes of aligned TS data.
pub fn extract_middle_chunk(input_file: &Path, chunk_size: u64) -> Result<Vec<u8>>;
```

### パケット境界検出アルゴリズム

TS パケットは 188 バイト固定長で、各パケットの先頭は `0x47` (sync byte):

```
パケット1 (188B)    パケット2 (188B)    パケット3 (188B)
[47 xx xx ... xx] [47 xx xx ... xx] [47 xx xx ... xx]
```

ファイルの任意位置にシークした後、正しいパケット境界を見つける手順:

1. 現在位置から `0x47` を前方探索
2. そこから 188 バイト間隔で `SYNC_VERIFY_COUNT` (8) 回連続して `0x47` が出現するか検証
3. 連続一致しなかった場合、次の `0x47` から再試行
4. 検証成功 → その位置をパケット境界として確定

```
偶然の 0x47    本物の sync byte
     ↓              ↓
... 47 xx xx ... [47 xx xx ...][47 xx xx ...][47 xx xx ...]
                  ← 188B →    ← 188B →    ← 188B →
                  ✓ 8回連続 0x47 → ここが境界
```

### 処理フロー

1. ファイルサイズ取得。`< MIN_FILE_SIZE` なら全体を `Vec<u8>` として返却
2. `mid = file_size / 2`, `start = mid.saturating_sub(chunk_size / 2)`
3. `start` 位置にシークし、パケット境界検出を実行
4. アライメント済みオフセットから `chunk_size` バイトを読み取り
5. 末尾の不完全パケット (188 で割り切れない端数) を切り捨て

## テスト方針

- 正常: 中間チャンク抽出、パケット境界アライメント検証
- 小ファイル: `MIN_FILE_SIZE` 未満で全体返却
- 空ファイル: エラーまたは空 `Vec`
- 壊れた TS: sync byte が見つからない場合のエラー
- パケット境界検出: 内部関数 `find_packet_boundary` の単体テスト
- Miri: `tempfile` を使うテストは `#[cfg_attr(miri, ignore)]` で除外

## 依存モジュール

なし (基盤モジュール)
