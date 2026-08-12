//! C++ language backend. Extends the C backend with classes, namespaces,
//! templates, and `using` declarations.

use crate::language::{Language, ParsedSource};
use crate::symbol::SymbolKind;
use std::path::Path;

pub struct CppLang;

impl Language for CppLang {
    fn name(&self) -> &'static str {
        "cpp"
    }

    fn grammar(&self) -> tree_sitter::Language {
        tree_sitter_cpp::LANGUAGE.into()
    }

    fn supports_path(&self, path: &Path) -> bool {
        matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" | "h++")
        )
    }

    fn definition_node_types(&self) -> &[(&'static str, SymbolKind)] {
        &[
            ("function_definition", SymbolKind::Function),
            ("class_specifier", SymbolKind::Class),
            ("struct_specifier", SymbolKind::Struct),
            ("union_specifier", SymbolKind::Struct),
            ("enum_specifier", SymbolKind::Enum),
            ("type_definition", SymbolKind::Type),
            ("field_declaration", SymbolKind::Variable),
            ("declaration", SymbolKind::Variable),
            ("namespace_definition", SymbolKind::Type),
            ("namespace_alias_definition", SymbolKind::Type),
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
        // Function prototypes have no body to slice; skip them.
        if super::c::is_function_prototype(*node) {
            return None;
        }
        let name_node = match node.child_by_field_name("name") {
            Some(n) => n,
            None => {
                // function_definition/type_definition/field_declaration/
                // declaration carry the name down the declarator chain.
                let declarator = node.child_by_field_name("declarator")?;
                super::c::declarator_name(declarator, 0)?
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

    fn definition_byte_range(&self, node: &tree_sitter::Node) -> std::ops::Range<usize> {
        // A template header (`template <typename T>`) wraps the definition;
        // include it so slices and compact views carry the template params.
        if node
            .parent()
            .is_some_and(|p| p.kind() == "template_declaration")
        {
            return node.parent().unwrap().byte_range();
        }
        node.byte_range()
    }

    fn doc_comment(&self, parsed: &ParsedSource, node: &tree_sitter::Node) -> Option<String> {
        // Doc comments sit above the template_declaration wrapper.
        let effective = if node
            .parent()
            .is_some_and(|p| p.kind() == "template_declaration")
        {
            node.parent().unwrap()
        } else {
            *node
        };
        crate::language::doc_comment_above(parsed, &effective)
    }

    fn import_node_types(&self) -> &[&'static str] {
        &["preproc_include", "using_declaration"]
    }

    fn import_targets(
        &self,
        node: &tree_sitter::Node,
        source: &str,
    ) -> Vec<crate::imports::ImportTarget> {
        match node.kind() {
            "preproc_include" => (|| {
                let path = node.child_by_field_name("path")?;
                let kind = path.kind();
                let text = path.utf8_text(source.as_bytes()).ok()?;
                let unquoted = text.trim().trim_matches(['"', '<', '>']);
                let relative = kind == "string_literal";
                Some(vec![crate::imports::ImportTarget {
                    target: unquoted.to_string(),
                    relative,
                }])
            })()
            .unwrap_or_default(),
            "using_declaration" => {
                // `using namespace std;` -> std; `using std::vector;` ->
                // std::vector. Type aliases (`using Foo = Bar;`) are not
                // imports.
                (|| {
                    let text = node.utf8_text(source.as_bytes()).ok()?.trim();
                    if text.contains('=') {
                        return None;
                    }
                    let after_using = text.strip_prefix("using")?.trim();
                    let target = after_using
                        .strip_prefix("namespace")
                        .unwrap_or(after_using)
                        .trim()
                        .trim_end_matches(';')
                        .trim()
                        .to_string();
                    Some(vec![crate::imports::ImportTarget {
                        target,
                        relative: false,
                    }])
                })()
                .unwrap_or_default()
            }
            _ => Vec::new(),
        }
    }
}
