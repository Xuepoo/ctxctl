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

    fn body_node_kinds(&self) -> &[&'static str] {
        &["block"]
    }

    fn is_opener_line(&self, line: &str) -> bool {
        let t = line.trim();
        t.ends_with(['{', ':']) || t.starts_with('{')
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

    fn comment_prefix(&self) -> &'static str {
        "#"
    }

    fn keeps_brace_closers(&self) -> bool {
        false
    }

    fn body_start_line(&self, _parsed: &ParsedSource, node: &tree_sitter::Node) -> Option<usize> {
        // The body block starts right after the signature line — exact even
        // with trailing comments or docstring content that looks like `x:`.
        let def = if node.kind() == "decorated_definition" {
            node.child_by_field_name("definition")?
        } else {
            *node
        };
        let body = def.child_by_field_name("body")?;
        let body_row = body.start_position().row;
        // Does the body share a line with the signature's tail (`): T: ...`
        // stubs, where folding after the body line would cut the signature)?
        let shares_line = def
            .child_by_field_name("parameters")
            .is_some_and(|p| p.end_position().row == body_row)
            || def
                .child_by_field_name("return_type")
                .is_some_and(|r| r.end_position().row == body_row);
        if shares_line && body_row == body.end_position().row {
            // Stub: the whole body (`...`) sits on the signature line. Fold
            // after it; when that is the symbol's last line the caller's
            // clamp turns this into a passthrough.
            return Some(body_row + 2);
        }
        Some(body_row + 1)
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
        if let Some(prev) = effective.prev_sibling()
            && prev.kind() == "expression_statement"
        {
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
            if !is_module_doc
                && let Some(first) = prev.named_child(0)
                && first.kind() == "string"
                && let Some(doc) = string_literal_text(first, &parsed.source)
            {
                return Some(doc);
            }
        }
        // `# comment` siblings work through the generic scan.
        doc_comment_above(parsed, node)
    }

    fn import_node_types(&self) -> &[&'static str] {
        &["import_statement", "import_from_statement"]
    }

    fn import_targets(
        &self,
        node: &tree_sitter::Node,
        source: &str,
    ) -> Vec<crate::imports::ImportTarget> {
        match node.kind() {
            "import_statement" => {
                // `import a.b.c` / `import numpy as np` / `import os, sys` —
                // one target per name on the same line.
                let mut out = Vec::new();
                let mut child = node.walk();
                for kid in node.children(&mut child) {
                    let name = match kid.kind() {
                        "dotted_name" => Some(kid),
                        "aliased_import" => kid.named_child(0),
                        _ => None,
                    };
                    if let Some(name) = name
                        && let Ok(target) = name.utf8_text(source.as_bytes())
                    {
                        out.push(crate::imports::ImportTarget {
                            target: target.trim().to_string(),
                            relative: false,
                        });
                    }
                }
                out
            }
            "import_from_statement" => {
                // `from a.b import c` / `from . import x` — the module path.
                let Some(module) = node.child_by_field_name("module_name") else {
                    return Vec::new();
                };
                let Some(target) = module.utf8_text(source.as_bytes()).ok() else {
                    return Vec::new();
                };
                let target = target.trim().to_string();
                vec![crate::imports::ImportTarget {
                    relative: target.starts_with('.'),
                    target,
                }]
            }
            _ => Vec::new(),
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
