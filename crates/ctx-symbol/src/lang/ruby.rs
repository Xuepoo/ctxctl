//! Ruby language backend.

use crate::language::Language;
use crate::symbol::SymbolKind;
use std::path::Path;

pub struct RubyLang;

impl Language for RubyLang {
    fn name(&self) -> &'static str {
        "ruby"
    }

    fn grammar(&self) -> tree_sitter::Language {
        tree_sitter_ruby::LANGUAGE.into()
    }

    fn supports_path(&self, path: &Path) -> bool {
        path.extension().map(|e| e == "rb").unwrap_or(false)
    }

    fn definition_node_types(&self) -> &[(&'static str, SymbolKind)] {
        &[
            ("method", SymbolKind::Function),
            ("singleton_method", SymbolKind::Function),
            ("class", SymbolKind::Class),
            ("module", SymbolKind::Type),
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

    fn comment_prefix(&self) -> &'static str {
        "#"
    }

    fn is_opener_line(&self, line: &str) -> bool {
        // Ruby blocks open with keywords, not braces: `def`, `class`,
        // `module`, `if`, `unless`, `begin`, `while`, `until`, `for`,
        // `do`. `case` is deliberately excluded: it requires a `when`
        // before its `end`, which a fold marker cannot satisfy.
        let t = line.trim();
        [
            "def ", "class ", "module ", "if ", "unless ", "begin", "while ", "until ", "for ",
            "do ",
        ]
        .iter()
        .any(|k| t.starts_with(k))
            || t == "begin"
            || t == "do"
            || t.starts_with("class <<")
    }

    fn import_node_types(&self) -> &[&'static str] {
        &["call"]
    }

    fn import_targets(
        &self,
        node: &tree_sitter::Node,
        source: &str,
    ) -> Vec<crate::imports::ImportTarget> {
        (|| {
            // `require 'json'` / `require_relative 'x'` — the method field
            // names the call; targets come from the first string argument.
            let method = node.child_by_field_name("method")?;
            let method = method.utf8_text(source.as_bytes()).ok()?.trim();
            let is_require = matches!(method, "require" | "require_relative");
            if !is_require {
                return None;
            }
            let args = node.child_by_field_name("arguments")?;
            let first = args.named_child(0)?;
            let target = string_value(first, source)?;
            let relative = method == "require_relative"
                || target.starts_with("./")
                || target.starts_with("../");
            Some(vec![crate::imports::ImportTarget { target, relative }])
        })()
        .unwrap_or_default()
    }
}

/// Text of a string literal node without its quotes.
fn string_value(node: tree_sitter::Node, source: &str) -> Option<String> {
    let text = node.utf8_text(source.as_bytes()).ok()?.trim();
    let unquoted = text.strip_prefix(['\'', '"'])?;
    Some(
        unquoted
            .strip_suffix(['\'', '"'])
            .unwrap_or(unquoted)
            .to_string(),
    )
}
