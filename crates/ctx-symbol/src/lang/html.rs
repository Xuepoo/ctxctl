//! HTML language backend.
//!
//! Elements carrying an `id` attribute are the symbols (name = id value) —
//! the anchor an agent can jump back to. Plain structural tags are noise and
//! are skipped. `.htm` is accepted alongside `.html`.

use crate::language::Language;
use crate::symbol::SymbolKind;
use std::path::Path;

pub struct HtmlLang;

impl Language for HtmlLang {
    fn name(&self) -> &'static str {
        "html"
    }

    fn grammar(&self) -> tree_sitter::Language {
        tree_sitter_html::LANGUAGE.into()
    }

    fn supports_path(&self, path: &Path) -> bool {
        matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("html") | Some("htm")
        )
    }

    fn definition_node_types(&self) -> &[(&'static str, SymbolKind)] {
        &[("element", SymbolKind::Element)]
    }

    fn symbol_name(&self, node: &tree_sitter::Node, source: &str) -> Option<String> {
        let start_tag = node
            .children(&mut node.walk())
            .find(|c| c.kind() == "start_tag" || c.kind() == "self_closing_tag")?;
        for child in start_tag.children(&mut start_tag.walk()) {
            if child.kind() != "attribute" {
                continue;
            }
            // An `attribute` is `[attribute_name] [attribute_value]?`; the
            // value may be bare or quoted.
            let named: Vec<tree_sitter::Node> = child
                .children(&mut child.walk())
                .filter(|c| c.is_named())
                .collect();
            let Some(name_node) = named.first() else {
                continue;
            };
            if name_node.utf8_text(source.as_bytes()).ok()? != "id" {
                continue;
            }
            let value_node = named.get(1)?;
            let cleaned = value_node
                .utf8_text(source.as_bytes())
                .ok()?
                .trim()
                .trim_matches(['"', '\''])
                .trim();
            if cleaned.is_empty() {
                return None;
            }
            return Some(cleaned.to_string());
        }
        None
    }
}
