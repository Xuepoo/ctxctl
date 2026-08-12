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

    fn import_node_types(&self) -> &[&'static str] {
        &["use_declaration", "mod_item", "extern_crate_declaration"]
    }

    fn import_target(
        &self,
        node: &tree_sitter::Node,
        source: &str,
    ) -> Option<crate::imports::ImportTarget> {
        match node.kind() {
            "use_declaration" => {
                // `use` -> argument: use_tree. The `path` field only holds the
                // first segment, so derive the full target from the text and
                // trim aliases / groups / globs: `use a::b as c;`,
                // `use a::{b, c};`, `use a::b::*;`.
                let tree = node.child_by_field_name("argument")?;
                let mut target = tree.utf8_text(source.as_bytes()).ok()?.trim().to_string();
                if let Some(idx) = target.find(" as ") {
                    target.truncate(idx);
                }
                if let Some(idx) = target.find("::{") {
                    target.truncate(idx);
                } else if let Some(idx) = target.find("::*") {
                    target.truncate(idx);
                }
                // Pathless group: `use {a::x, b::y};` -> first item.
                if target.starts_with('{') {
                    target = target
                        .strip_prefix('{')
                        .and_then(|rest| rest.split(',').next())
                        .unwrap_or("")
                        .trim()
                        .to_string();
                }
                let relative = target == "crate"
                    || target == "super"
                    || target.starts_with("crate::")
                    || target.starts_with("super::")
                    || target.starts_with("self::");
                Some(crate::imports::ImportTarget { target, relative })
            }
            // `mod foo;` declares a same-crate file module; inline modules
            // (with a body) are not file dependencies.
            "mod_item" => {
                if node.child_by_field_name("body").is_some() {
                    return None;
                }
                let name = node.child_by_field_name("name")?;
                let target = name.utf8_text(source.as_bytes()).ok()?.trim().to_string();
                Some(crate::imports::ImportTarget {
                    target,
                    relative: true,
                })
            }
            "extern_crate_declaration" => {
                let name = node.child_by_field_name("name")?;
                let target = name.utf8_text(source.as_bytes()).ok()?.trim().to_string();
                Some(crate::imports::ImportTarget {
                    target,
                    relative: false,
                })
            }
            _ => None,
        }
    }
}
