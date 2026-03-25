# Changelog

## [v0.3.6](https://github.com/naa0yama/dtvmgr/compare/v0.3.5...v0.3.6) - 2026-03-25

### Documentation 🗒️

- refactor(cli): suppress noisy 3rd-party logs in default EnvFilter by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/160

## [v0.3.5](https://github.com/naa0yama/dtvmgr/compare/v0.3.4...v0.3.5) - 2026-03-25

### Documentation 🗒️

- feat(pipeline): cache VMAF quality search result by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/158

## [v0.3.4](https://github.com/naa0yama/dtvmgr/compare/v0.3.3...v0.3.4) - 2026-03-25

### Development Environment 🔧

- ci(miri): add per-crate dynamic matrix and source-only trigger by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/155

### Other Changes

- refactor(pipeline): centralise stream specifier normalisation by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/156

## [v0.3.3](https://github.com/naa0yama/dtvmgr/compare/v0.3.2...v0.3.3) - 2026-03-24

### Development Environment 🔧

- chore(deps): update taiki-e/install-action action to v2.68.35 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/154

### Other Changes

- fix(pipeline): preserve stream specifier in inject_quality_override by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/151
- feat(command): capture stderr and emit OTel exception events by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/153

## [v0.3.2](https://github.com/naa0yama/dtvmgr/compare/v0.3.1...v0.3.2) - 2026-03-24

### Documentation 🗒️

- fix(ffmpeg): prepend format=nv12 before hwupload for SW-decoded input by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/149

## [v0.3.1](https://github.com/naa0yama/dtvmgr/compare/v0.3.0...v0.3.1) - 2026-03-23

### Documentation 🗒️

- fix(vmaf): append format= after hwdownload in reference filter by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/123
- refactor(jlse): add OTel resource attributes and remove unused CLI flags by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/130

### Dependency Updates 📦

- chore(deps): lock file maintenance by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/127
- chore(deps): update rust crate rusqlite to 0.39 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/120

### Development Environment 🔧

- chore(deps): update taiki-e/install-action action to v2.68.32 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/124
- fix(tagpr): format CHANGELOG.md after tagpr generates it by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/126
- chore(deps): update dependency dprint to v0.53.0 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/129
- chore(deps): update taiki-e/install-action action to v2.68.33 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/132
- fix(ci): exclude mise.toml from rust change detection by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/135
- chore(deps): update dependency aqua:ast-grep/ast-grep to v0.42.0 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/133
- fix(security): isolate bearer token and rename CI jobs by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/136
- refactor(ci): migrate workflows to github-script by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/137
- fix(ci): avoid fromJSON on empty tagpr pull_request output by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/138
- chore(deps): update github/codeql-action action to v4.33.0 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/139
- chore(deps): update taiki-e/install-action action to v2.68.34 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/146
- fix(security): remove remaining tainted data from error paths and add CodeQL infra by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/145

### Other Changes

- refactor(otel): align with updated semantic conventions by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/131
- fix(otel): sanitize reqwest error logging in TMDB client by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/134
- fix(security): prevent cleartext logging of sensitive data by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/140
- fix(security): break CWE-532 taint chain and improve API error diagnostics by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/143
- refactor(tmdb): simplify over-hardened CWE-532 error handling by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/148

## [v0.3.0](https://github.com/naa0yama/dtvmgr/compare/v0.2.6...v0.3.0) - 2026-03-22

### Development Environment 🔧

- fix(tagpr): update Cargo.lock via workflow step by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/121
- fix(tagpr): use postVersionCommand with MISE_ENV profile by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/122

### Other Changes

- fix(tagpr): stage Cargo.lock in postVersionCommand by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/118

## [v0.2.6](https://github.com/naa0yama/dtvmgr/compare/v0.2.5...v0.2.6) - 2026-03-22

### Development Environment 🔧

- chore(deps): update taiki-e/install-action action to v2.68.31 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/114
- chore: backport boilerplate-rust infrastructure changes by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/116

## [v0.2.5](https://github.com/naa0yama/dtvmgr/compare/v0.2.4...v0.2.5) - 2026-03-22

### Documentation 🗒️

- feat(vmaf): add VMAF-based quality parameter search by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/110

### Development Environment 🔧

