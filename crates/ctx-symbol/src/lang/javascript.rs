//! JavaScript language backend.

use crate::lang::util::string_value;
use crate::language::Language;
use crate::symbol::SymbolKind;
use std::path::Path;

pub struct JavaScriptLang;

impl Language for JavaScriptLang {
    fn name(&self) -> &'static str {
        "javascript"
    }

    fn grammar(&self) -> tree_sitter::Language {
        tree_sitter_javascript::LANGUAGE.into()
    }

    fn supports_path(&self, path: &Path) -> bool {
        matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("js" | "jsx" | "mjs" | "cjs")
        )
    }

    fn body_node_kinds(&self) -> &[&'static str] {
        &["statement_block", "class_body", "object"]
    }

    fn definition_node_types(&self) -> &[(&'static str, SymbolKind)] {
        &[
            ("function_declaration", SymbolKind::Function),
            ("class_declaration", SymbolKind::Class),
            ("method_definition", SymbolKind::Method),
            ("lexical_declaration", SymbolKind::Variable),
            ("variable_declaration", SymbolKind::Variable),
            ("generator_function_declaration", SymbolKind::Function),
        ]
    }

    fn symbol_name(&self, node: &tree_sitter::Node, source: &str) -> Option<String> {
        // `var x = ...` has no name field on the declaration node; read it
        // from the variable_declarator child.
        let name_node = match node.child_by_field_name("name") {
            Some(n) => n,
            None => node.named_child(0)?.child_by_field_name("name")?,
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
        &["import_statement", "export_statement", "call_expression"]
    }

    fn import_targets(
        &self,
        node: &tree_sitter::Node,
        source: &str,
    ) -> Vec<crate::imports::ImportTarget> {
        (|| {
            let target = match node.kind() {
                "import_statement" => {
                    // `import x from 'y'` / `import 'y'`.
                    let src = node.child_by_field_name("source")?;
                    string_value(src, source)?
                }
                "export_statement" => {
                    // Only re-exports carry a source: `export {x} from 'y'`.
                    let src = node.child_by_field_name("source")?;
                    string_value(src, source)?
                }
                "call_expression" => {
                    // `require('y')`.
                    let function = node.child_by_field_name("function")?;
                    if function.utf8_text(source.as_bytes()).ok()? != "require" {
                        return None;
                    }
                    let args = node.child_by_field_name("arguments")?;
                    let first = args.named_child(0)?;
                    string_value(first, source)?
                }
                _ => return None,
            };
            let relative = target.starts_with("./") || target.starts_with("../");
            Some(vec![crate::imports::ImportTarget { target, relative }])
        })()
        .unwrap_or_default()
    }
}
