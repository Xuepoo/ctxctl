//! `ctx-symbol` — a reusable symbol engine for token-efficient code reading.
//!
//! Uses tree-sitter to locate symbols (classes, functions, methods, …) in a
//! source file, then exposes their **byte ranges** into the original text so a
//! caller can slice out exactly the part it needs — instead of reading a whole
//! file into an LLM context.
//!
//! Design:
//! - Stateless: parse on demand, no cache, no index.
//! - Language-agnostic core; each backend in `lang/` maps tree-sitter node
//!   types to [`symbol::SymbolKind`].
//! - Byte-stable output: slices come from the original source, preserving
//!   comments/formatting and enabling provider prompt caching.

pub mod imports;
pub mod lang;
pub mod language;
pub mod symbol;

pub use imports::{Import, ImportTarget, extract_imports};
pub use language::{Language, ParsedSource, SymbolError, extract_symbols, parse};
pub use symbol::{Symbol, SymbolKind};

/// Convenience: locate a symbol by name and return its original source slice.
pub fn slice_by_name(
    source: &str,
    path: &std::path::Path,
    name: &str,
) -> Result<String, SymbolError> {
    let parsed = parse(path, source)?;
    let symbols = extract_symbols(&parsed);
    let sym = symbols
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| SymbolError::NotFound(name.to_string()))?;
    let bytes = &source.as_bytes()[sym.byte_range.clone()];
    let text = std::str::from_utf8(bytes).map_err(|e| SymbolError::Parse(e.to_string()))?;
    Ok(text.to_string())
}

/// Convenience: return the full outline (all symbols) of a source file.
pub fn outline(source: &str, path: &std::path::Path) -> Result<Vec<Symbol>, SymbolError> {
    let parsed = parse(path, source)?;
    Ok(extract_symbols(&parsed))
}

/// Compact view of a symbol: signature (and decorators / multi-line signature
/// lines) plus a fold marker replacing the body, keeping the closing line
/// (e.g. `}`) when the body ends with one.
///
/// The result is a byte-stable function of the source and re-parses without
/// errors in the backend grammar (the marker is a line comment). Single-line
/// symbols pass through unchanged.
pub fn compact_symbol(parsed: &ParsedSource, symbol: &Symbol) -> String {
    let bytes = &parsed.source.as_bytes()[symbol.byte_range.clone()];
    let Ok(text) = std::str::from_utf8(bytes) else {
        return symbol.signature.clone();
    };
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= 1 {
        return text.to_string();
    }
    let base_indent = leading_whitespace(lines[0]);
    // Header = the signature: lines up to (and including) the first block
    // opener (`{` / `:` / ruby keywords); the fold begins at the next body
    // line. Lines with no opener (e.g. a lone decorator) extend the header.
    let mut fold_at = lines.len();
    let mut last_opener: Option<usize> = None;
    let mut seen_opener = false;
    for (i, line) in lines.iter().enumerate() {
        if i > 0 && seen_opener && indented_more(line, base_indent) {
            fold_at = i;
            break;
        }
        if parsed.language.is_opener_line(line) {
            seen_opener = true;
            last_opener = Some(i);
        }
    }
    if fold_at == lines.len() {
        // No deeper-indented body line followed an opener: fold right after
        // the last opener (or after the first line if there was none, e.g.
        // preprocessor macros).
        fold_at = last_opener.map_or(1, |i| i + 1);
    }
    // A fold boundary must not split a syntactic continuation: a kept line
    // ending with an operator/separator (`,`, `+`, `(`, `=` …) — or a next
    // line starting with one (`+ __str._M_check(...)`) — still needs its
    // lines together. Slide the fold point past any such boundary. Comment
    // lines never continue; neither do lines inside a multi-line string.
    let mut in_string = vec![false; lines.len()];
    let mut state = false;
    for (i, line) in lines.iter().enumerate() {
        state ^= quote_parity(line);
        in_string[i] = state;
    }
    // Cumulative `(`/`[` balance after each line (parens inside strings are
    // skipped): a boundary with pending open parens would orphan them.
    let mut open_parens = vec![0usize; lines.len()];
    let mut balance = 0i64;
    for (i, line) in lines.iter().enumerate() {
        let in_str = i > 0 && in_string[i - 1];
        if !in_str {
            for ch in line.chars() {
                match ch {
                    '(' | '[' => balance += 1,
                    ')' | ']' => balance -= 1,
                    _ => {}
                }
            }
        }
        open_parens[i] = balance.max(0) as usize;
    }
    while fold_at < lines.len()
        && boundary_continues(
            lines[fold_at - 1],
            lines[fold_at],
            in_string[fold_at - 1],
            open_parens[fold_at - 1],
            parsed,
        )
    {
        fold_at += 1;
    }
    let mut rest: Vec<&str> = lines[fold_at..].to_vec();
    if rest.is_empty() {
        return text.to_string();
    }
    // The fold region must not begin with a NEW block opener at a shallower
    // indent than the header's last opener (e.g. a multi-line java
    // annotation preceding `class Foo {`): slide the fold point past the
    // opener line so the header owns it.
    while rest[0].trim_end().ends_with(['{', ':'])
        && leading_whitespace(rest[0]).len()
            < last_opener.map_or(0, |i| leading_whitespace(lines[i]).len())
    {
        fold_at += 1;
        rest = lines[fold_at..].to_vec();
        if rest.is_empty() {
            return text.to_string();
        }
    }
    // Preprocessor directives inside the fold region (e.g. `#ifdef` guards
    // in C function bodies) must stay balanced; folding through them would
    // orphan the `#endif`. Keep the whole symbol instead.
    if rest.iter().any(|l| l.trim_start().starts_with('#')) {
        return text.to_string();
    }
    // Keep a bare closing line so the compact view reads as a complete block.
    // Preprocessor macros are spliced by `\` continuations — the closer
    // would land outside the directive, so never keep it there.
    let last = rest[rest.len() - 1];
    let keep_last = is_closer(last)
        && !lines[0].trim_start().starts_with('#')
        && closer_balances(last, &lines[..fold_at]);
    let omitted = rest.len() - usize::from(keep_last);
    if omitted == 0 {
        return text.to_string();
    }
    let indent = leading_whitespace(rest[0]);
    let mut out = String::new();
    for line in &lines[..fold_at] {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(indent);
    out.push_str(parsed.language.comment_prefix());
    out.push_str(" ... [");
    out.push_str(&omitted.to_string());
    out.push_str(" lines omitted]");
    if keep_last {
        out.push('\n');
        out.push_str(last);
    }
    // Always end on a newline: without one, a trailing `\` continuation
    // (C/C++ preprocessor macros) splices the final line into the directive
    // at EOF and the fragment no longer re-parses.
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Odd-count of unescaped `"` or backtick delimiters in one line. Toggled
/// across lines it tracks multi-line strings (js templates, python docstrings,
/// go raw strings). Single quotes are excluded: char literals and rust
/// lifetimes (`'static`) are always single-line.
fn quote_parity(line: &str) -> bool {
    let mut odd = false;
    let mut escaped = false;
    for ch in line.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
        } else if matches!(ch, '"' | '`') {
            odd = !odd;
        }
    }
    odd
}

