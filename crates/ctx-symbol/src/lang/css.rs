//! CSS / SCSS language backend.
//!
//! Rulesets are the symbols: the selector list is the name, the declaration
//! block is the body. `@media`/`@supports` blocks nest their inner rulesets,
//! which the recursive walk finds anyway. `.scss` files are routed here as a
//! best effort — SCSS-specific syntax (nesting, `$variables`, mixins) yields
//! tree-sitter ERROR nodes but plain-CSS rulesets still extract.

use crate::language::Language;
use crate::symbol::SymbolKind;
use std::path::Path;

pub struct CssLang;

impl Language for CssLang {
    fn name(&self) -> &'static str {
        "css"
    }

    fn grammar(&self) -> tree_sitter::Language {
        tree_sitter_css::LANGUAGE.into()
    }

    fn supports_path(&self, path: &Path) -> bool {
        matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("css") | Some("scss")
        )
    }

    fn definition_node_types(&self) -> &[(&'static str, SymbolKind)] {
        &[("rule_set", SymbolKind::Rule)]
    }

    fn symbol_name(&self, node: &tree_sitter::Node, source: &str) -> Option<String> {
        // The css grammar names no fields; the selector list is the first
        // `selectors` child.
        let selectors = node
            .children(&mut node.walk())
            .find(|c| c.kind() == "selectors")?;
        let text = selectors
            .utf8_text(source.as_bytes())
            .ok()?
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if text.is_empty() {
            return None;
        }
        Some(text)
    }

    fn signature(&self, node: &tree_sitter::Node, source: &str) -> String {
        // The signature is the full selector list; keep it on one line.
        self.symbol_name(node, source).unwrap_or_else(|| "…".into())
    }

    fn body_node_kinds(&self) -> &[&'static str] {
        &["block"]
    }

    fn comment_prefix(&self) -> &'static str {
        "/*"
    }

    fn comment_close(&self) -> &'static str {
        "*/"
    }
}
