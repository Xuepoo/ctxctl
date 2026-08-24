//! Language-agnostic symbol extraction framework.
//!
//! The engine core is language-independent. Each language backend implements
//! [`Language`] and declares how to map tree-sitter node types to [`SymbolKind`]
//! and extract names/signatures. Adding a language = adding one backend module.

use crate::symbol::{Symbol, SymbolKind};
use std::path::Path;

/// Errors returned by the symbol engine.
#[derive(Debug, thiserror::Error)]
pub enum SymbolError {
    #[error("unsupported language for path: {0}")]
    UnsupportedLanguage(String),
    #[error("failed to parse source: {0}")]
    Parse(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("symbol not found: {0}")]
    NotFound(String),
}

/// A tree-sitter grammar paired with a source text, ready for extraction.
pub struct ParsedSource {
    pub tree: tree_sitter::Tree,
    pub source: String,
    pub language: &'static dyn Language,
}

/// Interface every language backend must implement.
pub trait Language: Send + Sync {
    /// Human-readable language name (e.g. "rust").
    fn name(&self) -> &'static str;

    /// The tree-sitter grammar for this language.
    fn grammar(&self) -> tree_sitter::Language;

    /// The tree-sitter grammar for a specific path. Defaults to
    /// [`Self::grammar`]; backends with per-extension grammars (e.g.
    /// TypeScript's TSX grammar for `.tsx`) override this.
    fn grammar_for_path(&self, _path: &Path) -> tree_sitter::Language {
        self.grammar()
    }

    /// Return true if this backend handles the given file path.
    fn supports_path(&self, path: &Path) -> bool;

    /// Node types that represent definition-like symbols, mapped to kinds.
    /// Node types not listed here are ignored during extraction.
    fn definition_node_types(&self) -> &[(&'static str, SymbolKind)];

    /// Given a definition node, produce its symbol name. Falls back to
    /// scanning child nodes for an identifier.
    fn symbol_name(&self, node: &tree_sitter::Node, source: &str) -> Option<String>;

    /// Produce the "signature" line(s) for a definition node — usually the
    /// first line or a compact header.
    ///
    /// Default: the node's first source line, trimmed (`…` when empty).
    /// Backends whose signature is not the first line (e.g. CSS, where the
    /// selector list alone is the header) override this.
    fn signature(&self, node: &tree_sitter::Node, source: &str) -> String {
        let text = source
            .get(node.start_byte()..node.end_byte().min(source.len()))
            .unwrap_or("…");
        let line = text.split('\n').next().unwrap_or("").trim();
        if line.is_empty() {
            "…".to_string()
        } else {
            line.to_string()
        }
    }

    /// Return true if `node` may carry a doc comment immediately above it.
    /// Backends override to handle comment idioms (e.g. `//`, `///`, `/** */`).
    fn has_doc_comment(&self, _node: &tree_sitter::Node) -> bool {
        false
    }

    /// Byte range of a definition node; defaults to the node itself.
    /// Backends extend it to include attached syntax (e.g. python decorators).
    fn definition_byte_range(&self, node: &tree_sitter::Node) -> std::ops::Range<usize> {
        node.byte_range()
    }

    /// Line-comment prefix used by compact views for fold markers.
    fn comment_prefix(&self) -> &'static str {
        "//"
    }

    /// Closing delimiter for block comments ([`Self::comment_prefix`] =
    /// `/*`); empty for line-comment languages. Appended to fold markers so
    /// compact views of CSS-like languages stay re-parseable.
    fn comment_close(&self) -> &'static str {
        ""
    }

    /// True if `}`/`)`/`]` lines may be kept as block closers in compact
    /// views. False for languages without brace/paren block syntax (python:
    /// indentation; ruby: `end`).
    fn keeps_brace_closers(&self) -> bool {
        true
    }

    /// True if the language has a C-style preprocessor whose directives
    /// start with `#` and splice continuation lines via `\`. Gates the
    /// preprocessor macro fold branch in compact views: `#`-led lines in
    /// other languages (markdown headings, script comments) are not
    /// directives and must never fold as if they were.
    fn has_preprocessor(&self) -> bool {
        false
    }

    /// Kinds of a definition's foldable body node (e.g. `block`,
    /// `compound_statement`, `field_declaration_list`). Used by the generic
    /// AST-anchored fold locator; empty means "no body nodes" and the fold
    /// falls back to the line heuristic.
    fn body_node_kinds(&self) -> &[&'static str] {
        &[]
    }

