//! Symbol types shared across the ctx-symbol engine.

use serde::Serialize;
use std::ops::Range;

/// The kind of a code symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SymbolKind {
    Class,
    Struct,
    Enum,
    Interface,
    Function,
    Method,
    Const,
    Variable,
    Trait,
    Type,
}

/// A single symbol located in a source file.
///
/// `byte_range` refers to offsets into the **original source text**, so a
/// consumer can slice the raw bytes back out — preserving comments, formatting,
/// and byte-stability for prompt caching.
#[derive(Debug, Clone, Serialize)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    /// 1-based start line.
    pub start_line: usize,
    /// 1-based end line (inclusive).
    pub end_line: usize,
    /// Byte range into the original source.
    pub byte_range: Range<usize>,
    /// First line of the symbol (typically the signature).
    pub signature: String,
    /// Doc comment immediately above the symbol, if any.
    pub doc_comment: Option<String>,
}

impl Symbol {
    /// Token-approximation of this symbol's cost (4 chars ~ 1 token).
    pub fn estimated_tokens(&self) -> usize {
        self.byte_range.len() / 4
    }
}
