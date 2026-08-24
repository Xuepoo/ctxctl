//! Go language backend.

use crate::language::Language;
use crate::symbol::SymbolKind;
use std::path::Path;

pub struct GoLang;

impl Language for GoLang {
    fn name(&self) -> &'static str {
        "go"
    }

    fn grammar(&self) -> tree_sitter::Language {
        tree_sitter_go::LANGUAGE.into()
    }

    fn supports_path(&self, path: &Path) -> bool {
        path.extension().map(|e| e == "go").unwrap_or(false)
    }

    fn body_node_kinds(&self) -> &[&'static str] {
        &["block", "field_declaration_list", "literal_value"]
    }

    fn definition_node_types(&self) -> &[(&'static str, SymbolKind)] {
        // `type_declaration` / `var_declaration` wrap the spec nodes below,
        // which carry the `name` field; the specs are extracted individually.
        &[
            ("function_declaration", SymbolKind::Function),
            ("method_declaration", SymbolKind::Method),
            ("type_spec", SymbolKind::Type),
            ("const_spec", SymbolKind::Const),
            ("var_spec", SymbolKind::Variable),
        ]
    }

    fn symbol_name(&self, node: &tree_sitter::Node, source: &str) -> Option<String> {
        let name_node = node.child_by_field_name("name")?;
        name_node
            .utf8_text(source.as_bytes())
            .ok()
            .map(|s| s.trim().to_string())
    }

    fn has_doc_comment(&self, _node: &tree_sitter::Node) -> bool {
        true
    }

    fn import_node_types(&self) -> &[&'static str] {
        // `import_spec` appears in both `import "x"` and `import (...)` forms.
        &["import_spec"]
    }

    fn import_targets(
        &self,
        node: &tree_sitter::Node,
        source: &str,
    ) -> Vec<crate::imports::ImportTarget> {
        let path = node.child_by_field_name("path");
        let Some(path) = path else { return Vec::new() };
        let Some(text) = path.utf8_text(source.as_bytes()).ok() else {
            return Vec::new();
        };
        let target = text.trim().trim_matches(['"', '`']).to_string();
        if target.is_empty() {
            return Vec::new();
        }
        vec![crate::imports::ImportTarget {
            target,
            relative: false,
        }]
    }
}