    /// 1-based source line where the definition's body begins (the first
    /// line after the signature), when the backend can anchor it in the AST.
    /// Backends with indentation-based blocks (python) must provide this —
    /// line heuristics cannot tell `def f():  # comment` (a signature with a
    /// trailing comment, which does not end in `:`) from docstring prose
    /// like `Args:`.
    fn body_start_line(&self, _parsed: &ParsedSource, _node: &tree_sitter::Node) -> Option<usize> {
        None
    }

    /// True if a line opens a block, so the compact view folds after it.
    ///
    /// Default: lines ending with `{` plus lines starting with `{` (braces on
    /// their own line, e.g. C/C++ one-line bodies). `:` is NOT an opener here
    /// — it signals indentation blocks (python), which override this. Ruby
    /// overrides with its keyword openers (`def`, `class`, `if`, …).
    fn is_opener_line(&self, line: &str) -> bool {
        let t = line.trim();
        t.ends_with('{') || t.starts_with('{')
    }

    /// Doc comment immediately above the definition node, if any.
    ///
    /// Default: a comment-kind sibling scan. Backends with richer comment
    /// idioms (e.g. Python docstrings) override this.
    fn doc_comment(&self, parsed: &ParsedSource, node: &tree_sitter::Node) -> Option<String> {
        doc_comment_above(parsed, node)
    }

    /// Node kinds that represent imports. Nodes of these kinds are visited by
    /// [`crate::imports::extract_imports`] and passed to [`Self::import_targets`].
    fn import_node_types(&self) -> &[&'static str] {
        &[]
    }

    /// Derive the import targets from an import node (see
    /// [`Self::import_node_types`]). A single statement may carry several
    /// targets (e.g. python `import os, sys`). Return an empty vec for nodes
    /// that are not actually imports.
    fn import_targets(
        &self,
        _node: &tree_sitter::Node,
        _source: &str,
    ) -> Vec<crate::imports::ImportTarget> {
        Vec::new()
    }
}

/// Parse source text with the grammar for the given path.
pub fn parse(path: &Path, source: &str) -> Result<ParsedSource, SymbolError> {
    let lang = detect_language(path)
        .ok_or_else(|| SymbolError::UnsupportedLanguage(path.display().to_string()))?;
    let (lang, tree) = parse_ambiguous(path, source, lang)?;
    Ok(ParsedSource {
        tree,
        source: source.to_string(),
        language: lang,
    })
}

/// Parse with one grammar.
fn parse_tree(
    grammar: tree_sitter::Language,
    source: &str,
) -> Result<tree_sitter::Tree, SymbolError> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&grammar)
        .map_err(|e| SymbolError::Parse(e.to_string()))?;
    parser
        .parse(source, None)
        .ok_or_else(|| SymbolError::Parse("tree-sitter returned no tree".into()))
}

/// Pre-order traversal of `root` without native-stack recursion: pending
/// nodes live on an explicit heap stack, so arbitrarily deep nesting cannot
/// overflow the stack (CTX-0030). The visitor returns `false` to skip a
/// node's subtree; children are visited left-to-right exactly like a
/// recursive walk, keeping all outputs byte-identical.
pub(crate) fn walk_preorder(
    root: tree_sitter::Node,
    mut visit: impl FnMut(tree_sitter::Node) -> bool,
) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if !visit(node) {
            continue;
        }
        for i in (0..node.child_count()).rev() {
            if let Some(kid) = node.child(i as u32) {
                stack.push(kid);
            }
        }
    }
}

/// Count ERROR/MISSING nodes in a tree.
pub(crate) fn count_error_nodes(tree: &tree_sitter::Tree) -> usize {
    let mut count = 0;
    walk_preorder(tree.root_node(), |node| {
        if node.is_error() || node.is_missing() {
            count += 1;
            // error nodes may wrap children; they are already counted once
            return false;
        }
        true
    });
    count
}

/// Error profile of a tree: total bytes covered by ERROR nodes plus the
/// ERROR/MISSING node count. A single runaway error node can swallow a whole
/// file; the byte span weighs that much more heavily than one-off token
/// errors (e.g. an annotation macro).
fn error_metrics(tree: &tree_sitter::Tree) -> (usize, usize) {
    let (mut bytes, mut count) = (0, 0);
    walk_preorder(tree.root_node(), |node| {
        if node.is_error() {
            bytes += node.end_byte() - node.start_byte();
            count += 1;
            return false;
        }
        if node.is_missing() {
            count += 1;
            return false;
        }
        true
    });
    (bytes, count)
}

