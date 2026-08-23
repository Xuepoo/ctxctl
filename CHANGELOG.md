# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),

## [0.3.0] - 2026-08-23

### Added

- **HTML, CSS/SCSS and Markdown backends** — `outline` / `symbol` /
  `--kind` now cover 14 languages. HTML extracts elements carrying an
  `id` attribute (name = id value); CSS extracts rulesets (selector list =
  name, nested `@media` included; `.scss` is best-effort via the CSS
  grammar); Markdown extracts ATX and setext headings, and each heading's
  slice spans its whole section — `symbol --name "Chapter"` yields the
  entire chapter text.
- Three new symbol kinds: `heading`, `rule`, `element` (accepted by
  `symbol --kind`, reported in outline JSON).
- **`ctxctl mcp`** — optional MCP stdio adapter serving `outline` / `symbol`
  / `read` / `deps` / `exec` as MCP tools (newline-delimited JSON-RPC 2.0).
  The CLI remains the canonical interface; the adapter reuses the exact same
  handlers, so output stays byte-identical. Tool failures surface as
  `isError` results; non-zero exec exit codes are prefixed into the result
  text.

### Changed

- **exec preserves diagnostic location lines** — lines matching
  `^\s+-->` (rustc/cargo-style `--> file:line`) are now kept implicitly,
  so a kept error header no longer loses its file:line context.
- **exec warns on over-broad keep patterns** — when folding ran but saved
  at most 10% (typically a `--keep` regex that case-insensitively matches
  most lines), a deterministic warning is appended: text mode gains a final
  `warning:` line, JSON mode a top-level `"warning"` field.

## [0.2.2] - 2026-08-14

### Added

