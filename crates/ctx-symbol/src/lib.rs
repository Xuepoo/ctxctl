//! `ctx-symbol` — a reusable symbol engine for token-efficient code reading.
//!
//! Uses tree-sitter to locate symbols (classes, functions, methods, …) in a
//! source file, then exposes their **byte ranges** into the original text so a
//! caller can slice out exactly the part it needs — instead of reading a whole
//! file into an LLM context.
//!
//! Design:
//! - Stateless: parse on demand, no cache, no index.
//! - Language-agnostic core; each backend in `lang/` maps tree-sitter node
//!   types to [`symbol::SymbolKind`].
//! - Byte-stable output: slices come from the original source, preserving
//!   comments/formatting and enabling provider prompt caching.

pub mod imports;
pub mod lang;
pub mod language;
pub mod symbol;

use std::ops::Range;

pub use imports::{Import, ImportTarget, extract_imports};
pub use language::{Language, ParsedSource, SymbolError, extract_symbols, parse};
pub use symbol::{Symbol, SymbolKind};

/// Convenience: locate a symbol by name and return its original source slice.
pub fn slice_by_name(
    source: &str,
    path: &std::path::Path,
    name: &str,
) -> Result<String, SymbolError> {
    let parsed = parse(path, source)?;
    let symbols = extract_symbols(&parsed);
    let sym = symbols
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| SymbolError::NotFound(name.to_string()))?;
    let bytes = &source.as_bytes()[sym.byte_range.clone()];
    let text = std::str::from_utf8(bytes).map_err(|e| SymbolError::Parse(e.to_string()))?;
    Ok(text.to_string())
}

/// Convenience: return the full outline (all symbols) of a source file.
pub fn outline(source: &str, path: &std::path::Path) -> Result<Vec<Symbol>, SymbolError> {
    let parsed = parse(path, source)?;
    Ok(extract_symbols(&parsed))
}

/// Number of ERROR/MISSING nodes in the tree (0 = clean parse). Callers can
/// use this to signal partial symbol lists instead of failing silently.
pub fn parse_error_count(parsed: &ParsedSource) -> usize {
    crate::language::count_error_nodes(&parsed.tree)
}

