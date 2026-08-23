# CtxCtl CLI Contract Specification

**Path:** `ctxctl-cli/docs/cli-contract.md`
**Doc version:** v0.3
**Applies to:** ctxctl v0.3.x

---

## 1. Design Goals

ctxctl is a **CLI-first, stateless** context layer for AI coding agents,
with an optional `ctxctl mcp` adapter serving the same commands as MCP
tools. It serves:

- Coding agents (primary consumer — output is optimized for cold LLM reading)
- Human developers
- Shell scripts / CI
- Other tools (via a strict `--json` contract)

The CLI must provide:

- **Byte-stable output** — no timestamps/counters, so it hits provider prompt
  caching (core principle, see §8)
- Stable command names
- Deterministic behavior (same input → same output)
- Machine-readable `--json`
- Well-defined Exit Codes
- Non-interactive operation
- **Built-in token-savings metrics** (`saved N%`)
- Predictable config resolution (§6)
- Clear error-recovery guidance

---

## 2. Invocation

```text
ctxctl [global-options] <command> [arguments]
```

Examples:

```bash
ctxctl outline src/server.rs
ctxctl symbol src/server.rs --name handle_request
ctxctl read src/server.rs --lines 100-150
ctxctl exec "cargo build" --keep 'error|warning'
```

---

## 3. Global Options

| Option            | Type       | Default | Description                                                                                                                                          |
| ----------------- | ---------- | ------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `--config <path>` | string     | see §6  | Explicit config file; highest priority                                                                                                               |
| `--format <fmt>`  | text\|json | text    | Output format. `json` is the machine contract                                                                                                        |
| `--json`          | flag       | off     | Alias for `--format=json`                                                                                                                            |
| `--no-color`      | flag       | off     | Disable ANSI colors                                                                                                                                  |
| `--no-saved`      | flag       | off     | Suppress `saved%` metrics                                                                                                                            |
| `--output <path>` | string     | —       | Write the full payload to `<path>` instead of stdout (bypasses stdout size limits); a `wrote <path>` confirmation goes to stderr, stdout stays empty |
| `-h, --help`      | —          | —       | Help                                                                                                                                                 |
| `-V, --version`   | —          | —       | Version                                                                                                                                              |

**Byte-stability (§8):** global options must not inject timestamps, counters,
or randomness into the output body.

---

## 4. Command Contracts

### 4.1 `ctxctl outline <file>`

Print a symbol outline of a file (classes/methods/functions/variables +
line numbers + signatures).

```bash
ctxctl outline src/server.rs
ctxctl outline src/server.rs --json
```

**text output** (one line per symbol, cold-read optimized):

```text
# src/server.rs  [12 symbols, ~2.1 KB -> ~410 tokens, saved ~80%]
  fn     handle_request  L:42-58      pub async fn handle_request(&self, id: u64)
  struct RequestHandler  L:12-40      pub struct RequestHandler {
  const  MAX_RETRIES     L:60         const MAX_RETRIES: u32 = 3;
```

**json output contract** (fixed envelope):

```json
{
  "schema_version": 1,
  "tool": "outline",
  "path": "src/server.rs",
  "language": "rust",
  "symbols": [
    {
      "name": "handle_request",
      "kind": "method",
      "start_line": 42,
      "end_line": 58,
      "signature": "pub async fn handle_request(&self, id: u64)",
      "doc_comment": "Process a single request by id."
    }
  ],
  "saved": { "tokens_before": 512, "tokens_after": 380, "percent": 26 }
}
```

**`saved` semantics:** `tokens_before` = tokens of the whole file;
`tokens_after` = tokens of the **actual output** (the rendered symbol list
in text mode, the serialized payload in JSON mode — excluding the `saved`
field itself), not a sum of per-symbol slice estimates. On tiny files the
JSON envelope can exceed the file, so `percent` may legitimately be 0.
`percent` = `(before - min(after, before)) / before`, capped at 100.

