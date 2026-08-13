# ctxctl

**Your coding agent pays for every byte of context. ctxctl makes files and command output smaller before they reach the model.**

Agents dump whole files and raw build logs into context, then burn tokens re-reading what they already saw. ctxctl is a pure-CLI, zero-MCP, stateless context layer: an agent reads only the symbol it needs — via tree-sitter AST location → original-source slice — and compresses command output with byte-stable results that hit provider prompt caching.

```bash
ctxctl outline src/server.rs
```

```text
# src/server.rs  [12 symbols, ~2.1 KB -> ~410 tokens, saved ~80%]
  fn     handle_request  L:42-58      pub async fn handle_request(&self, id: u64)
  struct Config           L:60-71      pub struct Config {
  fn     validate         L:73-88      fn validate(cfg: &Config) -> Result<(), Error> {
```

```bash
ctxctl symbol src/server.rs --name handle_request --compact
```

```text
# handle_request  src/server.rs:42-58  (58 tokens, saved ~85%)
pub async fn handle_request(&self, id: u64) -> Result<String, Error> { ... }
```

```bash
ctxctl exec "cargo build"
```

```text
$ cargo build
error[E0308]: mismatched types --> src/main.rs:12
... [34 lines omitted]
Saved ~70% (1,240 -> 372 tokens)
```

No whole-file dumps. No raw log walls. Deterministic output — identical runs stay cache-hot.

English | [简体中文](README.zh-CN.md)

## Installation

### Cargo (recommended)

```bash
cargo install ctxctl
```

### npm

```bash
npm install -g ctxctl
# or
bun add -g ctxctl
```

### GitHub Releases

Download the prebuilt binary for your platform from the [releases page](https://github.com/Xuepoo/ctxctl/releases) (also ships `.deb`, `.rpm`, `.apk`, and Arch packages).

## Quick start

```bash
ctxctl outline src/main.rs                         # symbol map + savings
ctxctl symbol src/main.rs --name run --compact     # one symbol, body folded
ctxctl read src/main.rs --lines 40-80              # raw line slices
ctxctl deps src/main.rs                            # import graph (local/external)
ctxctl exec "cargo test" --keep "FAILED|passed"    # run + compress output
ctxctl outline src/main.rs --json                  # machine contract
```

## Commands

| Command                               | Purpose                                                                     |
| ------------------------------------- | --------------------------------------------------------------------------- |
| `outline <file>`                      | Symbol outline with token-savings stats                                     |
| `symbol <file> --name <s>`            | Original-source slice of one symbol (`--compact`, `--signature`, `--lines`) |
| `read <file> --lines 100-150,200-210` | Raw line-range slices (no AST)                                              |
| `deps <file>`                         | Import/module dependency graph (local / external / ignored)                 |
| `exec <cmd> [--keep <pat>]`           | Run a command, compress its output                                          |

Global flags: `--json` (machine contract), `--config <path>`, `--no-saved`. Config: `--config` > `.ctxctl/config.toml` (walk-up) > XDG > defaults.

## Agent Skill Setup

Load the ctxctl skill to teach your coding agent to read files by symbol slice and compress command output instead of dumping raw context:

```bash
# List available skills
npx skills add Xuepoo/ctxctl-skills --list

# Install the core skill for all detected agents
npx skills add Xuepoo/ctxctl-skills --all

# Or install for specific agents
npx skills add Xuepoo/ctxctl-skills \
  --skill ctxctl-core \
  --agent codex \
  --agent claude-code \
  --agent cursor \
  --agent github-copilot
```

Use the skill once without installing it:

```bash
npx skills use Xuepoo/ctxctl-skills --skill ctxctl-core
```

The skill gives agents first-class awareness of the `outline` / `symbol` / `read` / `deps` / `exec` workflow — fewer tokens per task, cache-friendly runs. The CLI supports GitHub shorthand, full URLs, and local paths; see <https://github.com/vercel-labs/skills>.

## Documentation

- Website & guides: [ctxctl.xuepoo.xyz](https://ctxctl.xuepoo.xyz)
- Agent skill source: [ctxctl-skills](https://github.com/Xuepoo/ctxctl-skills)

## Principles

- **Zero-MCP, stateless** — no server, no state file, no background process. Every invocation is self-contained.
- **Byte-stable output** — no timestamps, no counters; identical inputs produce identical bytes so provider prompt caches stay hot.
- **Slices, not summaries** — symbols come from the original source by byte range; nothing is reworded.
- **Minimal deps** — 11 language backends via tree-sitter, zero network dependencies.

## Development

Rust edition 2024 workspace: `ctx-symbol` (symbol engine), `ctx-exec` (output compression), `ctxctl` (thin clap shell).

```bash
just check      # fmt + clippy + check + test
cargo test      # 142 tests
```

## License

MIT
