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

    /// The tree-sitter grammar for a specific path. Defaults to
    /// [`Self::grammar`]; backends with per-extension grammars (e.g.
    /// TypeScript's TSX grammar for `.tsx`) override this.
    fn grammar_for_path(&self, _path: &Path) -> tree_sitter::Language {
        self.grammar()
    }

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

    /// Byte range of a definition node; defaults to the node itself.
    /// Backends extend it to include attached syntax (e.g. python decorators).
    fn definition_byte_range(&self, node: &tree_sitter::Node) -> std::ops::Range<usize> {
        node.byte_range()
    }

    /// Line-comment prefix used by compact views for fold markers.
    fn comment_prefix(&self) -> &'static str {
        "//"
    }

    /// True if `}`/`)`/`]` lines may be kept as block closers in compact
    /// views. False for languages without brace/paren block syntax (python:
    /// indentation; ruby: `end`).
    fn keeps_brace_closers(&self) -> bool {
        true
    }

    /// Kinds of a definition's foldable body node (e.g. `block`,
    /// `compound_statement`, `field_declaration_list`). Used by the generic
    /// AST-anchored fold locator; empty means "no body nodes" and the fold
    /// falls back to the line heuristic.
    fn body_node_kinds(&self) -> &[&'static str] {
        &[]
    }

    /// 1-based source line where the definition's body begins (the first
    /// line after the signature), when the backend can anchor it in the AST.
    /// Backends with indentation-based blocks (python) must provide this —
    /// line heuristics cannot tell `def f():  # comment` (a signature with a
    /// trailing comment, which does not end in `:`) from docstring prose
    /// like `Args:`.
    fn body_start_line(&self, _parsed: &ParsedSource, _node: &tree_sitter::Node) -> Option<usize> {
        None
    }

    /// True if a line opens a block, so the compact view folds after it.
    ///
    /// Default: lines ending with `{` plus lines starting with `{` (braces on
    /// their own line, e.g. C/C++ one-line bodies). `:` is NOT an opener here
    /// — it signals indentation blocks (python), which override this. Ruby
    /// overrides with its keyword openers (`def`, `class`, `if`, …).
    fn is_opener_line(&self, line: &str) -> bool {
        let t = line.trim();
        t.ends_with('{') || t.starts_with('{')
    }

    /// Doc comment immediately above the definition node, if any.
    ///
    /// Default: a comment-kind sibling scan. Backends with richer comment
    /// idioms (e.g. Python docstrings) override this.
    fn doc_comment(&self, parsed: &ParsedSource, node: &tree_sitter::Node) -> Option<String> {
        doc_comment_above(parsed, node)
    }

    /// Node kinds that represent imports. Nodes of these kinds are visited by
    /// [`crate::imports::extract_imports`] and passed to [`Self::import_targets`].
    fn import_node_types(&self) -> &[&'static str] {
        &[]
    }

    /// Derive the import targets from an import node (see
    /// [`Self::import_node_types`]). A single statement may carry several
    /// targets (e.g. python `import os, sys`). Return an empty vec for nodes
    /// that are not actually imports.
    fn import_targets(
        &self,
        _node: &tree_sitter::Node,
        _source: &str,
    ) -> Vec<crate::imports::ImportTarget> {
        Vec::new()
    }
}

/// Parse source text with the grammar for the given path.
pub fn parse(path: &Path, source: &str) -> Result<ParsedSource, SymbolError> {
    let lang = detect_language(path)
        .ok_or_else(|| SymbolError::UnsupportedLanguage(path.display().to_string()))?;
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&lang.grammar_for_path(path))
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
    &crate::lang::python::PythonLang,
    &crate::lang::go::GoLang,
    &crate::lang::javascript::JavaScriptLang,
    &crate::lang::java::JavaLang,
    &crate::lang::c::CLang,
    &crate::lang::cpp::CppLang,
    &crate::lang::csharp::CSharpLang,
    &crate::lang::ruby::RubyLang,
    &crate::lang::lua::LuaLang,
];

/// True if the node kind is a definition type for the given language.
pub fn is_definition(lang: &dyn Language, kind: &str) -> Option<SymbolKind> {
    lang.definition_node_types()
        .iter()
        .find(|(t, _)| *t == kind)
        .map(|(_, k)| *k)
}

/// Normalize a raw signature for display. Deterministic, applied uniformly
/// across all backends so outline rows stay compact:
///
/// - skip leading attribute / decorator-only lines (`#[...]`, `@decorator`)
/// - drop a trailing comment (started by `//` or `/*` after code)
/// - collapse internal whitespace runs
/// - strip trailing continuation delimiters (`(`, `{`, `,`, `;`, `:`, `=`,
///   `->`, `=>`) left by declarations that span multiple lines
/// - cap at [`MAX_SIGNATURE`] chars with an ellipsis
pub(crate) fn clean_signature(raw: &str) -> String {
    const MAX_SIGNATURE: usize = 120;

    let line = raw
        .lines()
        .map(str::trim)
        .find(|t| !(t.starts_with("#[") || is_annotation_only(t)))
        .unwrap_or("");
    let line = strip_trailing_comment(line);

    let mut sig = line.split_whitespace().collect::<Vec<_>>().join(" ");
    loop {
        let mut changed = false;
        for suffix in ["=>", "->"] {
            if let Some(t) = sig.strip_suffix(suffix) {
                sig = t.trim_end().to_string();
                changed = true;
            }
        }
        if sig.ends_with(['(', '{', ',', ';', ':', '=']) {
            sig = sig
                .trim_end_matches(['(', '{', ',', ';', ':', '='])
                .trim_end()
                .to_string();
            changed = true;
        }
        if !changed {
            break;
        }
    }

    if sig.len() > MAX_SIGNATURE {
        let mut cut = MAX_SIGNATURE - 1;
        while cut > 0 && !sig.is_char_boundary(cut) {
            cut -= 1;
        }
        sig.truncate(cut);
        sig.push('…');
    }
    if sig.is_empty() {
        "…".to_string()
    } else {
        sig
    }
}

