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

| Crate     | Reason                                                                         |
| --------- | ------------------------------------------------------------------------------ |
| dtvmgr-db | FFI via `rusqlite` (bundled SQLite C library); Miri cannot interpret foreign C |

> `dtvmgr-db` is the only crate excluded at the crate level in `.github/workflows/miri.yaml`.

### Per-Test Skip Categories

1. **FFI / SQLite (`rusqlite`)** — 55 tests in `dtvmgr-db`. Every test that opens a real SQLite connection is skipped because `rusqlite` calls into the bundled C library, which Miri cannot execute. Affects all modules: `connection`, `titles`, `channels`, `migrations`, `programs`, `recorded`.
2. **Async HTTP / `wiremock` (`reqwest`, `wiremock`)** — 34 tests in `dtvmgr-api` (`tmdb/client`, `syoboi/client`, `epgstation/client`). Tests spin up a `wiremock` mock server and make real `reqwest` HTTP calls; both rely on Tokio I/O and TLS code (`rustls`) that is not Miri-compatible.
3. **Process spawning / external binaries (`std::process::Command`)** — tests in `dtvmgr-jlse/command/`, `dtvmgr-tsduck/command`, `dtvmgr-vmaf`. Tests invoke `ffprobe`, `ffmpeg`, `tsduck`, `logoframe`, and similar external processes; Miri cannot cross the syscall boundary for `exec`.
4. **Temporary filesystem (`tempfile`)** — tests across `dtvmgr-jlse`, `dtvmgr-cli`, `dtvmgr-tsduck`, `dtvmgr-vmaf`. `tempfile::tempdir()` calls `mkdir` which Miri rejects under isolation mode.
5. **Regex DFA compilation (`regex`)** — tests across `dtvmgr-jlse`, `dtvmgr-tui` (`normalize_viewer/state`, `title_viewer`). The `regex` crate builds a DFA at first use; under Miri this is prohibitively slow (hours per test).
6. **CLI integration / process spawning (`assert_cmd`)** — tests in `dtvmgr-cli/tests/`. Integration tests launch the compiled binary as a subprocess via `assert_cmd`; Miri cannot fork/exec.
7. **Terminal UI rendering (`ratatui` TestBackend)** — tests across `dtvmgr-tui` (`ui`, `progress_viewer/ui`, `encode_selector/ui`, `normalize_viewer/ui`, `title_viewer/ui`). TestBackend rendering is not Miri-compatible.
8. **Miscellaneous / unlabelled** — tests across `dtvmgr-jlse` (`avs`, `output/avs`, `output/chapter`, `output/ffmpeg_filter`, `validate`, `pipeline`) and `dtvmgr-cli` (`config/config`, `config/mapping`, `main`). These also use `tempfile`, invoke subprocesses, or perform filesystem writes.

### Statistics

| Metric                      | Count |
| --------------------------- | ----- |
| Total tests                 | 1411  |
| Miri-compatible             | 932   |
| Miri-ignored (per-test)     | 398   |
| Miri-excluded (crate-level) | 81    |

> Only `dtvmgr-db` is excluded at the crate level; all other crates use per-test `#[cfg_attr(miri, ignore)]`.

## Coverage

Target: 90%+ line coverage. Run: `mise run coverage`
