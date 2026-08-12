//! Python language backend.

use crate::language::{Language, ParsedSource, doc_comment_above};
use crate::symbol::SymbolKind;
use std::path::Path;

pub struct PythonLang;

impl Language for PythonLang {
    fn name(&self) -> &'static str {
        "python"
    }

    fn grammar(&self) -> tree_sitter::Language {
        tree_sitter_python::LANGUAGE.into()
    }

    fn supports_path(&self, path: &Path) -> bool {
        matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("py" | "pyi")
        )
    }

    fn definition_node_types(&self) -> &[(&'static str, SymbolKind)] {
        // `decorated_definition` is a wrapper; its inner function/class
        // definition is extracted with a range that excludes the decorators.
        &[
            ("function_definition", SymbolKind::Function),
            ("class_definition", SymbolKind::Class),
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

    fn definition_byte_range(&self, node: &tree_sitter::Node) -> std::ops::Range<usize> {
        // A decorated definition (`@deco` lines) wraps the function/class
        // node; include the decorators so slices carry the full semantics.
        if node
            .parent()
            .is_some_and(|p| p.kind() == "decorated_definition")
        {
            return node.parent().unwrap().byte_range();
        }
        node.byte_range()
    }

    fn doc_comment(&self, parsed: &ParsedSource, node: &tree_sitter::Node) -> Option<String> {
        // Doc comments sit above the decorated_definition, not the wrapped
        // function/class node.
        let effective = if node
            .parent()
            .is_some_and(|p| p.kind() == "decorated_definition")
        {
            node.parent().unwrap()
        } else {
            *node
        };
        // Python idiom: a string-literal expression statement directly above
        // the definition is the docstring (takes priority over comments).
        if let Some(prev) = effective.prev_sibling() {
            if prev.kind() == "expression_statement" {
                // The module docstring is the first statement of the module;
                // it documents the module, not the first symbol below it.
                // Skip comment siblings when checking (comments are not
                // statements in Python semantics).
                let mut prev_statement = prev.prev_sibling();
                while prev_statement.is_some_and(|s| s.kind().contains("comment")) {
                    prev_statement = prev_statement.unwrap().prev_sibling();
                }
                let is_module_doc =
                    prev.parent().is_some_and(|p| p.kind() == "module") && prev_statement.is_none();
                if !is_module_doc {
                    if let Some(first) = prev.named_child(0) {
                        if first.kind() == "string" {
                            if let Some(doc) = string_literal_text(first, &parsed.source) {
                                return Some(doc);
                            }
                        }
                    }
                }
            }
        }
        // `# comment` siblings work through the generic scan.
        doc_comment_above(parsed, node)
    }

    fn import_node_types(&self) -> &[&'static str] {
        &["import_statement", "import_from_statement"]
    }

    fn import_target(
        &self,
        node: &tree_sitter::Node,
        source: &str,
    ) -> Option<crate::imports::ImportTarget> {
        match node.kind() {
            "import_statement" => {
                // `import a.b.c` / `import numpy as np` / `import os, sys`
                // (multi-imports report the first name; the line is shared).
                let name = node.child_by_field_name("name")?;
                let node = if name.kind() == "aliased_import" {
                    name.named_child(0).unwrap_or(name)
                } else {
                    name
                };
                let target = node.utf8_text(source.as_bytes()).ok()?.trim().to_string();
                Some(crate::imports::ImportTarget {
                    target,
                    relative: false,
                })
            }
            "import_from_statement" => {
                // `from a.b import c` / `from . import x` — the module path.
                let module = node.child_by_field_name("module_name")?;
                let target = module.utf8_text(source.as_bytes()).ok()?.trim().to_string();
                Some(crate::imports::ImportTarget {
                    relative: target.starts_with('.'),
                    target,
                })
            }
            _ => None,
        }
    }
}

/// Extract the content of a Python string literal, stripping an optional
/// prefix (`r`, `f`, `rf`, `b`, …) and the surrounding quotes.
fn string_literal_text(node: tree_sitter::Node, source: &str) -> Option<String> {
    let text = node.utf8_text(source.as_bytes()).ok()?.trim();
    let quote_at = text.find(['\'', '"'])?;
    let mut body = &text[quote_at..];
    for quote in ["\"\"\"", "'''", "\"", "'"] {
        if let Some(rest) = body.strip_prefix(quote) {
            body = rest.strip_suffix(quote).unwrap_or(rest);
            return Some(body.trim().to_string());
        }
    }
    Some(body.trim().to_string())
}