- **`symbol --kind <k>`** — disambiguate same-name symbols (issue #6): a
  class method vs a same-named local variable no longer resolves to whichever
  comes first in source order. Values match the outline JSON `kind` field.
- **`--output <path>`** — global option writing the full payload to a file,
  bypassing stdout size limits on large `outline --json` output (issue #5).

### Changed

- **Preprocessor-aware C/C++ header parsing (M2)** — `.h` files carrying
  C++-only markers (`::`, `template`, `namespace`, …) now route to the cpp
  grammar; annotation macros (SAL tokens, decl-style invocations, ALL-CAPS
  specifiers) are masked when they confuse the parser. C/C++ headers that
  used to exit 3 now outline cleanly (system corpus clean slices +26%).

## [0.2.1] - 2026-08-13

### Fixed

- **`.tsx` parsed with the TSX grammar** — JSX-in-TypeScript was parsed with
  the plain TypeScript grammar and failed to outline on 91% of `.tsx` files.
  `.tsx` now routes through `tree-sitter-typescript`'s `LANGUAGE_TSX`.
- **TypeScript `const`/`let` extraction** — the symbol name lives on the
  `variable_declarator` child, not the declaration node; TS outline now
  extracts lexical declarations (symbol counts roughly triple on real code).

## [0.2.0] - 2026-08-13

### Changed

- **Parallel parse + token counting** — `outline`/`symbol`/`deps` overlap
  tree-sitter parsing with the cl100k token count on a second thread
  (deterministic; both are pure functions of the input). ~33% faster on
  multi-megabyte files.
- **Streaming `exec` (memory-bounded)** — child output is compressed
  incrementally; memory is O(head + tail + kept matches) instead of O(total
  output). A 600 MB output peaks at ~29 MB RSS. Rendered output stays
  byte-identical to the previous buffered algorithm (stdout-then-stderr
  merge preserved).
- **Real token accounting** — `saved%` metrics now use the cl100k_base BPE
  tokenizer (GPT-4-class) instead of the 4-bytes-per-token approximation;
  CJK-heavy text is counted accurately and counts stay a deterministic
  function of the input (byte stability unaffected).
- **AST-anchored compact folds (M1)** — `compact` now folds at the
  tree-sitter body node of a definition instead of guessing from line
  heuristics: brace bodies keep the header through the opening `{` and the
  closing line (including trailing declarators like `} Point;`), keyword
  bodies (python/ruby/lua) fold at the AST body start and keep their
  `end`-style closer. Stub detection, delimiter-aware string tracking, and
  block-comment masking keep the compact view re-parseable.
- **Body-less data literals pass through** — a variable/const whose value
  is a multi-line template string, JSX fragment, or array/struct literal is
  string data, not a block; `compact` no longer scans such symbols for `{`
  openers and leaves them unchanged (preprocessor macros still fold).

## [0.1.3] - 2026-08-13

### Fixed

- **Deterministic local test runs** — `tmp_dir()` now clears stale
  fixtures from a previous run; re-running the suite on the same machine
  no longer fails with `File exists` on the symlinked-config test.
- **Fully idempotent release pipeline** — GitHub Release creation skips
  when the tag release exists, and the npm root verify polls the registry
  until the fresh version is visible before installing.

## [0.1.2] - 2026-08-13

### Fixed

- **crates.io publish** — 0.1.1's `ctx-symbol` shipped without the
  `parse_error_count` export (a commit was missed from the tag), making
  `ctxctl` unpublishable against it; crates.io versions are immutable, so
  this release republishes the three crates with the complete API and
  adds `repository`/`homepage` manifest metadata.

## [0.1.1] - 2026-08-13

### Changed

- **`outline` saved accounting now measures actual output** — `tokens_after`
  counts the bytes actually delivered (the rendered symbol list / serialized
  JSON payload), not the sum of per-symbol slice estimates. Nested
  definitions are no longer double-counted, so `saved ~0%` on large files
  with real ~75% savings is gone (issue #2).
- **`outline` signals parse failures** — a broken file no longer masquerades
  as "0 symbols, saved 100%". JSON gains a `parse_error` field, text mode
  prints a warning on stderr, and the exit code is 3 while the partial
  symbol list is still delivered (issue #3).

### Fixed

- **Clean signatures** — outline rows no longer end in dangling `(`/`{`/`:`
  from multi-line declarations; trailing comments are dropped, attribute and
  decorator lines are skipped, long signatures are capped with an ellipsis.
- **`mod` items are `module` kind** — rust `mod foo;` was reported as `type`
  in outlines and JSON output.
- **Symlinked project config** — regression tests pin that a symlinked
  `.ctxctl/config.toml` (file or directory) is followed during walk-up
  discovery (issue #4).
- **release workflow** — recognize cargo's current "already exists on
  crates.io" wording so re-runs tolerate already-published crates; drop
  homebrew/scoop/docker publishing jobs.

## [0.1.0] - 2026-08-13

### Added

- **`ctx-symbol`** — reusable symbol engine: language-agnostic tree-sitter
  extraction (`Language` trait, `Symbol` with byte ranges into the original
  source), original-source slicing, and doc-comment capture. Backends: Rust,
  TypeScript, Python (docstring-aware, decorators included in slices), Go,
  JavaScript, Java (records/constructors/static+wildcard imports), C
  (declarator chains, typedefs, preprocessor), C++ (templates include
  `template <T>` headers in slices/compact, namespaces, `using`), C#
  (records/structs/properties/fields, `using static`), Ruby
  (`require_relative` local), Lua (`require` paths, `--` markers). Pure AST
  import extraction (`extract_imports`, one target per name for python
  multi-imports) for dependency graphs. `compact_symbol`: signature +
  fold-marker view that re-parses cleanly.
- **`ctx-exec`** — rg-rule-driven command-output compression: head/tail
  summary, keep patterns (`error`, `warning`, `failed`, `panic`, `fatal`,
  case-insensitive), `... [N lines omitted]` fold markers, deterministic
  token-savings stats. Byte-stable by design.
- **`ctxctl`** — CLI shell over both crates: `outline`, `symbol`
  (`--signature`/`--lines`/`--compact`), `read`, `exec` (`--keep`/`--head`/
  `--tail`, exit-code passthrough), `deps`. Global `--format json`/`--json`,
  `--config`, `--no-saved`, `--no-color`. Config resolution
  (`--config` > project `.ctxctl/config.toml` walk-up > XDG > defaults) with
  key-wise merge. `[exec]`, `[outline]` (`fold_threshold` text-mode folding),
  `[paths]` (ignore globs for `deps` classification), `[general]` sections.
  JSON envelopes `{schema_version, tool, path, saved}`; exit codes 0-4;
  byte-stable output targeting provider prompt caching. 121 tests.

---

## Legacy History

The predecessor product (carryctx-era ctxctl) has its own history in
[`CHANGELOG.legacy.md`](CHANGELOG.legacy.md).
