//! Markdown language backend.
//!
//! ATX (`#`) and setext (underline) headings are the symbols. A heading's
//! slice spans its **whole section** — from the heading line to just before
//! the next heading of any level — so `symbol --name "Chapter"` extracts the
//! entire chapter, not only its title line. The tree-sitter-md grammar
//! provides this for free: every heading lives inside a `section` node that
//! ends exactly there.

use crate::language::Language;
use crate::symbol::SymbolKind;
use std::path::Path;

pub struct MarkdownLang;

impl Language for MarkdownLang {
    fn name(&self) -> &'static str {
        "markdown"
    }

    fn grammar(&self) -> tree_sitter::Language {
        tree_sitter_md::LANGUAGE.into()
    }

    fn supports_path(&self, path: &Path) -> bool {
        matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("md") | Some("markdown")
        )
    }

    fn definition_node_types(&self) -> &[(&'static str, SymbolKind)] {
        &[
            ("atx_heading", SymbolKind::Heading),
            ("setext_heading", SymbolKind::Heading),
        ]
    }

    /// Heading text: the first `inline` descendant, minus a leading
    /// link/anchor decoration if present.
    fn symbol_name(&self, node: &tree_sitter::Node, source: &str) -> Option<String> {
        let inline = first_inline(*node)?;
        let text = inline.utf8_text(source.as_bytes()).ok()?;
        let cleaned = text.trim().trim_start_matches('#').trim();
        if cleaned.is_empty() {
            return None;
        }
        Some(cleaned.to_string())
    }

    /// Extend the heading's range to its enclosing `section`, i.e. through
    /// the whole chapter including nested subsections.
    fn definition_byte_range(&self, node: &tree_sitter::Node) -> std::ops::Range<usize> {
        match node.parent() {
            Some(section) if section.kind() == "section" => section.byte_range(),
            _ => node.byte_range(),
        }
    }

    /// No brace blocks; compact views pass headings through unfolded.
    fn keeps_brace_closers(&self) -> bool {
        false
    }

    fn comment_prefix(&self) -> &'static str {
        ""
    }

    /// Markdown has no line comments; nothing above a heading is one.
    fn has_doc_comment(&self, _node: &tree_sitter::Node) -> bool {
        false
    }
}

fn first_inline<'a>(node: tree_sitter::Node<'a>) -> Option<tree_sitter::Node<'a>> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "inline" {
            return Some(child);
        }
        if let Some(found) = first_inline(child) {
            return Some(found);
        }
    }
    None
}
