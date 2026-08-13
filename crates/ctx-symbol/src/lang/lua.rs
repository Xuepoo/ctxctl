//! Lua language backend.

use crate::language::Language;
use crate::symbol::SymbolKind;
use std::path::Path;

pub struct LuaLang;

impl Language for LuaLang {
    fn name(&self) -> &'static str {
        "lua"
    }

    fn grammar(&self) -> tree_sitter::Language {
        tree_sitter_lua::LANGUAGE.into()
    }

    fn supports_path(&self, path: &Path) -> bool {
        path.extension().map(|e| e == "lua").unwrap_or(false)
    }

    fn body_node_kinds(&self) -> &[&'static str] {
        &["block"]
    }

    fn definition_node_types(&self) -> &[(&'static str, SymbolKind)] {
        &[
            ("function_declaration", SymbolKind::Function),
            ("variable_declaration", SymbolKind::Variable),
        ]
    }

    fn symbol_name(&self, node: &tree_sitter::Node, source: &str) -> Option<String> {
        let name_node = match node.child_by_field_name("name") {
            Some(n) => n,
            None => {
                // `local x = 1` — names live down the chain:
                // variable_declaration -> assignment_statement ->
                // variable_list -> identifier.
                let mut cursor = node.walk();
                let assignment = node
                    .named_children(&mut cursor)
                    .find(|c| c.kind() == "assignment_statement")?;
                let mut cursor = assignment.walk();
                let list = assignment
                    .named_children(&mut cursor)
                    .find(|c| c.kind() == "variable_list")?;
                let mut cursor = list.walk();
                let first = list.named_children(&mut cursor).next()?;
                first.child_by_field_name("name").unwrap_or(first)
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

    fn comment_prefix(&self) -> &'static str {
        "--"
    }

    fn import_node_types(&self) -> &[&'static str] {
        &["function_call"]
    }

    fn import_targets(
        &self,
        node: &tree_sitter::Node,
        source: &str,
    ) -> Vec<crate::imports::ImportTarget> {
        (|| {
            // `require "x"` / `require("x")` — the name field is "require".
            let name = node.child_by_field_name("name")?;
            if name.utf8_text(source.as_bytes()).ok()?.trim() != "require" {
                return None;
            }
            let args = node.child_by_field_name("arguments")?;
            let first = args.named_child(0)?;
            let target = string_value(first, source)?;
            let relative = target.starts_with("./") || target.starts_with("../");
            Some(vec![crate::imports::ImportTarget { target, relative }])
        })()
        .unwrap_or_default()
    }
}

/// Text of a string literal node without its quotes. Lua `string` nodes
/// expose a `content` child; fall back to quote-stripping.
fn string_value(node: tree_sitter::Node, source: &str) -> Option<String> {
    if node.kind() == "string"
        && let Some(content) = node.child_by_field_name("content")
        && let Ok(text) = content.utf8_text(source.as_bytes())
    {
        return Some(text.trim().to_string());
    }
    let text = node.utf8_text(source.as_bytes()).ok()?.trim();
    let unquoted = text.strip_prefix(['\'', '"'])?;
    Some(
        unquoted
            .strip_suffix(['\'', '"'])
            .unwrap_or(unquoted)
            .to_string(),
    )
}
