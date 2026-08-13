# ctxctl

**Pure CLI, zero-MCP, stateless context layer for AI coding agents.**

ctxctl lets an agent read only the part of a file it needs — tree-sitter AST
symbol location → original-source slice — and compress command output, instead
of dumping whole files into context. Byte-stable output targets provider
prompt caching.

```text
$ ctxctl outline src/server.rs
# src/server.rs  [12 symbols, ~2.1 KB -> ~410 tokens, saved ~80%]
  fn     handle_request  L:42-58      pub async fn handle_request(&self, id: u64)

$ ctxctl symbol src/server.rs --name handle_request
# handle_request  src/server.rs:42-58  (58 tokens, saved ~85%)
pub async fn handle_request(&self, id: u64) -> Result<String, Error> { ... }

$ ctxctl exec "cargo build"
$ cargo build
error[E0308]: mismatched types --> src/main.rs:12
... [34 lines omitted]
Saved ~70% (1,240 -> 372 tokens)
```

## Commands

| Command                               | Purpose                                                                     |
| ------------------------------------- | --------------------------------------------------------------------------- |
| `outline <file>`                      | Symbol outline with token-savings stats                                     |
| `symbol <file> --name <s>`            | Original-source slice of one symbol (`--compact`, `--signature`, `--lines`) |
| `read <file> --lines 100-150,200-210` | Raw line-range slices (no AST)                                              |
| `deps <file>`                         | Import/module dependency graph (local / external / ignored)                 |
| `exec <cmd> [--keep <pat>]`           | Run a command, compress its output                                          |

Global flags: `--json` / `--format json` (machine contract), `--config <path>`,
`--no-saved`. Full specification: [`ctxctl-docs/cli-contract.md`](https://github.com/Xuepoo/ctxctl-docs/blob/main/cli-contract.md).

## Install

- **crates.io:** `cargo install ctxctl`
- **npm:** `npm install --save-dev ctxctl`
- **GitHub Releases / deb / rpm / apk / Homebrew / Scoop:** published on tag
- **Docker:** `docker build -t ctxctl .`
- **Nix:** `nix develop` (dev shell)

## Development

Rust edition 2024 workspace: `ctx-symbol` (symbol engine, 11 language
backends via tree-sitter), `ctx-exec` (rg-rule output compression), `ctxctl`
(thin clap shell).

```bash
just check      # fmt + clippy + check + test (full quality gates)
just dev -- outline src/main.rs
```

Config: `--config` > `.ctxctl/config.toml` (walk-up) > XDG > defaults. See
`ctxctl-docs/cli-contract.md` §6-7.

## License

MIT