/// Compact view of a symbol: signature (and decorators / multi-line signature
/// lines) plus a fold marker replacing the body, keeping the closing line
/// (e.g. `}`) when the body ends with one.
///
/// The result is a byte-stable function of the source and re-parses without
/// errors in the backend grammar (the marker is a line comment). Single-line
/// symbols pass through unchanged.
pub fn compact_symbol(parsed: &ParsedSource, symbol: &Symbol) -> String {
    let bytes = &parsed.source.as_bytes()[symbol.byte_range.clone()];
    let Ok(text) = std::str::from_utf8(bytes) else {
        return symbol.signature.clone();
    };
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= 1 {
        return text.to_string();
    }
    // Absolute byte offsets of each line's start in `parsed.source` (lines
    // are slices of the symbol range).
    let sym_start = symbol.byte_range.start;
    let mut line_starts = Vec::with_capacity(lines.len());
    let mut off = 0usize;
    for line in &lines {
        line_starts.push(off);
        off += line.len() + 1;
    }
    // AST context for this symbol: comment ranges (masked from the lexical
    // scans below) and, for backends that provide it, the body's start line.
    let sym_node = parsed
        .tree
        .root_node()
        .descendant_for_byte_range(symbol.byte_range.start, symbol.byte_range.end);
    let comments = sym_node
        .map(|node| {
            let mut out = Vec::new();
            collect_comment_ranges(node, &mut out);
            out.sort_by_key(|r| r.start);
            out
        })
        .unwrap_or_default();
    // AST-anchored fold: when a body node exists, fold at its boundary. A
    // body that is empty or uncloseable is passed through unchanged — it is
    // not a candidate for the line heuristic (which would mangle e.g. a
    // constructor with an empty `{ }` or a one-line class stub). The line
    // heuristic below is the fallback for body-less definitions (macros,
    // forward declarations).
    if let Some(body) = sym_node.and_then(|n| find_body_node(parsed.language, n)) {
        return match fold_at_body_node(&parsed.source, symbol, &body, parsed) {
            Some(ast) => ast,
            None => text.to_string(),
        };
    }
    // Symbol-relative index of the first body line (python). The absolute
    // body line is anchored in the AST; subtract the range's first source
    // line (which may include decorators).
    let range_start_line = parsed.source.as_bytes()[..symbol.byte_range.start]
        .iter()
        .filter(|b| **b == b'\n')
        .count()
        + 1;
    let ast_fold_at = sym_node
        .and_then(|node| parsed.language.body_start_line(parsed, &node))
        .map(|body_line| body_line.saturating_sub(range_start_line));
    // Header = the signature. Without an AST body anchor the only foldable
    // body-less definition is a preprocessor macro (handled below): a
    // variable/const declaration whose value is a literal or expression
    // (template string, JSX, array/struct literal) is data, and scanning it
    // for a `{` opener would mangle that data. Such symbols pass through.
    let mut fold_at = lines.len();
    let mut last_opener: Option<usize> = None;
    if let Some(idx) = ast_fold_at {
        // AST-anchored fold: the signature ends just above the body. Never
        // fold the first line away — a body that starts on the symbol's own
        // first line (one-line `def f(): return …` with a multi-line
        // expression) folds from line 1 and lets the boundary slide sort
        // out the continuation.
        fold_at = idx.clamp(1, lines.len());
        last_opener = Some(fold_at.saturating_sub(1));
    }
    if fold_at == lines.len() {
        // With no AST anchor and no opener, only preprocessor macros (which
        // splice their body via `\`) are foldable — anything else is a
        // forward declaration, data literal, or other body-less definition:
        // pass through unchanged.
        fold_at = match last_opener {
            Some(i) => i + 1,
            // A macro folds after its first line only when the name lives on
            // that line (`#define FOO(x) \`); `#define \` defers the name to
            // the next line and must stay intact.
            None if lines[0].trim_start().starts_with('#') => {
                let rest = lines[0].trim_start();
                let rest = rest.trim_start_matches('#').trim_start();
                let rest = rest.strip_prefix("define").unwrap_or(rest);
                if rest.trim_matches([' ', '\t', '\\']).is_empty() {
                    return text.to_string();
                }
                1
            }
            None => return text.to_string(),
        };
    }
    // A fold boundary must not split a syntactic continuation: a kept line
    // ending with an operator/separator (`,`, `+`, `(`, `=` …) — or a next
    // line starting with one (`+ __str._M_check(...)`) — still needs its
    // lines together. Slide the fold point past any such boundary. Comment
    // lines never continue; neither do lines inside a multi-line string.
    let mut in_string = vec![false; lines.len()];
    let mut state = StrDelim::None;
    for (i, line) in lines.iter().enumerate() {
        state = advance_str_state(state, line, line_starts[i] + sym_start, &comments);
        in_string[i] = state != StrDelim::None;
    }
    // Cumulative `(`/`[` balance after each line (parens inside strings are
    // skipped): a boundary with pending open parens would orphan them.
    let mut open_parens = vec![0usize; lines.len()];
    let mut balance = 0i64;
    for (i, line) in lines.iter().enumerate() {
        let in_str = i > 0 && in_string[i - 1];
        if !in_str {
            for ch in code_chars(line, line_starts[i] + sym_start, &comments) {
                match ch {
                    '(' | '[' => balance += 1,
                    ')' | ']' => balance -= 1,
                    _ => {}
                }
            }
        }
        open_parens[i] = balance.max(0) as usize;
    }
    let in_preproc = lines[0].trim_start().starts_with('#');
    while fold_at < lines.len()
        && (boundary_continues(
            lines[fold_at - 1],
            lines[fold_at],
            in_string[fold_at - 1],
            open_parens[fold_at - 1],
            parsed,
            in_preproc,
        ) || splits_comment(
            lines[fold_at - 1],
            line_starts[fold_at - 1] + sym_start,
            &comments,
        ))
    {
        fold_at += 1;
    }
    let mut rest: Vec<&str> = lines[fold_at..].to_vec();
    if rest.is_empty() {
        return text.to_string();
    }
    // The fold region must not begin with a NEW block opener at a shallower
    // indent than the header's last opener (e.g. a multi-line java
    // annotation preceding `class Foo {`): slide the fold point past the
    // opener line so the header owns it. AST-anchored folds (python) already
    // know the exact body start — docstring lines like `"""Arguments:` would
    // only confuse this heuristic.
    while ast_fold_at.is_none()
        && rest[0].trim_end().ends_with(['{', ':'])
        && leading_whitespace(rest[0]).len()
            < last_opener.map_or(0, |i| leading_whitespace(lines[i]).len())
    {
        fold_at += 1;
        rest = lines[fold_at..].to_vec();
        if rest.is_empty() {
            return text.to_string();
        }
    }
    // Preprocessor directives inside the fold region (e.g. `#ifdef` guards
    // in C function bodies) must stay balanced; folding through them would
    // orphan the `#endif`. Keep the whole symbol instead. Languages whose
    // line comments start with `#` (python, ruby) have no preprocessor —
    // their `#` lines are plain comments.
    if parsed.language.comment_prefix() != "#"
        && rest.iter().any(|l| l.trim_start().starts_with('#'))
    {
        return text.to_string();
    }
    // Keep a bare closing line so the compact view reads as a complete block.
    // Preprocessor macros are spliced by `\` continuations — the closer
    // would land outside the directive, so never keep it there. Brace/paren
    // closers only apply to brace languages (python folds by indentation,
    // ruby closes with `end`).
    let last = rest[rest.len() - 1];
    let keep_last = is_closer(last)
        && (parsed.language.keeps_brace_closers() || is_end_closer(last))
        && !lines[0].trim_start().starts_with('#')
        && closer_balances(last, &lines[..fold_at]);
    let omitted = rest.len() - usize::from(keep_last);
    if omitted == 0 {
        return text.to_string();
    }
    let indent = leading_whitespace(rest[0]);
    let mut out = String::new();
    for line in &lines[..fold_at] {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(indent);
    out.push_str(parsed.language.comment_prefix());
    out.push_str(" ... [");
    out.push_str(&omitted.to_string());
    out.push_str(" lines omitted]");
    let closer = parsed.language.comment_close();
    if !closer.is_empty() {
        out.push(' ');
        out.push_str(closer);
    }
    if keep_last {
        out.push('\n');
        out.push_str(last);
    }
    // Always end on a newline: without one, a trailing `\` continuation
    // (C/C++ preprocessor macros) splices the final line into the directive
    // at EOF and the fragment no longer re-parses.
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// The foldable body node of a definition: the `body` field of the
/// (unwrapped) definition node, else the first descendant (source order)
/// whose kind is in [`Language::body_node_kinds`]. `None` when the backend
/// has no body nodes.
fn find_body_node<'a>(
    lang: &'a dyn Language,
    sym_node: tree_sitter::Node<'a>,
) -> Option<tree_sitter::Node<'a>> {
    if lang.body_node_kinds().is_empty() {
        return None;
    }
    // Unwrap definition wrappers (`decorated_definition` -> its `definition`
    // field) so `field("body")` reads through.
    let def = if sym_node.kind().contains("decorated") {
        sym_node
            .child_by_field_name("definition")
            .unwrap_or(sym_node)
    } else {
        sym_node
    };
    if let Some(body) = def.child_by_field_name("body") {
        return Some(body);
    }
    let kinds = lang.body_node_kinds();
    // Iterative pre-order DFS (source order): a node is tested before its
    // descendants are pushed, so a direct child body wins over any deeper
    // body. This lets a typedef find the field list inside its struct
    // specifier while a class finds its own declaration list first.
    let mut stack: Vec<tree_sitter::Node> = Vec::new();
    let mut cursor = sym_node.walk();
    for child in sym_node
        .children(&mut cursor)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        stack.push(child);
    }
    while let Some(node) = stack.pop() {
        if kinds.contains(&node.kind()) {
            return Some(node);
        }
        let mut cursor = node.walk();
        for child in node
            .children(&mut cursor)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            stack.push(child);
        }
    }
    None
}

