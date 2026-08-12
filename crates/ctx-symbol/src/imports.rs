//! Import extraction for the symbol engine.
//!
//! Pure AST walk: each language backend declares which node types are import
//! nodes and how to derive the import target from them. No filesystem access
//! here — resolution (local/external/ignored) happens in the CLI layer so the
//! engine stays free of I/O side effects beyond reading the source file.

use crate::language::ParsedSource;
use std::ops::Range;

/// A single import extracted from the AST.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    /// Import target as written (e.g. `serde::Deserialize`, `./helpers`,
    /// `os`). Not normalized or resolved.
    pub target: String,
    /// True for imports the backend classifies as relative/in-crate
    /// (`./x`, `../x`, leading-dot Python, `crate::`/`super::`/`self::`,
    /// `mod x;`).
    pub relative: bool,
    /// 1-based line of the import statement.
    pub line: usize,
    /// Byte range of the import node in the original source.
    pub byte_range: Range<usize>,
}

/// Target extraction result from a language backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportTarget {
    pub target: String,
    pub relative: bool,
}

/// Walk the tree and collect all imports in source order.
pub fn extract_imports(parsed: &ParsedSource) -> Vec<Import> {
    let mut out = Vec::new();
    collect_imports(parsed, parsed.tree.root_node(), &mut out);
    out
}

fn collect_imports(parsed: &ParsedSource, node: tree_sitter::Node, out: &mut Vec<Import>) {
    if parsed.language.import_node_types().contains(&node.kind()) {
        let line = node.start_position().row + 1;
        let byte_range = node.byte_range();
        for target in parsed.language.import_targets(&node, &parsed.source) {
            out.push(Import {
                target: target.target,
                relative: target.relative,
                line,
                byte_range: byte_range.clone(),
            });
        }
    }
    if node.child_count() > 0 {
        let mut child = node.walk();
        for kid in node.children(&mut child) {
            collect_imports(parsed, kid, out);
        }
    }
}
