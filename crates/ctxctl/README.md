# ctxctl

Pure-CLI, zero-MCP, stateless context layer for AI coding agents.

`ctxctl` lets an agent read only the part of a file it needs — a symbol
located via a tree-sitter AST, sliced straight from the original source —
and compress command output instead of dumping whole files into context.
Output is **byte-stable** (a pure function of input + config), so provider
prompt caching applies to repeated reads.

This crate is the thin `clap` shell over two engine crates:

- `ctx-symbol` — tree-sitter AST symbol location + original-source slicing
  (Rust, TypeScript, Python, Go backends)
- `ctx-exec` — rg-rule-driven command output compression

## Quickstart

```bash
cargo build --release
./target/release/ctxctl --help
```

## Commands

### outline — symbol outline with token savings

```bash
ctxctl outline src/server.rs
ctxctl outline src/server.rs --json
```

```text
# src/server.rs  [30 symbols, ~22.9 KB -> 5,429 tokens, saved ~7%]
  type    config          L:13         mod config;
  struct  Cli             L:30-49      struct Cli {
```

### symbol — original source slice of one symbol

```bash
ctxctl symbol src/server.rs --name handle_request
ctxctl symbol src/server.rs --name handle_request --signature
ctxctl symbol src/server.rs --name handle_request --compact
```

```text
# handle_request  src/server.rs:42-58  (58 tokens, saved ~85%)
pub async fn handle_request(&self, id: u64) -> Result<String, Error> {
    let row = self.db.get(id).await?;
    Ok(row.to_string())
}
```

`--compact` keeps the signature (and python decorators) and folds the
body behind a `// ... [N lines omitted]` marker.

### read — raw line ranges, no AST

```bash
ctxctl read src/server.rs --lines 100-150,200-210
```

### exec — run a command, compress its output

```bash
ctxctl exec "cargo test" --keep 'error|warning|failed'
```

Keeps head/tail and lines matching keep patterns, folds the middle:

```text
$ cargo test
error[E0308]: mismatched types --> src/main.rs:12
... [34 lines omitted]
warning: unused variable: `x` --> src/server.rs:88
Saved ~70% (1,240 -> 372 tokens)
```

### deps — import dependency graph

```bash
ctxctl deps src/main.rs
```

```text
# src/main.rs  [3 imports, ~512 B -> ~64 tokens, saved ~88%]
external  serde       L:1
local     crate::lib  L:2
```

## Configuration

Stateless preference file, no state/index/session. Lookup precedence:

1. `--config <path>`
2. `.ctxctl/config.toml` discovered by walking up from the cwd
3. `$XDG_CONFIG_HOME/ctxctl/config.toml`

```toml
[exec]
keep = ["error", "warning", "failed", "panic", "fatal"]
head_lines = 5
tail_lines = 5
collapse_threshold = 20

[outline]
fold_threshold = 50
show_doc = true

[paths]
ignore = ["node_modules", "target", "dist", ".git"]

[general]
show_saved = true
```

Project-level keys override global keys; undeclared keys fall back to
global → defaults. No array-concatenation.

## Byte stability

The output body never contains timestamps, counters, random values,
machine-specific paths, or PIDs. Same input + same config → byte-identical
output, which is what lets provider prompt caching discount the tokens
already saved. `saved%` is a deterministic estimate (4 bytes ≈ 1 token),
not an external measurement.

## Contract

The authoritative CLI contract lives in
[`ctxctl-docs/cli-contract.md`](https://github.com/Xuepoo/ctxctl-docs/blob/main/cli-contract.md):
command contracts, JSON envelopes, exit codes, config keys, and byte
stability requirements.
