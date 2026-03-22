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

| Crate         | Reason                                                                                                  | Tests |
| ------------- | ------------------------------------------------------------------------------------------------------- | ----- |
| dtvmgr-db     | FFI via `rusqlite` (bundled SQLite C library); Miri cannot interpret foreign C                          | 61    |
| dtvmgr-tsduck | All tests use `tempfile` (filesystem I/O) or spawn subprocesses; no pure-Rust logic tested in isolation | 97    |

> Neither crate appears in the `matrix.crate` list in `.github/workflows/miri.yaml`.

### Per-Test Skip Categories

1. **FFI / SQLite (`rusqlite`)** — 55 tests in `dtvmgr-db`. Every test that opens a real SQLite connection is skipped because `rusqlite` calls into the bundled C library, which Miri cannot execute. Affects all modules: `connection`, `titles`, `channels`, `migrations`, `programs`, `recorded`.
2. **Async HTTP / `wiremock` (`reqwest`, `wiremock`)** — 34 tests in `dtvmgr-api` (`tmdb/client`, `syoboi/client`, `epgstation/client`). Tests spin up a `wiremock` mock server and make real `reqwest` HTTP calls; both rely on Tokio I/O and TLS code (`rustls`) that is not Miri-compatible.
3. **Process spawning / external binaries (`std::process::Command`)** — 36 tests in `dtvmgr-jlse/command/` (`mod`, `ffprobe`, `logoframe`, `chapter_exe`, `join_logo_scp`). Tests invoke `ffprobe`, `logoframe`, and similar external processes; Miri cannot cross the syscall boundary for `exec`.
4. **Temporary filesystem (`tempfile`)** — 30 tests across `dtvmgr-jlse` (`settings`, `param`, `channel`, `pipeline`, `command/*`) and `dtvmgr-cli` (`config/paths`, `main`). `tempfile::tempdir()` calls `mkdir` which Miri rejects under isolation mode.
5. **Regex DFA compilation (`regex`)** — 25 tests across `dtvmgr-jlse` (`param`, `channel`). The `regex` crate builds a DFA at first use; under Miri this is prohibitively slow (hours per test).
6. **CLI integration / process spawning (`assert_cmd`)** — 25 tests in `dtvmgr-cli/tests/` (`cli_subcommands_test`, `cli_syoboi_test`). Integration tests launch the compiled binary as a subprocess via `assert_cmd`; Miri cannot fork/exec.
7. **Miscellaneous / unlabelled** — 51 tests across `dtvmgr-jlse` (`avs`, `output/avs`, `output/chapter`, `output/ffmpeg_filter`, `validate`, `pipeline`) and `dtvmgr-cli` (`config/config`, `config/mapping`, `main`). These also use `tempfile`, invoke subprocesses, or perform filesystem writes.

### Statistics

| Metric                      | Count |
| --------------------------- | ----- |
| Total tests                 | 897   |
| Miri-compatible             | 483   |
| Miri-ignored (per-test)     | 256   |
| Miri-excluded (crate-level) | 158   |

#### Per-Crate Breakdown

| Crate         | Total tests | Miri-ignored (per-test) | Miri-excluded (crate) |
| ------------- | ----------- | ----------------------- | --------------------- |
| dtvmgr-db     | 61          | 55                      | 61 (excluded)         |
| dtvmgr-jlse   | 341         | 91                      | —                     |
| dtvmgr-api    | 107         | 34                      | —                     |
| dtvmgr-cli    | 291         | 53                      | —                     |
| dtvmgr-tsduck | 97          | 23                      | 97 (excluded)         |
| **Total**     | **897**     | **256**                 | **158**               |

## Coverage

Target: 80%+ line coverage. Run: `mise run coverage`