/// AST-anchored fold: the header is everything up to (and including) the
/// body's opening line; the body is replaced by a fold marker; the body's
/// closing line is kept when it is a block closer (and the backend keeps
/// brace/end closers), including any trailing declarators on the same line
/// (`} Point;`, `};`). Returns `None` when there is nothing to fold.
fn fold_at_body_node(
    source: &str,
    symbol: &Symbol,
    body: &tree_sitter::Node,
    parsed: &ParsedSource,
) -> Option<String> {
    let sym_start = symbol.byte_range.start;
    let sym_end = symbol.byte_range.end;
    let body_start = body.start_byte();
    let body_end = body.end_byte();
    if body_start <= sym_start || body_end > sym_end || body_start >= body_end {
        return None;
    }
    // A `{` body only exists in brace languages; indentation/keyword bodies
    // (python, ruby, lua) may still have a `{` as the first statement
    // (a dict/table literal).
    let brace_body = parsed.language.keeps_brace_closers()
        && (source[body_start..].starts_with('{')
            || source[body_start..body_end]
                .lines()
                .next()
                .unwrap_or("")
                .trim_start()
                .starts_with('{'));

    if brace_body {
        // Header = up to and including the `{` line; the closer is the `}`
        // line inside the body node (with any trailing declarators).
        let opener_nl = source[body_start..sym_end]
            .find('\n')
            .map(|i| body_start + i)
            .unwrap_or(sym_end);
        let header = &source[sym_start..opener_nl];
        if header.matches("/*").count() > header.matches("*/").count() {
            return None; // open block comment swallows the fold marker
        }
        // The opener line must not carry code after the `{` — folding right
        // after it would cut a statement in half (`{ return very_long(...`)
        let after_open = &source[body_start + 1..opener_nl];
        let after_open_t = after_open.trim();
        let comment_only = after_open_t.is_empty()
            || after_open_t.starts_with("//")
            || (after_open_t.starts_with("/*") && after_open_t.ends_with("*/"));
        if !comment_only {
            return None;
        }
        let last_byte = body_end.saturating_sub(1);
        let closer_line_start = source[..last_byte]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(sym_start);
        if closer_line_start <= opener_nl {
            return None; // single-line brace body
        }
        let middle = &source[opener_nl + 1..closer_line_start];
        // A struct/class/enum specifier's terminating `;` lives OUTSIDE the
        // specifier node, on the parent declaration. When the fold ends on a
        // bare closer `}`, carry the same-line `;` so a folded
        // `struct Pseudo { … };` stays parseable instead of losing its `;`.
        let mut tail_end = sym_end;
        if source[closer_line_start..sym_end].trim_end().ends_with('}')
            && let Some(line) = source[sym_end..].split('\n').next()
        {
            let lead = line.len() - line.trim_start_matches([' ', '\t']).len();
            if line[lead..].starts_with(';') {
                tail_end = sym_end + lead + 1;
            }
        }
        let tail = source[closer_line_start..tail_end].trim_end_matches('\n');
        let tail_first = tail.lines().next().unwrap_or("");
        if !is_closer(tail_first) {
            return None; // inline `}` in minified source
        }
        // The closer line must not close more braces than the header opens —
        // consecutive `}}`/`}}}` (nested namespaces collapsing onto one line)
        // would otherwise leave the fold unbalanced.
        if tail.matches('}').count() > header.matches('{').count() {
            return None;
        }
        let keep_tail = parsed.language.keeps_brace_closers();
        let omitted = middle.lines().count() + usize::from(!keep_tail);
        if omitted == 0 {
            return None;
        }
        let indent = leading_whitespace(middle.lines().next().unwrap_or(""));
        let mut out = String::from(header);
        out.push('\n');
        out.push_str(indent);
        out.push_str(parsed.language.comment_prefix());
        out.push_str(" ... [");
        out.push_str(&omitted.to_string());
        out.push_str(" lines omitted]");
        if keep_tail {
            out.push('\n');
            out.push_str(tail);
        }
        if !out.ends_with('\n') {
            out.push('\n');
        }
        return Some(out);
    }

    // Indentation / keyword body (python, ruby, lua): no `{` opener. The
    // body region runs from the body node start to the symbol end; the last
    // line is kept only when it is an `end` closer.
    let header_src = &source[sym_start..body_start];
    let header = match header_src.rfind('\n') {
        None => return None,
        Some(nl) if !header_src[nl + 1..].trim().is_empty() => return None,
        Some(nl) => header_src[..nl].trim_end(),
    };
    let body_region = source[body_start..sym_end].trim_end_matches('\n');
    let body_lines: Vec<&str> = body_region.lines().collect();
    let last = body_lines.last().copied().unwrap_or("");
    let keep_closer = is_end_closer(last.trim());
    let omitted = body_lines.len() - usize::from(keep_closer);
    if omitted == 0 {
        return None;
    }
    let indent = leading_whitespace(body_lines[0]);
    let mut out = String::from(header);
    out.push('\n');
    out.push_str(indent);
    out.push_str(parsed.language.comment_prefix());
    out.push_str(" ... [");
    out.push_str(&omitted.to_string());
    out.push_str(" lines omitted]");
    let closer = parsed.language.comment_close();
    if !closer.is_empty() {
        out.push(' ');
        out.push_str(closer);
    }
    if keep_closer {
        out.push('\n');
        out.push_str(last);
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Some(out)
}

/// Which multi-line string delimiter is currently open, if any.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StrDelim {
    None,
    Quote,
    Backtick,
}