/// A lone `@decorator` / `@decorator(args)` line with nothing else on it.
fn is_annotation_only(t: &str) -> bool {
    let Some(rest) = t.strip_prefix('@') else {
        return false;
    };
    if rest.is_empty() {
        return true;
    }
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '.';
    if let Some(after) = rest.find('(') {
        rest[..after].chars().all(ident) && rest.ends_with(')')
    } else {
        !rest.contains(char::is_whitespace) && rest.chars().all(ident)
    }
}

/// Cut a `//` or `/*` comment when it starts after code (preceded by
/// whitespace), leaving URL strings untouched.
fn strip_trailing_comment(line: &str) -> &str {
    let cut_at = |marker: &str| {
        line.match_indices(marker).find_map(|(i, _)| {
            let preceded_by_ws = line[..i].ends_with(char::is_whitespace);
            (i == 0 || preceded_by_ws).then_some(i)
        })
    };
    let cut = cut_at("//").into_iter().chain(cut_at("/*")).min();
    match cut {
        Some(i) => line[..i].trim_end(),
        None => line,
    }
}

/// Walk a tree and collect all definition symbols in source order.
pub fn extract_symbols(parsed: &ParsedSource) -> Vec<Symbol> {
    let mut out = Vec::new();
    collect_definitions(parsed, parsed.tree.root_node(), &mut out);
    out
}

fn collect_definitions(parsed: &ParsedSource, node: tree_sitter::Node, out: &mut Vec<Symbol>) {
    let kind = node.kind();
    if let Some(sym_kind) = is_definition(parsed.language, kind)
        && let Some(name) = parsed.language.symbol_name(&node, &parsed.source)
    {
        let range = parsed.language.definition_byte_range(&node);
        let sig = clean_signature(&parsed.language.signature(&node, &parsed.source));
        let (start, end) = (node.start_position(), node.end_position());
        let doc = parsed.language.doc_comment(parsed, &node);
        out.push(Symbol {
            name,
            kind: sym_kind,
            start_line: start.row + 1,
            end_line: end.row + 1,
            byte_range: range,
            signature: sig,
            doc_comment: doc,
        });
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
/// override via their own [`Language::doc_comment`].
pub(crate) fn doc_comment_above(parsed: &ParsedSource, node: &tree_sitter::Node) -> Option<String> {
    if !parsed.language.has_doc_comment(node) {
        return None;
    }
    let mut prev = node.prev_sibling();
    let mut depth = 0;
    while let Some(sib) = prev {
        if sib.kind().contains("comment") {
            let text = strip_comment_markers(sib.utf8_text(parsed.source.as_bytes()).ok()?);
            if text.is_empty() {
                return None;
            }
            return Some(text);
        }
        depth += 1;
        if depth > 2 {
            break;
        }
        prev = sib.prev_sibling();
    }
    None
}

/// Strip comment markers from a doc-comment node's text: `///`/`//!`/`//`,
/// `/** */`/`/*! */`, or `#`, returning the plain prose.
fn strip_comment_markers(text: &str) -> String {
    let t = text.trim();
    let inner = if t.starts_with("/*") {
        t.strip_prefix("/*")
            .unwrap_or(t)
            .strip_suffix("*/")
            .unwrap_or(t)
    } else if let Some(rest) = t.strip_prefix("--") {
        rest
    } else if let Some(rest) = t.strip_prefix("//") {
        rest
    } else if let Some(rest) = t.strip_prefix('#') {
        rest
    } else {
        t
    };
    inner
        .lines()
        .map(|l| {
            l.trim()
                .trim_start_matches(['*', '/', '!'])
                .trim()
                .to_string()
        })
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::clean_signature;

    #[test]
    fn strips_trailing_continuation_delimiters() {
        assert_eq!(clean_signature("pub fn run_exec("), "pub fn run_exec");
        assert_eq!(clean_signature("struct Cli {"), "struct Cli");
        assert_eq!(
            clean_signature("fn main() -> ExitCode {"),
            "fn main() -> ExitCode"
        );
        assert_eq!(clean_signature("mod config;"), "mod config");
        assert_eq!(clean_signature("def validate(cfg):"), "def validate(cfg)");
        assert_eq!(clean_signature("const f = (x) =>"), "const f = (x)");
        assert_eq!(
            clean_signature("pub fn add(a: i32, b: i32) -> i32 {"),
            "pub fn add(a: i32, b: i32) -> i32"
        );
    }

    #[test]
    fn skips_attribute_and_decorator_lines() {
        assert_eq!(
            clean_signature("#[derive(Debug)]\npub struct Config {"),
            "pub struct Config"
        );
        assert_eq!(clean_signature("@dataclass\nclass Point:"), "class Point");
    }

    #[test]
    fn drops_trailing_comments_and_collapses_whitespace() {
        assert_eq!(
            clean_signature("pub  fn   foo() { // does things"),
            "pub fn foo()"
        );
        assert_eq!(
            clean_signature("const URL = \"https://example.com\";"),
            "const URL = \"https://example.com\""
        );
    }

    #[test]
    fn caps_long_signatures() {
        let sig = clean_signature(&format!("fn very_long_name{}(", "x".repeat(300)));
        assert!(sig.ends_with('…'));
        assert!(sig.chars().count() <= 120);
    }
}
