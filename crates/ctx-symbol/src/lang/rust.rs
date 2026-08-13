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
            ("mod_item", SymbolKind::Module),
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

    fn import_targets(
        &self,
        node: &tree_sitter::Node,
        source: &str,
    ) -> Vec<crate::imports::ImportTarget> {
        (|| {
            match node.kind() {
                "use_declaration" => {
                    // `use` -> argument: use_tree. The `path` field only holds
                    // the first segment, so derive the full target(s) from the
                    // text: `use a::b as c;`, `use a::{b, c};`, `use a::b::*;`
                    // — group items expand to one target each.
                    let tree = node.child_by_field_name("argument")?;
                    let text = tree.utf8_text(source.as_bytes()).ok()?.trim();
                    let mut out = Vec::new();
                    expand_use_target(text, &mut out);
                    if out.is_empty() {
                        return None;
                    }
                    let targets = out
                        .into_iter()
                        .map(|target| {
                            let relative = target == "crate"
                                || target == "super"
                                || target.starts_with("crate::")
                                || target.starts_with("super::")
                                || target.starts_with("self::");
                            crate::imports::ImportTarget { target, relative }
                        })
                        .collect();
                    Some(targets)
                }
                // `mod foo;` declares a same-crate file module; inline modules
                // (with a body) are not file dependencies.
                "mod_item" => {
                    if node.child_by_field_name("body").is_some() {
                        return None;
                    }
                    let name = node.child_by_field_name("name")?;
                    let target = name.utf8_text(source.as_bytes()).ok()?.trim().to_string();
                    Some(vec![crate::imports::ImportTarget {
                        target,
                        relative: true,
                    }])
                }
                "extern_crate_declaration" => {
                    let name = node.child_by_field_name("name")?;
                    let target = name.utf8_text(source.as_bytes()).ok()?.trim().to_string();
                    Some(vec![crate::imports::ImportTarget {
                        target,
                        relative: false,
                    }])
                }
                _ => None,
            }
        })()
        .unwrap_or_default()
    }
}

/// Expand a `use` target into one entry per concrete path: `a::{b, c}` ->
/// `a::b`, `a::c`; `{a::x, b::y}` -> both; `a::b as c` drops the alias;
/// `a::b::*` drops the glob. Nested groups expand recursively.
fn expand_use_target(text: &str, out: &mut Vec<String>) {
    if let Some(idx) = text.find(" as ") {
        expand_use_target(text[..idx].trim(), out);
        return;
    }
    if let Some(idx) = text.find("::{") {
        let prefix = &text[..idx];
        let inner = text[idx + 3..].trim().trim_end_matches(';');
        let inner = inner.strip_suffix('}').unwrap_or(inner);
        for item in split_top_level(inner, '{', '}') {
            expand_use_target(&format!("{prefix}::{item}"), out);
        }
        return;
    }
    if let Some(rest) = text.trim().strip_prefix('{') {
        let inner = rest.strip_suffix('}').unwrap_or(rest);
        for item in split_top_level(inner, '{', '}') {
            expand_use_target(item.trim(), out);
        }
        return;
    }
    let target = text.trim().trim_end_matches("::*").trim();
    if !target.is_empty() {
        out.push(target.to_string());
    }
}

/// Split on top-level commas (commas outside nested braces).
fn split_top_level(text: &str, open: char, close: char) -> Vec<&str> {
    let mut depth = 0usize;
    let mut start = 0usize;
    let mut out = Vec::new();
    for (i, ch) in text.char_indices() {
        match ch {
            c if c == open => depth += 1,
            c if c == close => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                out.push(text[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    let last = text[start..].trim();
    if !last.is_empty() {
        out.push(last);
    }
    out
}