/// Advance the string-delimiter state across one line. `"` toggles the quote
/// state, `` ` `` the backtick state — each delimiter is inert inside the
/// other's string (a go raw string may contain `"`, a js template may contain
/// quotes), so the active delimiter is tracked explicitly rather than as a
/// single odd/even toggle. Escapes (`\`) are skipped except inside raw
/// strings, where they are literal. Comment content and char literals are
/// masked out via `code_chars`.
fn advance_str_state(
    start: StrDelim,
    line: &str,
    abs_start: usize,
    comments: &[Range<usize>],
) -> StrDelim {
    let mut state = start;
    let mut escaped = false;
    for ch in code_chars(line, abs_start, comments) {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && state != StrDelim::Backtick {
            escaped = true;
            continue;
        }
        match state {
            StrDelim::None => {
                if ch == '"' {
                    state = StrDelim::Quote;
                } else if ch == '`' {
                    state = StrDelim::Backtick;
                }
            }
            StrDelim::Quote if ch == '"' => state = StrDelim::None,
            StrDelim::Backtick if ch == '`' => state = StrDelim::None,
            _ => {}
        }
    }
    state
}

/// Collect the byte ranges of all comment nodes inside a subtree, sorted by
/// start. Comment kinds vary per grammar (`comment`, `line_comment`,
/// `block_comment`, `doc_comment`); the shared `contains("comment")` covers
/// them. String-literal docstrings (python) are NOT comments.
fn collect_comment_ranges(node: tree_sitter::Node, out: &mut Vec<Range<usize>>) {
    if node.kind().contains("comment") {
        out.push(node.byte_range());
        return;
    }
    if node.child_count() > 0 {
        let mut cursor = node.walk();
        for kid in node.children(&mut cursor) {
            collect_comment_ranges(kid, out);
        }
    }
}