- chore(deps): update taiki-e/install-action action to v2.68.28 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/108
- chore(deps): update taiki-e/install-action action to v2.68.29 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/111
- chore(deps): update actions-rust-lang/setup-rust-toolchain action to v1.15.4 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/112
- chore(deps): update taiki-e/install-action action to v2.68.30 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/113
- chore(deps): update jdx/mise-action action to v4 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/102
- chore(deps): update dependency usage to v3 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/105

## [v0.2.4](https://github.com/naa0yama/dtvmgr/compare/v0.2.3...v0.2.4) - 2026-03-21

### Development Environment 🔧

- fix(jlse): harden post-encode duration validation for MKV and other containers by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/106

## [v0.2.3](https://github.com/naa0yama/dtvmgr/compare/v0.2.2...v0.2.3) - 2026-03-20

### Other Changes

- fix(db): skip migration write on read-only database and improve TUI storage display by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/103

## [v0.2.2](https://github.com/naa0yama/dtvmgr/compare/v0.2.1...v0.2.2) - 2026-03-20

### Documentation 🗒️

- refactor: migrate from Jaeger to OpenObserve for local tracing by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/98
- refactor(api): add OTel metrics/logs and consolidate rate limiters by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/91
- feat(jlse): add storage instrumentation, TUI widget, and pre-encode disk check by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/101

### Dependency Updates 📦

- chore(deps): update rust crate assert_cmd to v2.1.3 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/94
- chore(deps): update rust crate assert_cmd to v2.2.0 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/96

### Development Environment 🔧

- chore(deps): update taiki-e/install-action action to v2.68.26 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/95
- chore(deps): update dependency aqua:ast-grep/ast-grep to v0.41.1 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/93
- chore(deps): update actions/download-artifact action to v8.0.1 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/97
- chore(deps): update taiki-e/install-action action to v2.68.27 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/99

### Other Changes

- fix(tagpr): use cargo generate-lockfile for post-version command by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/89
- refactor(tui): extract TUI into dtvmgr-tui crate by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/100

## [v0.2.1](https://github.com/naa0yama/dtvmgr/compare/v0.2.0...v0.2.1) - 2026-03-16

### Documentation 🗒️

- build(cli): enable otel feature by default by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/76
- refactor(skills): delegate project skills to global shared skills by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/80

### Dependency Updates 📦

- chore(deps): update rust crate libc to v0.2.183 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/79

### Development Environment 🔧

- chore(deps): update taiki-e/install-action action to v2.68.20 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/74
- chore(deps): update taiki-e/install-action action to v2.68.21 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/75
- chore(deps): update taiki-e/install-action action to v2.68.22 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/78
- chore(deps): update taiki-e/install-action action to v2.68.23 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/81
- chore(deps): update all action update by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/83
- chore(deps): update taiki-e/install-action action to v2.68.25 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/85
- chore(deps): update zizmorcore/zizmor-action action to v0.5.2 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/86
- ci: align CI/release workflows with chezmage upstream by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/88

### Other Changes

- refactor(cli): rename --dir to --config by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/71
- refactor: consolidate set_pdeathsig into shared apply_pdeathsig helper by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/73
- fix(ci): skip container cleanup when package does not exist by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/77
- Update initializeCommand.sh by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/82
- chore(deps): lock file maintenance by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/84
- update(devcontainer): improve port forwarding and add Jaeger UI customization by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/87

## [v0.2.0](https://github.com/naa0yama/dtvmgr/compare/v0.1.6...v0.2.0) - 2026-03-13

### Dependency Updates 👒

- chore(deps): update dependency github:rust-secure-code/cargo-auditable to v0.7.4 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/64
- chore(deps): update taiki-e/install-action action to v2.68.19 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/66
- chore(deps): update github/codeql-action action to v4.32.6 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/68

### Other Changes

- refactor(cli): flatten TmdbApiConfig into TmdbConfig by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/67
- feat: add EPGStation encode command with TUI selector by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/69
- fix(ci): prevent tagpr failure on pull_request labeled event by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/70

## [v0.1.6](https://github.com/naa0yama/dtvmgr/compare/v0.1.5...v0.1.6) - 2026-03-11

### Documentation 🗒️

- feat(jlse): EIT XML save, output extension override, and ffmpeg stream specifier fix by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/63

### Dependency Updates 👒

