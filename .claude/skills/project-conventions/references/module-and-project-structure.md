# Module & Project Structure (dtvmgr)

> **Base rules**: See `~/.claude/skills/rust-project-conventions/references/module-structure.md`
> for shared conventions (visibility, mod.rs pattern, size limits, CLI design, Clippy configuration).

## Project Source Layout

```
src/
  main.rs              # CLI entry point (clap derive)
  libs.rs              # Top-level library module
  libs/
    syoboi/            # Feature module (API client)
      mod.rs           # Re-exports
      api.rs           # API trait + implementation
      client.rs        # HTTP client + builder
      params.rs        # Query parameters
      rate_limiter.rs  # Rate limiting logic
      types.rs         # Data structures
      util.rs          # Utility functions
      xml.rs           # XML parsing
tests/
  cli_api_test.rs      # Integration tests (assert_cmd)
fixtures/
  syoboi/              # Test fixtures (XML)
ast-rules/
  *.yml                # Custom ast-grep lint rules
```

## OTel / Tracing Setup

- OTel is enabled by default (`default = ["otel"]`).
- Set `OTEL_EXPORTER_OTLP_ENDPOINT` env var to enable OTLP export (no-op when unset).
- Build without OTel: `cargo build --no-default-features`.
- Feature flag in `Cargo.toml`:
  ```toml
  [features]
  otel = [
  	"dep:opentelemetry",
  	"dep:opentelemetry_sdk",
  	"dep:opentelemetry-otlp",
  	"dep:tracing-opentelemetry",
  ]
  ```
