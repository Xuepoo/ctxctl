//! Rust language backend.

use crate::language::Language;
use crate::symbol::SymbolKind;
use std::path::Path;

pub struct RustLang;

impl Language for RustLang {
    fn name(&self) -> &'static str {
        "rust"
    }

    fn grammar(&self) -> tree_sitter::Language {
        tree_sitter_rust::LANGUAGE.into()
    }

    fn supports_path(&self, path: &Path) -> bool {
        path.extension().map(|e| e == "rs").unwrap_or(false)
    }

    fn definition_node_types(&self) -> &[(&'static str, SymbolKind)] {
        &[
            ("function_item", SymbolKind::Function),
            ("struct_item", SymbolKind::Struct),
            ("enum_item", SymbolKind::Enum),
            ("trait_item", SymbolKind::Trait),
            ("mod_item", SymbolKind::Type),
            ("type_item", SymbolKind::Type),
            ("const_item", SymbolKind::Const),
            ("static_item", SymbolKind::Variable),
        ]
    }

    fn symbol_name(&self, node: &tree_sitter::Node, source: &str) -> Option<String> {
        // Use the `name` grammar field — precise, avoids impl blocks (which
        // have no name field) and identifiers inside bodies.
        let name_node = node.child_by_field_name("name")?;
        name_node
            .utf8_text(source.as_bytes())
            .ok()
            .map(|s| s.trim().to_string())
    }

    fn signature(&self, node: &tree_sitter::Node, source: &str) -> String {
        let start = node.start_byte();
        let text = source
            .get(start..node.end_byte().min(source.len()))
            .unwrap_or("…");
        let line = text.split('\n').next().unwrap_or("").trim();
        if line.is_empty() {
            "…".to_string()
        } else {
            line.to_string()
        }
    }

    fn has_doc_comment(&self, _node: &tree_sitter::Node) -> bool {
        true
    }
}