- chore(deps): update taiki-e/install-action action to v2.68.17 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/57
- chore(deps): update taiki-e/install-action action to v2.68.18 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/61
- chore(deps): update dependency github:rust-secure-code/cargo-auditable to v0.7.3 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/60
- chore(deps): update rust docker tag to v1.93.1 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/62
- chore(deps): update docker/login-action action to v4 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/59

## [v0.1.5](https://github.com/naa0yama/dtvmgr/compare/v0.1.4...v0.1.5) - 2026-03-09

### Other Changes

- fix(security): prevent cleartext logging of API token in TMDB client by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/55

## [v0.1.4](https://github.com/naa0yama/dtvmgr/compare/v0.1.3...v0.1.4) - 2026-03-09

### Dependency Updates 👒

- chore(deps): update github/codeql-action action to v4.32.5 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/53

### Other Changes

- fix(ci): add end-of-options marker to printf with leading dash by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/52

## [v0.1.3](https://github.com/naa0yama/dtvmgr/compare/v0.1.2...v0.1.3) - 2026-03-09

### Other Changes

- fix(ci): preserve artifact permissions with tar-based upload by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/50

## [v0.1.2](https://github.com/naa0yama/dtvmgr/compare/v0.1.1...v0.1.2) - 2026-03-09

### Other Changes

- fix(ci): fix release build and sync Cargo.lock on tagpr version bump by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/46
- Add pull_request trigger to tagpr workflow by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/48

## [v0.1.1](https://github.com/naa0yama/dtvmgr/compare/v0.1.0...v0.1.1) - 2026-03-09

### Other Changes

- fix(ci): replace cargo-zigbuild with native runners in release build by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/44

## [v0.1.0](https://github.com/naa0yama/dtvmgr/commits/v0.1.0) - 2026-03-09

### Documentation 🗒️

- Dev/1st build by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/24

### Dependency Updates 👒

- Update taiki-e/install-action action to v2.67.20 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/4
- Update All action update by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/5
- Update taiki-e/install-action action to v2.67.25 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/6
- Update taiki-e/install-action action to v2.67.26 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/7
- Update taiki-e/install-action action to v2.67.27 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/8
- Update dependency mozilla/sccache to v0.14.0 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/9
- Update taiki-e/install-action action to v2.67.28 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/11
- Update dependency rust-cross/cargo-zigbuild to v0.21.7 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/13
- Update dependency rust-cross/cargo-zigbuild to v0.21.8 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/15
- Update github/codeql-action action to v4.32.3 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/16
- Update taiki-e/install-action action to v2.67.29 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/17
- Update taiki-e/install-action action to v2.67.30 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/18
- Update dependency rust-cross/cargo-zigbuild to v0.22.0 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/19
- Update Songmu/tagpr action to v1.17.0 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/21
- Update taiki-e/install-action action to v2.68.0 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/22
- Update All action update by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/27
- Update dependency usage to v2.18.1 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/28
- chore(deps): update rust crate predicates to v3.1.4 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/29
- chore(deps): pin ghcr.io/naa0yama/join_logo_scp_trial docker tag to faaf043 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/26
- chore(deps): update dependency aqua:ast-grep/ast-grep to v0.41.0 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/31
- chore(deps): update dependency dprint to v0.52.0 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/32
- chore(deps): update rust crate crossterm to 0.29 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/33
- chore(deps): update rust crate quick-xml to 0.39 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/34
- chore(deps): update all action update (major) by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/39
- chore(deps): update dependency usage to v2.18.2 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/40
- chore(deps): update taiki-e/install-action action to v2.68.16 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/41
- chore(deps): update rust crate toml to 0.9 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/37
- chore(deps): update rust crate ratatui to 0.30 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/35
- chore(deps): update rust crate toml to v1 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/42
- chore(deps): update rust crate rusqlite to 0.38 by @renovate[bot] in https://github.com/naa0yama/dtvmgr/pull/36

### Other Changes

- Add permissions for actionlint and zizmor jobs by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/2
- fix(ci): update jdx/mise-action to v3.6.3 by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/30
- fix(ci): move sccache config to env to unblock Renovate by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/38
- docs: add ETXTBSY on overlayfs knowledge to skills by @naa0yama in https://github.com/naa0yama/dtvmgr/pull/43
