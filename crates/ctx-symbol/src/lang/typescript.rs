//! TypeScript / JavaScript language backend.

use crate::lang::util::string_value;
use crate::language::Language;
use crate::symbol::SymbolKind;
use std::path::Path;

pub struct TypeScriptLang;

impl Language for TypeScriptLang {
    fn name(&self) -> &'static str {
        "typescript"
    }

    fn grammar(&self) -> tree_sitter::Language {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
    }

    fn grammar_for_path(&self, path: &Path) -> tree_sitter::Language {
        // `.tsx` is JSX-in-TypeScript and needs the TSX grammar; the plain
        // typescript grammar errors on JSX. `.mts`/`.cts` stay typescript.
        if path.extension().and_then(|e| e.to_str()) == Some("tsx") {
            tree_sitter_typescript::LANGUAGE_TSX.into()
        } else {
            self.grammar()
        }
    }

    fn supports_path(&self, path: &Path) -> bool {
        matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("ts" | "tsx" | "mts" | "cts")
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
            ("interface_declaration", SymbolKind::Interface),
            ("type_alias_declaration", SymbolKind::Type),
            ("enum_declaration", SymbolKind::Enum),
            ("lexical_declaration", SymbolKind::Variable),
            ("variable_declaration", SymbolKind::Variable),
            ("abstract_class_declaration", SymbolKind::Class),
        ]
    }

    fn symbol_name(&self, node: &tree_sitter::Node, source: &str) -> Option<String> {
        // The `name` grammar field is precise for function/class/interface/
        // enum/type declarations; method_definition also exposes `name`.
        // `const`/`let` lexical declarations carry the name on their
        // `variable_declarator` child instead — read it there.
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
        &[
            "import_statement",
            "import_require_clause",
            "export_statement",
            "call_expression",
        ]
    }

    fn import_targets(
        &self,
        node: &tree_sitter::Node,
        source: &str,
    ) -> Vec<crate::imports::ImportTarget> {
        let Some(target) = (|| {
            let target = match node.kind() {
                "import_statement" | "import_require_clause" => {
                    // `import x from 'y'` / `import 'y'` / `import x = require('y')`.
                    let src = node.child_by_field_name("source")?;
                    string_value(src, source)?
                }
                "export_statement" => {
                    // Only re-exports carry a source: `export {x} from 'y'`.
                    let src = node.child_by_field_name("source")?;
                    string_value(src, source)?
                }
                "call_expression" => {
                    // `require('y')`; skipped when part of `import x = require(...)`.
                    if node
                        .parent()
                        .is_some_and(|p| p.kind() == "import_require_clause")
                    {
                        return None;
                    }
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
            Some(crate::imports::ImportTarget { target, relative })
        })() else {
            return Vec::new();
        };
        vec![target]
    }
}
