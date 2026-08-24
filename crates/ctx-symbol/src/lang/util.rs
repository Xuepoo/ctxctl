//! Helpers shared by language backends.

/// Text of a string literal node without its quotes.
pub(crate) fn string_value(node: tree_sitter::Node, source: &str) -> Option<String> {
    let text = node.utf8_text(source.as_bytes()).ok()?.trim();
    let unquoted = text.strip_prefix(['\'', '"'])?;
    Some(
        unquoted
            .strip_suffix(['\'', '"'])
            .unwrap_or(unquoted)
            .to_string(),
    )
}