/// Length-preserving mask over C/C++ sources: blanks annotation macros that
/// tree-sitter's grammars cannot parse, so the rest of the file parses
/// cleanly. Only non-newline bytes become spaces, so every byte offset stays
/// valid against the original source.
///
/// - SAL-style annotation tokens (`_In_`, `_out_`, `_ret_`, `_opt_` variants)
///   are reserved identifiers in C — never real user code.
/// - An ALL-CAPS token at line start followed by two more words is a
///   macro-expanded declaration specifier (`SECUREC_INLINE void f()`); three
///   leading identifiers cannot appear in valid C/C++ otherwise. The
///   two-token form (`UINT32 x;`) is left alone: it is a typedef declaration.
fn mask_annotations(source: &str) -> String {
    use std::sync::OnceLock;
    static SAL: OnceLock<regex::Regex> = OnceLock::new();
    static SPEC: OnceLock<regex::Regex> = OnceLock::new();
    static DECLMACRO: OnceLock<regex::Regex> = OnceLock::new();
    let sal = SAL.get_or_init(|| {
        regex::Regex::new(r#"\b_(?:in|out|inout|ret)(?:_opt)?_\b"#).expect("valid sal regex")
    });
    let spec = SPEC.get_or_init(|| {
        regex::Regex::new(
            r#"(?m)^([ \t]*)([A-Z][A-Z0-9_]*)([ \t]+[A-Za-z_][A-Za-z0-9_]*)([ \t]+[A-Za-z_*&])"#,
        )
        .expect("valid specifier regex")
    });
    // A bare macro invocation without a trailing `;` alone on a line
    // (`__decl_clone(E)`, `MY_DECL(x)`) — expands to a declaration, but
    // tree-sitter sees a call expression missing its `;`. After a few of
    // these its error recovery desyncs and swallows the rest of the file.
    // Macro-style names only (`__`-prefixed or containing an uppercase
    // letter) so `if (x)` bodies like `foo(y)` are left untouched.
    let declmacro = DECLMACRO.get_or_init(|| {
        regex::Regex::new(
            r#"(?m)^[ \t]*(__[A-Za-z0-9_]*|[A-Za-z_]*[A-Z][A-Za-z0-9_]*)[ \t]*\([^;\n]*\)[ \t]*$"#,
        )
        .expect("valid decl-macro regex")
    });
    let mut out: Vec<u8> = source.as_bytes().to_vec();

    fn blank(bytes: &mut [u8], range: std::ops::Range<usize>) {
        for b in &mut bytes[range] {
            if *b != b'\n' && *b != b'\r' {
                *b = b' ';
            }
        }
    }

    for m in sal.find_iter(source) {
        // `#define _out_` defines the annotation — masking inside the
        // directive would leave a nameless `#define` and a fresh error.
        let line_start = source[..m.start()].rfind('\n').map_or(0, |i| i + 1);
        let line_prefix = source[line_start..m.start()].trim_start();
        if line_prefix.starts_with('#') {
            continue;
        }
        blank(&mut out, m.range());
    }
    for m in declmacro.find_iter(source) {
        // A constructor definition whose member-init list or body starts on
        // the next line (`Ctor(args)\n    : _w(w) {}`) is not a macro — its
        // invocation line must stay.
        let next = source[m.end()..]
            .lines()
            .map(str::trim_start)
            .find(|l| !l.is_empty())
            .unwrap_or("");
        if next.starts_with(':') || next.starts_with('{') {
            continue;
        }
        blank(&mut out, m.range());
    }

    // Specifier macros can chain (`SECUREC_INLINE SECUREC_UNUSED void f()`):
    // blank one per line per pass and repeat until stable.
    loop {
        let text = String::from_utf8(out.clone()).expect("mask keeps valid UTF-8");
        let mut changed = false;
        for m in spec.captures_iter(&text) {
            let Some(g) = m.get(2) else { continue };
            blank(&mut out, g.range());
            changed = true;
        }
        if !changed {
            break;
        }
    }

    String::from_utf8(out).expect("masking keeps valid UTF-8 (only blanks bytes)")
}

/// Pick the best parse for C/C++ sources:
///
/// 1. A `.h` carrying C++-only markers (`::`, `template`, …) is parsed as C++
///    outright — the C grammar "succeeds" at parsing C++ into structurally
///    wrong symbols (namespaces/templates/classes become function nodes) with
///    deceptively few error nodes, so error metrics are untrustworthy there.
/// 2. Otherwise `.h` is C — but a C++ parse is still tried when the C parse
///    has errors (a header that slipped past the sniff), keeping the tree
///    with the better error profile (fewer error bytes, then fewer error
///    nodes; ties prefer C).
/// 3. When the best parse still has errors, retry with annotation macros
///    masked ([`mask_annotations`]) — the mask is length-preserving, so byte
///    ranges stay valid against the original source.
///
/// A pure function of the source, so the choice is deterministic and
/// byte-stable.
fn parse_ambiguous(
    path: &Path,
    source: &str,
    lang: &'static dyn Language,
) -> Result<(&'static dyn Language, tree_sitter::Tree), SymbolError> {
    let is_h = lang.name() == "c" && path.extension().and_then(|e| e.to_str()) == Some("h");
    let cpp: Option<&'static dyn Language> = is_h
        .then(|| REGISTRY.iter().find(|l| l.name() == "cpp").copied())
        .flatten();
    let force_cpp = cpp.is_some() && looks_like_cpp(source);
    let primary: &'static dyn Language = if force_cpp { cpp.unwrap() } else { lang };

    let direct = parse_tree(primary.grammar_for_path(path), source)?;
    let mut best = (primary, direct);
    let mut best_metrics = error_metrics(&best.1);

    if !force_cpp
        && best_metrics != (0, 0)
        && let Some(cpp) = cpp
    {
        // Skip the C++ parse when C is already clean: cpp cannot beat
        // (0,0), and ties already prefer C. Halves .h work.
        let tree = parse_tree(cpp.grammar_for_path(path), source)?;
        let metrics = error_metrics(&tree);
        if metrics < best_metrics {
            best = (cpp, tree);
            best_metrics = metrics;
        }
    }

    if best_metrics != (0, 0) {
        let masked = mask_annotations(source);
        let mut candidates: Vec<&'static dyn Language> = vec![primary];
        if !force_cpp && let Some(cpp) = cpp {
            candidates.push(cpp);
        }
        for candidate in candidates {
            let tree = parse_tree(candidate.grammar_for_path(path), &masked)?;
            let metrics = error_metrics(&tree);
            if metrics < best_metrics {
                best = (candidate, tree);
                best_metrics = metrics;
            }
        }
    }

    Ok(best)
}

/// Heuristic: does this C/C++ source carry C++-only constructs? Used to route
/// `.h` files (which may be C or C++) to the right grammar. Only markers that
/// are essentially impossible in valid C are included (a C header mentioning
/// `::` or `template` is already C++ in spirit).
fn looks_like_cpp(source: &str) -> bool {
    use std::sync::OnceLock;
    static MARKERS: OnceLock<regex::Regex> = OnceLock::new();
    let re = MARKERS.get_or_init(|| {
        regex::Regex::new(
            r#"::|\b(?:template|namespace|typename|constexpr|nullptr|override|decltype|static_cast|dynamic_cast|const_cast|reinterpret_cast|noexcept|virtual|explicit|friend)\b|enum[ \t]+class"#,
        )
        .expect("valid cpp marker regex")
    });
    re.is_match(source)
}

/// Detect the language backend for a path, if any.
pub fn detect_language(path: &Path) -> Option<&'static dyn Language> {
    REGISTRY.iter().find(|l| l.supports_path(path)).copied()
}

