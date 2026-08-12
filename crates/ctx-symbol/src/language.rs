//! Language-agnostic symbol extraction framework.
//!
//! The engine core is language-independent. Each language backend implements
//! [`Language`] and declares how to map tree-sitter node types to [`SymbolKind`]
//! and extract names/signatures. Adding a language = adding one backend module.

use crate::symbol::{Symbol, SymbolKind};
use std::path::Path;

/// Errors returned by the symbol engine.
#[derive(Debug, thiserror::Error)]
pub enum SymbolError {
    #[error("unsupported language for path: {0}")]
    UnsupportedLanguage(String),
    #[error("failed to parse source: {0}")]
    Parse(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("symbol not found: {0}")]
    NotFound(String),
}

/// A tree-sitter grammar paired with a source text, ready for extraction.
pub struct ParsedSource {
    pub tree: tree_sitter::Tree,
    pub source: String,
    pub language: &'static dyn Language,
}

/// Interface every language backend must implement.
pub trait Language: Send + Sync {
    /// Human-readable language name (e.g. "rust").
    fn name(&self) -> &'static str;

    /// The tree-sitter grammar for this language.
    fn grammar(&self) -> tree_sitter::Language;

    /// Return true if this backend handles the given file path.
    fn supports_path(&self, path: &Path) -> bool;

    /// Node types that represent definition-like symbols, mapped to kinds.
    /// Node types not listed here are ignored during extraction.
    fn definition_node_types(&self) -> &[(&'static str, SymbolKind)];

    /// Given a definition node, produce its symbol name. Falls back to
    /// scanning child nodes for an identifier.
    fn symbol_name(&self, node: &tree_sitter::Node, source: &str) -> Option<String>;

    /// Produce the "signature" line(s) for a definition node — usually the
    /// first line or a compact header.
    fn signature(&self, node: &tree_sitter::Node, source: &str) -> String;

    /// Return true if `node` may carry a doc comment immediately above it.
    /// Backends override to handle comment idioms (e.g. `//`, `///`, `/** */`).
    fn has_doc_comment(&self, _node: &tree_sitter::Node) -> bool {
        false
    }
}

/// Parse source text with the grammar for the given path.
pub fn parse(path: &Path, source: &str) -> Result<ParsedSource, SymbolError> {
    let lang = detect_language(path)
        .ok_or_else(|| SymbolError::UnsupportedLanguage(path.display().to_string()))?;
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&lang.grammar())
        .map_err(|e| SymbolError::Parse(e.to_string()))?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| SymbolError::Parse("tree-sitter returned no tree".into()))?;
    Ok(ParsedSource {
        tree,
        source: source.to_string(),
        language: lang,
    })
}

/// Detect the language backend for a path, if any.
pub fn detect_language(path: &Path) -> Option<&'static dyn Language> {
    REGISTRY.iter().find(|l| l.supports_path(path)).copied()
}

/// All registered language backends. Add new languages here.
pub static REGISTRY: &[&'static dyn Language] = &[
    &crate::lang::rust::RustLang,
    &crate::lang::typescript::TypeScriptLang,
];

/// True if the node kind is a definition type for the given language.
pub fn is_definition(lang: &dyn Language, kind: &str) -> Option<SymbolKind> {
    lang.definition_node_types()
        .iter()
        .find(|(t, _)| *t == kind)
        .map(|(_, k)| *k)
}

/// Walk a tree and collect all definition symbols in source order.
pub fn extract_symbols(parsed: &ParsedSource) -> Vec<Symbol> {
    let mut out = Vec::new();
    collect_definitions(parsed, parsed.tree.root_node(), &mut out);
    out
}

fn collect_definitions(parsed: &ParsedSource, node: tree_sitter::Node, out: &mut Vec<Symbol>) {
    let kind = node.kind();
    if let Some(sym_kind) = is_definition(parsed.language, kind) {
        if let Some(name) = parsed.language.symbol_name(&node, &parsed.source) {
            let range = node.byte_range();
            let sig = parsed.language.signature(&node, &parsed.source);
            let (start, end) = (node.start_position(), node.end_position());
            out.push(Symbol {
                name,
                kind: sym_kind,
                start_line: start.row + 1,
                end_line: end.row + 1,
                byte_range: range,
                signature: sig,
                doc_comment: doc_comment_above(parsed, &node),
            });
        }
    }
    if node.child_count() > 0 {
        let mut child = node.walk();
        for kid in node.children(&mut child) {
            collect_definitions(parsed, kid, out);
        }
    }
}

/// Look one level up in the tree for a doc-comment sibling immediately before
/// the definition node. Best-effort; backends with richer comment handling can
/// override via their own walk.
fn doc_comment_above(parsed: &ParsedSource, node: &tree_sitter::Node) -> Option<String> {
    if !parsed.language.has_doc_comment(node) {
        return None;
    }
    let mut prev = node.prev_sibling();
    let mut depth = 0;
    while let Some(sib) = prev {
        if sib.kind().contains("comment") {
            let text = sib.utf8_text(parsed.source.as_bytes()).ok()?.trim();
            return Some(text.to_string());
        }
        depth += 1;
        if depth > 2 {
            break;
        }
        prev = sib.prev_sibling();
    }
    None
}
