//! C# language backend.

use crate::language::Language;
use crate::symbol::SymbolKind;
use std::path::Path;

pub struct CSharpLang;

impl Language for CSharpLang {
    fn name(&self) -> &'static str {
        "csharp"
    }

    fn grammar(&self) -> tree_sitter::Language {
        tree_sitter_c_sharp::LANGUAGE.into()
    }

    fn supports_path(&self, path: &Path) -> bool {
        path.extension().map(|e| e == "cs").unwrap_or(false)
    }

    fn definition_node_types(&self) -> &[(&'static str, SymbolKind)] {
        &[
            ("class_declaration", SymbolKind::Class),
            ("record_declaration", SymbolKind::Class),
            ("interface_declaration", SymbolKind::Interface),
            ("struct_declaration", SymbolKind::Struct),
            ("enum_declaration", SymbolKind::Enum),
            ("namespace_declaration", SymbolKind::Type),
            ("method_declaration", SymbolKind::Method),
            ("constructor_declaration", SymbolKind::Method),
            ("property_declaration", SymbolKind::Variable),
            ("field_declaration", SymbolKind::Variable),
            ("local_function_statement", SymbolKind::Function),
        ]
    }

    fn symbol_name(&self, node: &tree_sitter::Node, source: &str) -> Option<String> {
        // field_declaration has no name field; names live down the chain
        // field_declaration -> variable_declaration -> variable_declarator.
        let name_node = match node.child_by_field_name("name") {
            Some(n) => n,
            None => {
                let mut cursor = node.walk();
                let var_decl = node
                    .named_children(&mut cursor)
                    .find(|c| c.kind() == "variable_declaration")?;
                let mut cursor = var_decl.walk();
                var_decl
                    .named_children(&mut cursor)
                    .find(|c| c.kind() == "variable_declarator")?
                    .child_by_field_name("name")?
            }
        };
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
        &["using_directive"]
    }

    fn import_targets(
        &self,
        node: &tree_sitter::Node,
        source: &str,
    ) -> Vec<crate::imports::ImportTarget> {
        (|| {
            // `using System;` / `using System.Collections.Generic;` /
            // `using static System.Math;` — the grammar exposes the path via
            // a hidden `_name` rule (no field), so take the last named child.
            // Type aliases (`using Foo = Bar;`) are not imports.
            let text = node.utf8_text(source.as_bytes()).ok()?;
            if text.contains('=') {
                return None;
            }
            let mut cursor = node.walk();
            let name = node.named_children(&mut cursor).last()?;
            let target = name.utf8_text(source.as_bytes()).ok()?.trim().to_string();
            Some(vec![crate::imports::ImportTarget {
                target,
                relative: false,
            }])
        })()
        .unwrap_or_default()
    }
}
