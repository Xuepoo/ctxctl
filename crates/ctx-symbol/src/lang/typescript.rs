//! TypeScript / JavaScript language backend.

use crate::language::Language;
use crate::symbol::SymbolKind;
use std::path::Path;

pub struct TypeScriptLang;

impl Language for TypeScriptLang {
    fn name(&self) -> &'static str {
        "typescript"
    }

    fn grammar(&self) -> tree_sitter::Language {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
    }

    fn supports_path(&self, path: &Path) -> bool {
        matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("ts" | "tsx" | "mts" | "cts")
        )
    }

    fn definition_node_types(&self) -> &[(&'static str, SymbolKind)] {
        &[
            ("function_declaration", SymbolKind::Function),
            ("class_declaration", SymbolKind::Class),
            ("method_definition", SymbolKind::Method),
            ("interface_declaration", SymbolKind::Interface),
            ("type_alias_declaration", SymbolKind::Type),
            ("enum_declaration", SymbolKind::Enum),
            ("lexical_declaration", SymbolKind::Variable),
            ("variable_declaration", SymbolKind::Variable),
            ("abstract_class_declaration", SymbolKind::Class),
        ]
    }

    fn symbol_name(&self, node: &tree_sitter::Node, source: &str) -> Option<String> {
        // The `name` grammar field is precise for function/class/interface/
        // enum/type declarations; method_definition also exposes `name`.
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
