# Contributing

## Prerequisites

- Rust 1.97+ (CI, Docker, and Nix pin 1.97.1; `rust-toolchain.toml` tracks stable)
- `just` — command runner
- `lefthook` — Git hooks
- `cargo-nextest` — test runner (optional, `cargo test` works too)
- `cargo-deny` — license/advisory checks
- `cargo-audit` — vulnerability scanning
- `markdownlint-cli2` — Markdown linting

## Setup

```bash
just setup
```

## Development loop

```bash
just check              # fast: format + clippy + check + test
just ci                 # full pipeline
cargo test              # run tests
cargo nextest run       # faster parallel tests (if installed)
```

## Committing

Use Conventional Commits:

```text
feat(ctx-symbol): add a new language backend
fix(ctx-exec): keep-pattern matching for empty lists
docs(cli-contract): document exec signal exit codes
```