/// All registered language backends. Add new languages here.
pub static REGISTRY: &[&'static dyn Language] = &[
    &crate::lang::rust::RustLang,
    &crate::lang::typescript::TypeScriptLang,
    &crate::lang::python::PythonLang,
    &crate::lang::go::GoLang,
    &crate::lang::javascript::JavaScriptLang,
    &crate::lang::java::JavaLang,
    &crate::lang::c::CLang,
    &crate::lang::cpp::CppLang,
    &crate::lang::csharp::CSharpLang,
    &crate::lang::ruby::RubyLang,
    &crate::lang::lua::LuaLang,
    &crate::lang::html::HtmlLang,
    &crate::lang::css::CssLang,
    &crate::lang::markdown::MarkdownLang,
];

/// True if the node kind is a definition type for the given language.
pub fn is_definition(lang: &dyn Language, kind: &str) -> Option<SymbolKind> {
    lang.definition_node_types()
        .iter()
        .find(|(t, _)| *t == kind)
        .map(|(_, k)| *k)
}

/// Normalize a raw signature for display. Deterministic, applied uniformly
/// across all backends so outline rows stay compact:
///
/// - skip leading attribute / decorator-only lines (`#[...]`, `@decorator`)
/// - drop a trailing comment (started by `//` or `/*` after code)
/// - collapse internal whitespace runs
/// - strip trailing continuation delimiters (`(`, `{`, `,`, `;`, `:`, `=`,
///   `->`, `=>`) left by declarations that span multiple lines
/// - cap at [`MAX_SIGNATURE`] chars with an ellipsis
pub(crate) fn clean_signature(raw: &str) -> String {
    const MAX_SIGNATURE: usize = 120;

    let line = raw
        .lines()
        .map(str::trim)
        .find(|t| !(t.starts_with("#[") || is_annotation_only(t)))
        .unwrap_or("");
    let line = strip_trailing_comment(line);

    let mut sig = line.split_whitespace().collect::<Vec<_>>().join(" ");
    loop {
        let mut changed = false;
        for suffix in ["=>", "->"] {
            if let Some(t) = sig.strip_suffix(suffix) {
                sig = t.trim_end().to_string();
                changed = true;
            }
        }
        if sig.ends_with(['(', '{', ',', ';', ':', '=']) {
            sig = sig
                .trim_end_matches(['(', '{', ',', ';', ':', '='])
                .trim_end()
                .to_string();
            changed = true;
        }
        if !changed {
            break;
        }
    }

    if sig.chars().count() > MAX_SIGNATURE {
        sig = sig.chars().take(MAX_SIGNATURE - 1).collect();
        sig.push('…');
    }
    if sig.is_empty() {
        "…".to_string()
    } else {
        sig
    }
}