**Signatures:** `signature` is the first declaration line, normalized:
leading attribute / decorator-only lines are skipped, trailing comments are
dropped, whitespace is collapsed, trailing continuation delimiters (`(`,
`{`, `,`, `;`, `:`, `=`, `->`, `=>`) are stripped, and the result is capped
at 120 chars with `…`. Kinds: `class`, `struct`, `enum`, `interface`,
`function`, `method`, `module`, `const`, `var`, `trait`, `type`.

**Parse failures:** when tree-sitter reports ERROR/MISSING nodes the
partial symbol list is still delivered, but JSON gains
`"parse_error": {"count": N, "message": "..."}` and the exit code is 3
(text mode additionally prints `warning: parse failed ...` on stderr). A
clean parse never carries `parse_error` and exits 0.

**Arguments:**

| Arg          | Type | Default  | Description       |
| ------------ | ---- | -------- | ----------------- |
| `<file>`     | path | required | Target file       |
| `--no-doc`   | flag | off      | Omit doc_comment  |
| `--no-lines` | flag | off      | Omit line numbers |

**Folding (text mode only):** when the symbol count exceeds
`[outline] fold_threshold` (§7), the list is folded with a
`... [N symbols omitted]` marker after the shown entries. The header keeps
the total count. `--json` output is never folded.

**Exit Codes:** 0 success; 1 file read failure; 2 unsupported extension;
3 parse failure.

---

### 4.2 `ctxctl symbol <file> --name <sym>`

Locate a symbol by name and return its slice from the **original source**
(byte range → verbatim text).

```bash
ctxctl symbol src/server.rs --name handle_request
ctxctl symbol src/server.rs --name handle_request --json
```

**text output** (one locator line, then the verbatim slice):

```text
# handle_request  src/server.rs:42-58  (58 tokens, saved ~85%)
pub async fn handle_request(&self, id: u64) -> Result<String, Error> {
    let row = self.db.get(id).await?;
    Ok(row.to_string())
}
```

**Key properties:**

- The slice comes from the original source bytes, preserving comments,
  formatting, and indentation verbatim.
- Byte-stable (same file + same symbol → same bytes).
- Contains only the target symbol's body, not the rest of the file.

**Arguments:**

| Arg            | Type   | Default  | Description                                                                                                                                                                 |
| -------------- | ------ | -------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `<file>`       | path   | required | Target file                                                                                                                                                                 |
| `--name <sym>` | string | required | Symbol name (exact)                                                                                                                                                         |
| `--kind <k>`   | enum   | —        | Restrict the match to a symbol kind (`class`, `struct`, `enum`, `interface`, `function`, `method`, `module`, `const`, `var`, `trait`, `type`, `heading`, `rule`, `element`) |
| `--signature`  | flag   | off      | Return signature only, not body                                                                                                                                             |
| `--compact`    | flag   | off      | AST-pruned view: signature + fold marker for the body                                                                                                                       |
| `--lines`      | string | —        | Sub-range within the symbol, e.g. `3-10`                                                                                                                                    |

When several symbols share a name, the first in source order wins; `--kind`
narrows the match (e.g. a class method vs a same-named local variable).

`--compact` is mutually exclusive with `--signature` and `--lines`. The
compact view keeps the signature (and python decorators), replaces the
body with a line-comment fold marker (`// ... [N lines omitted]`, `# …`
for python), and keeps a bare closing line (`}`) when present. It is a
byte-stable function of the source and re-parses without errors. In JSON
mode `--compact` replaces `slice` with a `compact` field.

**Exit Codes:** 0 success; 1 read failure; 2 unsupported extension;
3 parse failure; 4 symbol not found.

---

### 4.3 `ctxctl read <file> --lines <N-M>`

Read a line range from the original source (no AST needed; plain text slice).

```bash
ctxctl read src/server.rs --lines 100-150
ctxctl read src/server.rs --lines 100-150,200-210
```

**Arguments:**

