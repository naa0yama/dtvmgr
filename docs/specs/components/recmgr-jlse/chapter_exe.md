# chapter_exe Command Wrapper

> 親ドキュメント: [Architecture.md](./Architecture.md)

## ステータス

- **Phase**: 2
- **実装状態**: 未実装
- **Node.js ソース**: `src/command/chapterexe.js` (34行)
- **Rust モジュール**: `crates/dtvmgr-jlse/src/command/chapter_exe.rs`

## 概要

無音区間とシーンチェンジポイントを検出する外部コマンド `chapter_exe` のラッパー。入力 AVS ファイルを解析し、検出結果を `obs_chapterexe.txt` に出力する。

## 仕様

### 共通コマンド実行パターン

全外部コマンドは以下の共通パターンで実行する。他のコマンドラッパー ([logoframe.md](./logoframe.md), [join_logo_scp.md](./join_logo_scp.md), [tsdivider.md](./tsdivider.md), [ffprobe.md](./ffprobe.md), [ffmpeg.md](./ffmpeg.md)) もこのパターンを参照する。

```rust
use tokio::process::Command;

async fn run_command(program: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .status()
        .await
        .with_context(|| format!("failed to spawn {program}"))?;
    if !status.success() {
        anyhow::bail!("{program} exited with code {}", status.code().unwrap_or(-1));
    }
    Ok(())
}
```

### 引数構築

```
chapter_exe -v <avs_file> -s 8 -e 4 -o <chapterexe_output>
```

| 引数 | 値                   | 説明               |
| ---- | -------------------- | ------------------ |
| `-v` | `in_org.avs`         | 入力 AVS ファイル  |
| `-s` | `8`                  | 無音判定の感度     |
| `-e` | `4`                  | シーンチェンジ感度 |
| `-o` | `obs_chapterexe.txt` | 出力ファイル       |

### stderr 処理

`Creating` で始まる行は AviSynth の初期化メッセージとしてログ出力。

## テスト方針

- 引数構築: 正しい引数配列が生成されること
- コマンド実行は統合テストで外部バイナリをモックして検証

## 依存モジュール

- [settings.md](./settings.md) — `BinaryPaths.chapter_exe`, `OutputPaths.input_avs`, `OutputPaths.chapterexe_output`
