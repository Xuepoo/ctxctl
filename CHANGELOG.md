# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),

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
