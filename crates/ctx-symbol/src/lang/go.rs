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

    fn import_node_types(&self) -> &[&'static str] {
        // `import_spec` appears in both `import "x"` and `import (...)` forms.
        &["import_spec"]
    }

    fn import_target(
        &self,
        node: &tree_sitter::Node,
        source: &str,
    ) -> Option<crate::imports::ImportTarget> {
        let path = node.child_by_field_name("path")?;
        let text = path.utf8_text(source.as_bytes()).ok()?.trim();
        let target = text.trim_matches(['"', '`']).to_string();
        if target.is_empty() {
            return None;
        }
        Some(crate::imports::ImportTarget {
            target,
            relative: false,
        })
    }
}