/// A lone `@decorator` / `@decorator(args)` line with nothing else on it.
fn is_annotation_only(t: &str) -> bool {
    let Some(rest) = t.strip_prefix('@') else {
        return false;
    };
    if rest.is_empty() {
        return true;
    }
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '.';
    if let Some(after) = rest.find('(') {
        rest[..after].chars().all(ident) && rest.ends_with(')')
    } else {
        !rest.contains(char::is_whitespace) && rest.chars().all(ident)
    }
}

/// Cut a `//` or `/*` comment when it starts after code (preceded by
/// whitespace), leaving URL strings untouched.
fn strip_trailing_comment(line: &str) -> &str {
    let cut_at = |marker: &str| {
        line.match_indices(marker).find_map(|(i, _)| {
            let preceded_by_ws = line[..i].ends_with(char::is_whitespace);
            (i == 0 || preceded_by_ws).then_some(i)
        })
    };
    let cut = cut_at("//").into_iter().chain(cut_at("/*")).min();
    match cut {
        Some(i) => line[..i].trim_end(),
        None => line,
    }
}

/// Walk a tree and collect all definition symbols in source order.
pub fn extract_symbols(parsed: &ParsedSource) -> Vec<Symbol> {
    let mut out = Vec::new();
    collect_definitions(parsed, &mut out);
    out
}

fn collect_definitions(parsed: &ParsedSource, out: &mut Vec<Symbol>) {
    walk_preorder(parsed.tree.root_node(), |node| {
        let kind = node.kind();
        if let Some(sym_kind) = is_definition(parsed.language, kind)
            && let Some(name) = parsed.language.symbol_name(&node, &parsed.source)
        {
            let range = parsed.language.definition_byte_range(&node);
            let sig = clean_signature(&parsed.language.signature(&node, &parsed.source));
            // Displayed lines follow the *slice*: when a backend extends the
            // range past the node (markdown sections, attached decorators), the
            // outline must advertise what a slice will actually deliver.
            let node_range = node.byte_range();
            let (start_line, end_line) = if range == node_range {
                (node.start_position().row + 1, node.end_position().row + 1)
            } else {
                let src = &parsed.source;
                let before = src[..range.start].matches('\n').count();
                let inside = src[range.clone()].matches('\n').count();
                (
                    before + 1,
                    if src[range.clone()].ends_with('\n') {
                        before + inside
                    } else {
                        before + inside + 1
                    },
                )
            };
            let doc = parsed.language.doc_comment(parsed, &node);
            out.push(Symbol {
                name,
                kind: sym_kind,
                start_line,
                end_line,
                byte_range: range,
                signature: sig,
                doc_comment: doc,
            });
        }
        true
    });
}

