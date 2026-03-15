# Testing Patterns (dtvmgr)

> **Base rules**: See `~/.claude/skills/rust-coding/references/testing-templates.md`
> for shared templates (unit test, async test, mock pattern, integration test, fixtures & HTTP mocking).

## Executable Script Tests (ETXTBSY on overlayfs)

This project uses a `write_script` helper to avoid `ETXTBSY` when writing and executing
scripts in tests on overlayfs-backed environments (Docker, CI).

### Helper locations

- `crates/dtvmgr-jlse/src/command/mod.rs` → `pub fn write_script` (in `#[cfg(test)] mod tests`)
- `crates/dtvmgr-tsduck/src/command.rs` → `pub fn write_script` (in `#[cfg(test)] mod tests`)

### Reference

For root cause analysis, reusable template, and NG patterns, see
`~/.claude/skills/rust-implementation/references/testing.md` → "ETXTBSY on overlayfs" section.

## Miri Compatibility

For universal Miri rules and decision flowchart, see
`~/.claude/skills/rust-implementation/references/testing.md` → "Miri" section.

### Crate-Level Exclusions

| Crate     | Reason                                                   | Tests |
| --------- | -------------------------------------------------------- | ----- |
| dtvmgr-db | FFI — all tests use rusqlite (bundled SQLite C bindings) | 50    |

### Per-Test Skip Categories

1. **File system (`tempfile` / `dirs`)** — 57 tests. Miri cannot perform real filesystem I/O (`mkdir`, `write`, `read_dir`, `canonicalize`). Tests in `dtvmgr-jlse` (avs, pipeline, settings, output/\*, command/logoframe, command/chapter\_exe, channel, param) and `dtvmgr-cli` (config/config, config/mapping, config/paths, main) use `tempfile::tempdir()` or `dirs::config_dir()`.
2. **Process spawning (`assert_cmd` / `std::process::Command`)** — 28 tests. Miri does not support `fork`/`exec`. Integration tests in `dtvmgr-cli` (cli\_subcommands\_test, cli\_syoboi\_test) use `assert_cmd`, and `dtvmgr-jlse` (command/mod) tests use `std::process::Command` directly.
3. **Network I/O (`wiremock` / `reqwest`)** — 22 tests. Miri cannot open sockets or perform TLS handshakes. All HTTP client tests in `dtvmgr-api` (tmdb/client, syoboi/client) build `reqwest` clients and/or spin up `wiremock` mock servers.
4. **Regex DFA compilation (`regex`)** — 11 tests. Regex DFA compilation is prohibitively slow under Miri's interpreter. Channel detection tests in `dtvmgr-jlse` (channel) and param detection tests (param) compile regex patterns at runtime.
5. **Clock syscall (`chrono::Utc::now`)** — 6 tests. `Utc::now()` issues a clock syscall unsupported by Miri. Cooldown and time-range tests in `dtvmgr-cli` (main) and `dtvmgr-api` (syoboi/params).
6. **FFI / `rusqlite`** — 3 tests. Tests in `dtvmgr-cli` (main) call `dtvmgr_db::open_db` which invokes SQLite FFI through `rusqlite`.
7. **Environment variables (`set_var` / `remove_var`)** — 2 tests. `std::env::set_var` and `remove_var` are unsafe under Miri with `-Zmiri-disable-isolation` due to potential data races. Tests in `dtvmgr-cli` (main).

### Statistics

| Metric                      | Count |
| --------------------------- | ----- |
| Total tests                 | 499   |
| Miri-compatible             | 322   |
| Miri-ignored (per-test)     | 127   |
| Miri-excluded (crate-level) | 50    |

## Coverage

Target: 80%+ line coverage. Run: `mise run coverage`
