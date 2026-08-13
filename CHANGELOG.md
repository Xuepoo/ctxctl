# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),

## [Unreleased]

### Changed

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
