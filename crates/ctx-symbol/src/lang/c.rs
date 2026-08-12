//! C language backend.

use crate::language::Language;
use crate::symbol::SymbolKind;
use std::path::Path;

pub struct CLang;

impl Language for CLang {
    fn name(&self) -> &'static str {
        "c"
    }

    fn grammar(&self) -> tree_sitter::Language {
        tree_sitter_c::LANGUAGE.into()
    }

    fn supports_path(&self, path: &Path) -> bool {
        matches!(path.extension().and_then(|e| e.to_str()), Some("c" | "h"))
    }

    fn definition_node_types(&self) -> &[(&'static str, SymbolKind)] {
        &[
            ("function_definition", SymbolKind::Function),
            ("struct_specifier", SymbolKind::Struct),
            ("union_specifier", SymbolKind::Struct),
            ("enum_specifier", SymbolKind::Enum),
            ("type_definition", SymbolKind::Type),
            ("field_declaration", SymbolKind::Variable),
            ("preproc_def", SymbolKind::Const),
            ("preproc_function_def", SymbolKind::Const),
        ]
    }

    fn symbol_name(&self, node: &tree_sitter::Node, source: &str) -> Option<String> {
        // A struct_specifier inside a typedef is captured by the enclosing
        // type_definition; skip it to avoid duplicate symbols.
        if node.kind() == "struct_specifier"
            && node.parent().is_some_and(|p| p.kind() == "type_definition")
        {
            return None;
        }
        let name_node = match node.child_by_field_name("name") {
            Some(n) => n,
            None => {
                // function_definition/type_definition/field_declaration carry
                // the name down the declarator chain: declarator ->
                // function_declarator -> (pointer_declarator)* -> identifier.
                let declarator = node.child_by_field_name("declarator")?;
                declarator_name(declarator, 0)?
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
        &["preproc_include"]
    }

    fn import_targets(
        &self,
        node: &tree_sitter::Node,
        source: &str,
    ) -> Vec<crate::imports::ImportTarget> {
        (|| {
            let path = node.child_by_field_name("path")?;
            let kind = path.kind();
            let text = path.utf8_text(source.as_bytes()).ok()?;
            let unquoted = text.trim().trim_matches(['"', '<', '>']);
            // Quoted includes are relative to the file; angle includes are
            // system headers.
            let relative = kind == "string_literal";
            Some(vec![crate::imports::ImportTarget {
                target: unquoted.to_string(),
                relative,
            }])
        })()
        .unwrap_or_default()
    }
}

/// Descend the declarator chain (`pointer_declarator*` -> `identifier`) to
/// find the declared name node. `depth` guards against pathological nesting.
pub(crate) fn declarator_name(node: tree_sitter::Node, depth: usize) -> Option<tree_sitter::Node> {
    if depth > 8 {
        return None;
    }
    match node.kind() {
        "identifier" | "field_identifier" | "type_identifier" => Some(node),
        _ => {
            let inner = node.child_by_field_name("declarator")?;
            declarator_name(inner, depth + 1)
        }
    }
}