| Arg       | Type   | Default  | Description                                                                     |
| --------- | ------ | -------- | ------------------------------------------------------------------------------- |
| `<file>`  | path   | required | Target file                                                                     |
| `--lines` | string | required | Line range(s), comma-separated; open-ended ranges (`10-`) clamp to the file end |

**Exit Codes:** 0 success; 1 read failure; 2 invalid or out-of-bounds range.

---

### 4.4 `ctxctl exec <cmd> [--keep <pattern>]`

Run a command and compress its output: keep key lines + head/tail summary,
collapse the middle.

```bash
ctxctl exec "cargo build"
ctxctl exec "cargo test" --keep 'error|warning|failed'
ctxctl exec "git status" --json
```

**Compression rules (default):**

- Keep lines matching the default patterns from §7 (`error`, `warning`,
  `failed`, `panic`, `fatal`; case-insensitive)
- Keep diagnostic **location lines** (`--> src/foo.rs:12:34`,
  rustc/cargo style; pattern `^\s+-->`) implicitly, so a kept error header
  retains its file:line context even when the configured patterns would
  drop it
- Keep output head and tail (command verdict/summary)
- Collapse the middle to `... [N lines omitted]`
- `--keep <pattern>` appends a custom keep regex
- An empty configured keep list (`[exec] keep = []`) matches nothing — it
  disables mid-output retention instead of keeping every line (implicit
  location keeps stay active)

**Over-broad keep detection:** when folding ran but saved at most 10%
(typically a `--keep` pattern that case-insensitively matches most lines),
a deterministic warning is appended: text mode adds a final
`warning: ...` line, JSON mode adds a top-level `"warning"` field. The
output stays byte-stable.