/// Chars of `line` (at absolute byte offset `abs_start` in the source) with
/// chars inside AST comment ranges removed. `comments` must be sorted by
/// start; the cursor advances monotonically.
fn masked_chars<'a>(
    line: &'a str,
    abs_start: usize,
    comments: &'a [Range<usize>],
) -> impl Iterator<Item = char> + 'a {
    let mut ci = 0usize;
    while ci < comments.len() && comments[ci].end <= abs_start {
        ci += 1;
    }
    line.char_indices().filter_map(move |(rel, ch)| {
        let abs = abs_start + rel;
        while ci < comments.len() && comments[ci].end <= abs {
            ci += 1;
        }
        if ci < comments.len() && comments[ci].start <= abs {
            None
        } else {
            Some(ch)
        }
    })
}

/// Chars of `line` outside comments and outside char-literal spans (`'X'`,
/// `'\X'`). Quotes/parens inside `'"'`-style char literals must not feed the
/// parity/balance scanners. Rust lifetimes (`'static`) have no closing quote
/// and are left alone.
fn code_chars(line: &str, abs_start: usize, comments: &[Range<usize>]) -> Vec<char> {
    let chars: Vec<char> = masked_chars(line, abs_start, comments).collect();
    let mut out = Vec::with_capacity(chars.len());
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c == '\''
            && ((i + 2 < chars.len() && chars[i + 2] == '\'')
                || (i + 3 < chars.len() && chars[i + 1] == '\\' && chars[i + 3] == '\''))
        {
            i += if chars[i + 1] == '\\' { 4 } else { 3 };
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// True if the line is a block closer: it starts with `}`, `]`, `)`
/// (optionally followed by more tokens, as in `} Point;` typedef closes), or
/// is an `end` keyword line (ruby/lua).
fn is_closer(line: &str) -> bool {
    let t = line.trim();
    let c = t.as_bytes().first().copied().unwrap_or(0);
    (c == b'}' || c == b']' || c == b')') || is_end_closer(t)
}

/// True if the line is an `end` keyword closer (ruby/lua).
fn is_end_closer(line: &str) -> bool {
    let t = line.trim().trim_end_matches(';');
    t == "end" || t.starts_with("end ")
}

/// True if the boundary between two lines is mid-expression: the previous
/// line ends with a continuation token (`+`, `,`, `(`, `=` …) or the next
/// line starts with one (`+ __str._M_check(...)`, `&& cond`). A fold marker
/// there would produce invalid syntax. Comment lines never continue — a
/// sentence period at end of line is not a continuation. Inside preprocessor
/// directives (`in_preproc`) `\` splices are the directive's own business and
/// never slide the fold.
fn boundary_continues(
    prev: &str,
    next: &str,
    prev_in_string: bool,
    prev_open_parens: usize,
    parsed: &ParsedSource,
    in_preproc: bool,
) -> bool {
    let prefix = parsed.language.comment_prefix();
    // A multi-line string literal (js template, python docstring, go raw
    // string) cannot be folded through — the marker would land inside it.
    // Checked before the comment-prefix rule: `//`-looking content lines
    // inside a raw string are string data, not comments.
    if prev_in_string {
        return true;
    }
    if prev.trim_start().starts_with(prefix) {
        return false;
    }
    let prev = prev.trim_end();
    let mut tokens = [
        '+', '-', '*', '/', '%', '=', '(', '[', ',', '&', '|', '~', '^', '<', '\\',
    ];
    if in_preproc {
        tokens[14] = '\0';
    }
    if prev.ends_with(tokens) {
        return true;
    }
    // An open `(`/`[` pending across the boundary (multi-line annotations,
    // long call chains) must not be orphaned by the fold marker.
    if prev_open_parens > 0 {
        return true;
    }
    if next.trim_start().starts_with([
        '+', '-', '/', '=', '.', ':', '&', '|', '?', '<', ';', '"', '\'',
    ]) {
        return true;
    }
    // A lone token at the end of the line (`__extension__`, `type`,
    // `return`) is incomplete — it needs the next line.
    let prev_t = prev.trim();
    !prev_t.is_empty()
        && !prev_t.ends_with(';')
        && !prev_t.contains(char::is_whitespace)
        && !parsed.language.is_opener_line(prev_t)
        && !is_closer(prev_t)
}

/// True if the previous line ends inside a block comment that continues past
/// the fold boundary — the marker would land inside the comment.
fn splits_comment(prev: &str, prev_abs_start: usize, comments: &[Range<usize>]) -> bool {
    let prev_end = prev_abs_start + prev.len();
    comments
        .iter()
        .any(|c| c.start < prev_end && c.end > prev_end)
}

/// For `)`/`]` closers, the kept header must contain the matching opener;
/// otherwise the closer would be an orphan (e.g. a multi-line `return (…)`
/// folded to `def f(): … )`).
fn closer_balances(closer: &str, header: &[&str]) -> bool {
    let t = closer.trim();
    let (open, close) = if t.starts_with(')') {
        ('(', ')')
    } else if t.starts_with(']') {
        ('[', ']')
    } else {
        return true;
    };
    let (mut o, mut c) = (0usize, 0usize);
    for line in header {
        for ch in line.chars() {
            if ch == open {
                o += 1;
            } else if ch == close {
                c += 1;
            }
        }
    }
    o > c
}

/// Leading spaces/tabs of a line (byte-safe for ASCII whitespace).
fn leading_whitespace(line: &str) -> &str {
    let len = line.len() - line.trim_start_matches([' ', '\t']).len();
    &line[..len]
}
