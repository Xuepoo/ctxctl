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
    // opener (`{` / `:`); the fold begins at the next body line. Lines with
    // no opener (e.g. a lone decorator) extend the header.
    let mut fold_at = lines.len();
    let mut seen_opener = false;
    for (i, line) in lines.iter().enumerate() {
        if i > 0 && seen_opener && indented_more(line, base_indent) {
            fold_at = i;
            break;
        }
        if line.trim_end().ends_with(['{', ':']) {
            seen_opener = true;
        }
    }
    if fold_at == lines.len() {
        // No opener: fold everything after the first line.
        fold_at = 1;
    }
    let rest: Vec<&str> = lines[fold_at..].to_vec();
    if rest.is_empty() {
        return text.to_string();
    }
    // Keep a bare closing line so the compact view reads as a complete block.
    let last = rest[rest.len() - 1];
    let keep_last = is_closer(last);
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
    out
}

/// True if the line is a bare block closer (`}`, `]`, `)`).
fn is_closer(line: &str) -> bool {
    matches!(line.trim(), "}" | "]" | ")")
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