/// True if the line is a block closer: it starts with `}`, `]`, `)`
/// (optionally followed by more tokens, as in `} Point;` typedef closes), or
/// is an `end` keyword line (ruby/lua).
fn is_closer(line: &str) -> bool {
    let t = line.trim();
    let c = t.as_bytes().first().copied().unwrap_or(0);
    (c == b'}' || c == b']' || c == b')')
        || t.trim_end_matches(';') == "end"
        || t.starts_with("end ")
}

/// True if the boundary between two lines is mid-expression: the previous
/// line ends with a continuation token (`+`, `,`, `(`, `=` …) or the next
/// line starts with one (`+ __str._M_check(...)`, `&& cond`). A fold marker
/// there would produce invalid syntax. Comment lines never continue — a
/// sentence period at end of line is not a continuation.
fn boundary_continues(
    prev: &str,
    next: &str,
    prev_in_string: bool,
    prev_open_parens: usize,
    parsed: &ParsedSource,
) -> bool {
    let prefix = parsed.language.comment_prefix();
    if prev.trim_start().starts_with(prefix) {
        return false;
    }
    let prev = prev.trim_end();
    if prev.ends_with([
        '+', '-', '*', '/', '%', '=', '(', '[', ',', '&', '|', '~', '^', '<',
    ]) {
        return true;
    }
    // A multi-line string literal (js template, python docstring, go raw
    // string) cannot be folded through — the marker would land inside it.
    if prev_in_string {
        return true;
    }
    // An open `(`/`[` pending across the boundary (multi-line annotations,
    // long call chains) must not be orphaned by the fold marker.
    if prev_open_parens > 0 {
        return true;
    }
    if next
        .trim_start()
        .starts_with(['+', '-', '/', '=', '.', ':', '&', '|', '?', '<'])
    {
        return true;
    }
    // A lone token at the end of the line (`__extension__`, `type`,
    // `return`) is incomplete — it needs the next line.
    let prev_t = prev.trim();
    !prev_t.is_empty()
        && !prev_t.ends_with(';')
        && !prev_t.contains(char::is_whitespace)
        && !parsed.language.is_opener_line(prev_t)
        && !is_closer(prev_t)
}

/// For `)`/`]` closers, the kept header must contain the matching opener;
/// otherwise the closer would be an orphan (e.g. a multi-line `return (…)`
/// folded to `def f(): … )`).
fn closer_balances(closer: &str, header: &[&str]) -> bool {
    let t = closer.trim();
    let (open, close) = if t.starts_with(')') {
        ('(', ')')
    } else if t.starts_with(']') {
        ('[', ']')
    } else {
        return true;
    };
    let (mut o, mut c) = (0usize, 0usize);
    for line in header {
        for ch in line.chars() {
            if ch == open {
                o += 1;
            } else if ch == close {
                c += 1;
            }
        }
    }
    o > c
}

/// True if the line is indented deeper than the reference prefix.
fn indented_more(line: &str, base: &str) -> bool {
    leading_whitespace(line).len() > base.len()
}

/// Leading spaces/tabs of a line (byte-safe for ASCII whitespace).
fn leading_whitespace(line: &str) -> &str {
    let len = line.len() - line.trim_start_matches([' ', '\t']).len();
    &line[..len]
}
