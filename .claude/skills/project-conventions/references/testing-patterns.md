# Testing Patterns

## Unit Test Template

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn test_descriptive_name() {
        // Arrange
        let input = "value";

        // Act
        let result = function_under_test(input);

        // Assert
        assert_eq!(result, expected);
    }
}
```

- `#![allow(clippy::unwrap_used)]` permitted in test modules only.
- Use Arrange / Act / Assert comments in each test.
- `use super::*` is the only allowed wildcard import.

## Async Test Template

```rust
#[tokio::test]
async fn test_async_operation() {
    // Arrange
    let mock = MockSyoboiApi::new(vec![batch1, batch2]);

    // Act
    let result = lookup_all_programs(&mock, &params).await.unwrap();

    // Assert
    assert_eq!(result.len(), expected_count);
}
```

## Mock Pattern

Implement traits on mock structs with pre-configured responses.
See `src/libs/syoboi/util.rs` tests for `MockSyoboiApi` example.

## Integration Test Template

File: `tests/<name>.rs`

```rust
#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

use assert_cmd::cargo_bin_cmd;
use predicates::prelude::predicate;

#[test]
fn test_cli_subcommand() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["syoboi", "prog", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--time-since"));
}
```

- Use `assert_cmd::cargo_bin_cmd!` macro, chain `.assert().success()` / `.failure()`.
- Use `predicates::str::contains()` for output content checks.

## Fixtures & HTTP Mocking

Load fixtures with `include_str!`:

```rust
const FIXTURE: &str = include_str!("../../fixtures/syoboi/title_lookup_6309.xml");
```

Use `wiremock::MockServer` for HTTP mocking:

```rust
let mock_server = wiremock::MockServer::start().await;
wiremock::Mock::given(wiremock::matchers::method("GET"))
    .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(FIXTURE))
    .mount(&mock_server).await;
```

## Miri Compatibility

For universal Miri rules and decision flowchart, see
`~/.claude/skills/rust-implementation/references/testing.md` → "Miri" section.

### Crate-Level Exclusions

| Crate     | Reason                                 | Tests |
| --------- | -------------------------------------- | ----- |
| dtvmgr-db | All tests use rusqlite (C FFI binding) | 50    |
| dtvmgr    | Binary crate — integration tests only  | N/A   |

### Per-Test Skip Categories

1. **File system (tempfile)** — 69 tests. Tests using `tempfile::tempdir()` or real file I/O. Miri has limited file system support. Affects dtvmgr-cli (14) and dtvmgr-jlse (55).
2. **FFI / C bindings (rusqlite)** — 50 tests. All dtvmgr-db tests use SQLite via C FFI. Entire crate excluded from Miri CI.
3. **Network I/O (reqwest, wiremock)** — 27 tests. HTTP client and mock server use unsupported socket syscalls. Affects dtvmgr-api (22) and dtvmgr-cli (5).
4. **Process spawning (Command)** — 19 tests. Tests that execute external tools via `std::process::Command`. Affects dtvmgr-jlse.
5. **TLS / Crypto (reqwest + rustls)** — included in Network I/O count. TLS initialization is extremely slow under Miri (~10 min/call).
6. **Regex compilation** — included in tests that indirectly trigger `regex::Regex::new()`. DFA construction under interpretation is extremely slow (~2-6 min/test).
7. **Environment variables** — 3 tests. Tests calling `std::env::set_var` or relying on `HOME`/`current_dir`. Affects dtvmgr-cli config/paths.

### Statistics

| Metric                      | Count                        |
| --------------------------- | ---------------------------- |
| Total tests                 | 499                          |
| Miri-compatible             | 329                          |
| Miri-ignored (per-test)     | 170                          |
| Miri-excluded (crate-level) | 2 crates (dtvmgr-db, dtvmgr) |

## Coverage

Target: 80%+ line coverage. Run: `mise run coverage`
