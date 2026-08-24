//! Java language backend.

use crate::language::Language;
use crate::symbol::SymbolKind;
use std::path::Path;

pub struct JavaLang;

impl Language for JavaLang {
    fn name(&self) -> &'static str {
        "java"
    }

    fn grammar(&self) -> tree_sitter::Language {
        tree_sitter_java::LANGUAGE.into()
    }

    fn supports_path(&self, path: &Path) -> bool {
        path.extension().map(|e| e == "java").unwrap_or(false)
    }

    fn body_node_kinds(&self) -> &[&'static str] {
        &["block", "class_body", "interface_body", "enum_body"]
    }

    fn definition_node_types(&self) -> &[(&'static str, SymbolKind)] {
        &[
            ("class_declaration", SymbolKind::Class),
            ("record_declaration", SymbolKind::Class),
            ("interface_declaration", SymbolKind::Interface),
            ("enum_declaration", SymbolKind::Enum),
            ("annotation_type_declaration", SymbolKind::Type),
            ("method_declaration", SymbolKind::Method),
            ("constructor_declaration", SymbolKind::Method),
            ("field_declaration", SymbolKind::Variable),
        ]
    }

    fn symbol_name(&self, node: &tree_sitter::Node, source: &str) -> Option<String> {
        // field_declaration carries its names on variable_declarator children
        // (`int a, b;`); report the first one.
        let name_node = match node.child_by_field_name("name") {
            Some(n) => n,
            None => {
                let declarator = node.child_by_field_name("declarator")?;
                declarator.child_by_field_name("name")?
            }
        };
        name_node
            .utf8_text(source.as_bytes())
            .ok()
            .map(|s| s.trim().to_string())
    }

    fn has_doc_comment(&self, _node: &tree_sitter::Node) -> bool {
        true
    }

    fn import_node_types(&self) -> &[&'static str] {
        &["import_declaration"]
    }

    fn import_targets(
        &self,
        node: &tree_sitter::Node,
        source: &str,
    ) -> Vec<crate::imports::ImportTarget> {
        // `import java.util.List;` / `import static java.lang.Math.PI;` /
        // `import java.util.*;` — target is the first path child; static
        // imports drop the member (`java.lang.Math.PI` -> `java.lang.Math`).
        let mut is_static = false;
        let mut child = node.walk();
        for kid in node.children(&mut child) {
            if kid.kind() == "static" {
                is_static = true;
                continue;
            }
            if matches!(kid.kind(), "scoped_identifier" | "identifier")
                && let Ok(text) = kid.utf8_text(source.as_bytes())
            {
                let mut target = text.trim().to_string();
                if is_static && let Some(idx) = target.rfind('.') {
                    target.truncate(idx);
                }
                return vec![crate::imports::ImportTarget {
                    target,
                    relative: false,
                }];
            }
        }
        Vec::new()
    }
}
