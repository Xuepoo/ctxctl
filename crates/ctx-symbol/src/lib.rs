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