**Stream merge:** stdout is emitted before stderr (temporal interleaving of
the child's two streams is not preserved). When both streams are non-empty
and stdout does not end with a newline, one is inserted between them.

**Streaming:** output is compressed incrementally as the child produces it;
memory stays bounded by the head/tail windows and keep-pattern matches, so
commands emitting gigabytes cannot exhaust memory. The rendered output is
byte-identical to the buffered algorithm.

**text output:**

```text
$ cargo build
error[E0308]: mismatched types --> src/main.rs:12
... [34 lines omitted]
warning: unused variable: `x` --> src/server.rs:88
   = note: 2 warnings emitted
Saved ~70% (1,240 -> 372 tokens)
```

**Arguments:**

| Arg                | Type   | Default             | Description                                                       |
| ------------------ | ------ | ------------------- | ----------------------------------------------------------------- |
| `<cmd>`            | string | required            | Command line to run (shell-word quoting)                          |
| `--keep <pattern>` | string | —                   | Extra keep regex (rg syntax), appended to the configured patterns |
| `--head <n>`       | int    | `[exec] head_lines` | Override head summary lines                                       |
| `--tail <n>`       | int    | `[exec] tail_lines` | Override tail summary lines                                       |

**Exit Codes:** passes through the wrapped command's exit code; 0 with no key
errors = command succeeded.

---

### 4.5 `ctxctl deps <file>`

Print the import/module dependency graph of a file: one line per import
with target, kind, and line number.

```bash
ctxctl deps src/main.rs
ctxctl deps src/main.rs --json
```

**text output** (one line per import, cold-read optimized):

```text
# src/main.rs  [3 imports, ~512 B -> ~64 tokens, saved ~88%]
external  serde       L:1
local     crate::lib  L:2
```

**json output contract** (fixed envelope):

```json
{
  "schema_version": 1,
  "tool": "deps",
  "path": "src/main.rs",
  "language": "rust",
  "imports": [
    { "target": "serde", "kind": "external", "line": 1 },
    { "target": "crate::lib", "kind": "local", "line": 2 }
  ],
  "saved": { "tokens_before": 512, "tokens_after": 64, "percent": 88 }
}
```

**kind semantics:**

- `local` — in-crate or relative imports: rust `crate::`/`super::`/`self::`
  and `mod x;`; TS relative `./`/`../`; python leading-dot modules; or a
  bare python/go target that resolves to an existing file/directory under
  the cwd (existence probe, no index).
- `external` — everything else (crates, stdlib, npm packages, remote
  modules).
- `ignored` — target matches `[paths] ignore` (§7).

**Exit Codes:** 0 success; 1 file read failure; 2 unsupported extension;
3 parse failure.

### 4.6 `ctxctl mcp`

Serve `outline` / `symbol` / `read` / `deps` / `exec` as MCP tools over
stdio (newline-delimited JSON-RPC 2.0). Optional adapter for MCP-native
agents; the CLI remains the canonical interface and nothing about it
changes.

```bash
ctxctl mcp            # serve until stdin EOF, exit 0
```

**Protocol behavior:**

- `initialize` → responds with `protocolVersion: "2025-03-26"`,
  `capabilities.tools`, `serverInfo { name: "ctxctl", version }`
- `tools/list` → five tools: `ctxctl_outline`, `ctxctl_symbol`,
  `ctxctl_read`, `ctxctl_deps`, `ctxctl_exec` (JSON Schema `inputSchema`)
- `tools/call` → runs the same handlers as the CLI in text mode with saved%
  metrics; results return as `{ content: [{ type: "text", text }] }`.
  Failures (missing file, unknown symbol, bad arguments) become
  `isError: true` results, never protocol errors.
- `tools/call` on `ctxctl_exec` prefixes `exit code N\n` when the wrapped
  command exited non-zero
- Notifications (messages without `id`) produce no response; unknown
  methods get JSON-RPC `-32601`; an id-carrying message without a method
  gets `-32600` (invalid request); malformed JSON gets `-32700`
- Config resolution is identical to the CLI (`--config` works globally)

**Exit Codes:** 0 on clean EOF.

---

## 5. Exit Code Summary

| Code                 | Meaning                                             |
| -------------------- | --------------------------------------------------- |
| 0                    | Success / wrapped command succeeded                 |
| 1                    | File or command read failure                        |
| 2                    | Unsupported extension / invalid line range          |
| 3                    | Parse failure                                       |
| 4                    | Symbol not found                                    |
| non-zero passthrough | `exec` passes through the child's exit code         |
| 128 + signal         | `exec` when the child was killed by a signal (Unix) |

**Error streams:** in text mode errors print to stderr; in JSON mode
(`--json` / `--format json`) the error envelope prints to **stdout** so
machine consumers get one parseable stream. The envelope is
`{"error": {"code": <exit-code>, "message": "<text>"}}`.

---

## 6. Config Resolution Precedence

ctxctl is **stateless**: the config file stores only default-behavior
preferences, never indexes/state/session.

**Lookup precedence (high → low):**

```text
1. --config <path>                         explicit
2. ./ctxctl/config.toml                    project-level (discovered walking up)
3. $XDG_CONFIG_HOME/ctxctl/config.toml     XDG global (default ~/.config/ctxctl/)
4. Built-in defaults
```

**Project-level discovery:** walk up from the current working directory
looking for `.ctxctl/config.toml` (like `.git` discovery); stop at the first
hit.

**Config model:**

- **No state** — no SQLite, no index, no session. Keeps ctxctl separate from
  carryctx's `.carryctx/` + `state.sqlite` responsibility.
- **Read per command** — stateless means no cache; light parse each time.
- Config keys are specified in §7.

---

## 7. Config Keys (`config.toml`)

```toml
# ~/.config/ctxctl/config.toml  (XDG global)  or  .ctxctl/config.toml  (project)
# If this file exists, it takes priority over the XDG global one.

[exec]
# Default keep regexes (rg-style)
keep = ["error", "warning", "failed", "panic", "fatal"]
# Lines to keep from output head / tail
head_lines = 5
tail_lines = 5
# Collapse only when output exceeds this many lines
collapse_threshold = 20

[outline]
# Fold large files when symbols exceed this threshold
fold_threshold = 50
# Show doc_comment by default
show_doc = true

[paths]
# Default-ignored directories (rg-style glob)
ignore = ["node_modules", "target", "dist", ".git"]

[general]
# Show saved% metrics
show_saved = true
```

**Merge semantics:** project-level overrides the global key; undeclared keys
fall back to global → default. No array-concatenation semantics (keeps
determinism).

---

## 8. Byte Stability (Core Contract)

**This is the principle that sets ctxctl apart from naive compressors.**

- The output body **must never** contain timestamps, counters, random values,
  machine-specific paths, or PIDs.
- Same input + same config → byte-identical output.
- Purpose: hit Anthropic ~90% / OpenAI ~50% **provider prompt caching**, so
  the tokens already saved get a further discount.
- `saved%` is a deterministic function: token counts come from the
  **cl100k_base BPE tokenizer** (GPT-4-class, the community-standard
  approximation across providers) — never an external measurement.
- Exception: the `--format=json` envelope may carry a stable schema-version
  field, but never volatile values.

---

## 9. Design Principles

1. **Stateless** — no index, no cache, no daemon. Parse-and-discard.
2. **Byte-stable** — all output targets prompt caching (§8).
3. **Agent-directed** — text output is cold-read optimized (compact, greppable,
   one symbol per line); `--json` is the program contract.
4. **Language-agnostic core** — adding a language means adding one backend
   module in `ctx-symbol`.
5. **exec is the differentiator** — command-output compression is ctxctl's
   unique value vs. peers (e.g. ast-outline).
6. **Offline-first** — zero network dependencies, no external services.

---

## 10. Future Extensions (non-blocking)

- More language backends (C/C++, C#, Ruby, …) via `ctx-symbol`

---

## 11. Supported Languages

Backends live in `ctx-symbol`; `language` reports the backend name.

| Backend      | Extensions                                     | Definitions                                                                                                                       | Imports                                          |
| ------------ | ---------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------ |
| `rust`       | `.rs`                                          | functions, structs, enums, traits, mods, types, consts, statics                                                                   | `use`, `mod`, `extern crate`                     |
| `typescript` | `.ts` `.tsx` `.mts` `.cts`                     | functions, classes, methods, interfaces, type aliases, enums, variables                                                           | `import`, re-exports, `require`                  |
| `python`     | `.py` `.pyi`                                   | functions, classes (+ decorators)                                                                                                 | `import`, `from … import`                        |
| `go`         | `.go`                                          | functions, methods, type specs, consts, vars                                                                                      | `import`                                         |
| `javascript` | `.js` `.jsx` `.mjs` `.cjs`                     | functions (incl. generators), classes, methods, variables                                                                         | `import`, re-exports, `require`                  |
| `java`       | `.java`                                        | classes, records, interfaces, enums, annotation types, methods, constructors, fields                                              | `import` (incl. static/wildcard)                 |
| `c`          | `.c` `.h`                                      | functions, structs, unions, enums, typedefs, fields, `#define`                                                                    | `#include` (quoted = local)                      |
| `cpp`        | `.cpp` `.cc` `.cxx` `.hpp` `.hh` `.hxx` `.h++` | functions, classes, structs, unions, enums, typedefs, fields, namespaces, `#define`; template defs include `template <T>` headers | `#include`, `using` (not aliases)                |
| `csharp`     | `.cs`                                          | classes, records, interfaces, structs, enums, namespaces, methods, constructors, properties, fields, local functions              | `using` (incl. `using static`; not aliases)      |
| `ruby`       | `.rb`                                          | methods (incl. singleton), classes, modules                                                                                       | `require` (external), `require_relative` (local) |
| `lua`        | `.lua`                                         | functions (incl. local), variables                                                                                                | `require` (path-prefixed = local)                |
| `html`       | `.html` `.htm`                                 | elements carrying an `id` attribute (name = id value)                                                                             | —                                                |
| `css`        | `.css` `.scss`                                 | rulesets (selector list = name); nested `@media` rules included. `.scss` is best-effort via the CSS grammar                       | —                                                |
| `markdown`   | `.md` `.markdown`                              | ATX + setext headings; each heading's slice spans its whole section                                                               | —                                                |