/// Look one level up in the tree for a doc-comment sibling immediately before
/// the definition node. "Immediately" is strict: the previous sibling must be
/// a comment and no blank line may sit between them — a comment separated
/// from code by a blank line or by another definition documents nothing.
/// Best-effort; backends with richer comment handling can override via their
/// own [`Language::doc_comment`].
pub(crate) fn doc_comment_above(parsed: &ParsedSource, node: &tree_sitter::Node) -> Option<String> {
    if !parsed.language.has_doc_comment(node) {
        return None;
    }
    let cmt = node.prev_sibling()?;
    if !cmt.kind().contains("comment") {
        return None;
    }
    let raw = cmt.utf8_text(parsed.source.as_bytes()).ok()?;
    // Some grammars fold the comment's trailing newline into the comment
    // node; cut it so adjacency is judged on real separating lines only.
    let text_end = cmt.start_byte() + raw.trim_end_matches(['\n', '\r']).len();
    let gap = &parsed.source[text_end..node.start_byte()];
    if has_blank_line(gap) {
        return None;
    }
    let text = strip_comment_markers(raw);
    if text.is_empty() {
        return None;
    }
    Some(text)
}

/// True when `gap` contains a blank line: a newline followed only by
/// whitespace before the next newline (or end of gap).
fn has_blank_line(gap: &str) -> bool {
    let mut after_newline = false;
    for ch in gap.chars() {
        match ch {
            '\n' => {
                if after_newline {
                    return true;
                }
                after_newline = true;
            }
            ' ' | '\t' | '\r' => {}
            _ => after_newline = false,
        }
    }
    false
}

/// Strip comment markers from a doc-comment node's text: `///`/`//!`/`//`,
/// `/** */`/`/*! */`, or `#`, returning the plain prose.
fn strip_comment_markers(text: &str) -> String {
    let t = text.trim();
    let inner = if t.starts_with("/*") {
        t.strip_prefix("/*")
            .unwrap_or(t)
            .strip_suffix("*/")
            .unwrap_or(t)
    } else if let Some(rest) = t.strip_prefix("--") {
        rest
    } else if let Some(rest) = t.strip_prefix("//") {
        rest
    } else if let Some(rest) = t.strip_prefix('#') {
        rest
    } else {
        t
    };
    inner
        .lines()
        .map(|l| {
            l.trim()
                .trim_start_matches(['*', '/', '!'])
                .trim()
                .to_string()
        })
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::clean_signature;

    #[test]
    fn strips_trailing_continuation_delimiters() {
        assert_eq!(clean_signature("pub fn run_exec("), "pub fn run_exec");
        assert_eq!(clean_signature("struct Cli {"), "struct Cli");
        assert_eq!(
            clean_signature("fn main() -> ExitCode {"),
            "fn main() -> ExitCode"
        );
        assert_eq!(clean_signature("mod config;"), "mod config");
        assert_eq!(clean_signature("def validate(cfg):"), "def validate(cfg)");
        assert_eq!(clean_signature("const f = (x) =>"), "const f = (x)");
        assert_eq!(
            clean_signature("pub fn add(a: i32, b: i32) -> i32 {"),
            "pub fn add(a: i32, b: i32) -> i32"
        );
    }

    #[test]
    fn skips_attribute_and_decorator_lines() {
        assert_eq!(
            clean_signature("#[derive(Debug)]\npub struct Config {"),
            "pub struct Config"
        );
        assert_eq!(clean_signature("@dataclass\nclass Point:"), "class Point");
    }

    #[test]
    fn drops_trailing_comments_and_collapses_whitespace() {
        assert_eq!(
            clean_signature("pub  fn   foo() { // does things"),
            "pub fn foo()"
        );
        assert_eq!(
            clean_signature("const URL = \"https://example.com\";"),
            "const URL = \"https://example.com\""
        );
    }

    #[test]
    fn caps_long_signatures() {
        let sig = clean_signature(&format!("fn very_long_name{}(", "x".repeat(300)));
        assert!(sig.ends_with('…'));
        assert!(sig.chars().count() <= 120);
    }

    #[test]
    fn cjk_signatures_cap_by_chars_not_bytes() {
        // 100 CJK chars are 300 bytes: a byte-based cap would truncate this
        // to ~40 chars; the cap counts characters, so it is kept whole.
        let sig = clean_signature(&"汉".repeat(100));
        assert_eq!(sig.chars().count(), 100);
        assert!(!sig.contains('…'));
    }

    #[test]
    fn truncation_still_caps_cjk_at_max_chars() {
        let sig = clean_signature(&format!("fn f() {{ {} }}", "汉".repeat(200)));
        assert_eq!(sig.chars().count(), 120);
        assert!(sig.ends_with('…'));
    }
}
