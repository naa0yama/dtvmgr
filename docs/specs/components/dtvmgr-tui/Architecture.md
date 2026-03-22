# dtvmgr-tui Architecture

## 概要

`ratatui` + `crossterm` ベースの対話型 TUI コンポーネント群。チャンネル選択、エンコードキュー管理、タイトル閲覧、正規化プレビュー、進捗表示の 5 つのビューアを提供する。

## ステータス

- **実装状態**: 完了
- **Rust クレート**: `crates/dtvmgr-tui`

## コンポーネント構成

| コンポーネント     | エントリ関数                      | 概要                                          |
| ------------------ | --------------------------------- | --------------------------------------------- |
| `channel_selector` | `run_channel_selector`            | チャンネルグループ / チャンネルの対話選択     |
| `encode_selector`  | `setup_terminal` / イベントループ | EPGStation 録画からエンコード対象を選択・設定 |
| `title_viewer`     | `run_title_viewer` (推定)         | キャッシュ済みタイトル / 番組の閲覧・除外設定 |
| `normalize_viewer` | `run_normalize_viewer` (推定)     | タイトル正規化結果のプレビューと正規表現編集  |
| `progress_viewer`  | `run_progress_viewer`             | CM 検出パイプラインのリアルタイム進捗表示     |

## 共通アーキテクチャ

各コンポーネントは同一のパターンに従う:

1. **State 構造体** - `mod state` に UI 状態を集約 (`*State` 構造体)
2. **UI 描画** - `mod ui` に `ratatui` ウィジェット描画ロジックを分離
3. **イベントループ** - `crossterm::event` でキーイベントを処理し State を更新
4. **ターミナル管理** - `enable_raw_mode` / `EnterAlternateScreen` で代替画面に切り替え、終了時に復元

## 状態管理パターン

- `InputMode` enum でモード切替 (Normal / Filter / Edit など)
- `ActivePane` enum でフォーカスペイン管理 (2 ペイン構成のビューア)
- `SelectorResult` enum で操作結果を返却 (Confirmed / Cancelled)
- `progress_viewer` は `mpsc::Receiver<ProgressEvent>` でパイプラインスレッドからイベントを受信

## 依存関係

### Rust クレート

| クレート    | 用途                           |
| ----------- | ------------------------------ |
| `ratatui`   | TUI ウィジェットフレームワーク |
| `crossterm` | ターミナル制御 / イベント入力  |
| `regex`     | タイトル正規化パターン         |

### 内部依存

| クレート      | 用途                                      |
| ------------- | ----------------------------------------- |
| `dtvmgr-db`   | `CachedTitle`, `CachedChannel` 等の型参照 |
| `dtvmgr-jlse` | `ProgressEvent` 型 (進捗ビューア)         |
