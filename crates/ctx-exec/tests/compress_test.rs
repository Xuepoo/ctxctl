//! Tests for ctx-exec compression: critical-line retention, folding,
//! custom keep patterns, collapse threshold, byte stability, and edge cases.

use ctx_exec::{
    CompressOptions, DEFAULT_HEAD_LINES, DEFAULT_TAIL_LINES, compress, estimate_tokens,
};

fn opts() -> CompressOptions {
    CompressOptions::default()
}

fn lines(n: usize, label: &str) -> String {
    (1..=n)
        .map(|i| format!("{label}{i}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn keeps_critical_lines_in_the_middle() {
    let mut input = lines(12, "l");
    input.push_str("\nerror: something broke\n");
    input.push('\n');
    input.push_str(&lines(12, "m"));

    let result = compress(&input, &opts()).unwrap();
    let text = &result.text;

    assert!(text.contains("error: something broke"));
    assert!(text.contains("... [7 lines omitted]"));
    assert!(text.contains("... [8 lines omitted]"));
    assert_eq!(
        result.stats.kept_lines,
        DEFAULT_HEAD_LINES + DEFAULT_TAIL_LINES + 1
    );
    assert_eq!(result.stats.total_lines, 26);
    assert_eq!(
        result.stats.omitted_lines,
        result.stats.total_lines - result.stats.kept_lines
    );
    assert!(result.stats.saved_percent > 0);
}

#[test]
fn keeps_critical_lines_case_insensitively() {
    let mut input = lines(12, "l");
    input.push_str("\nPANIC: boom\nfailed\nwarning\nfatal\n");
    input.push_str(&lines(12, "m"));
    let result = compress(&input, &opts()).unwrap();
    for needle in ["PANIC: boom", "failed", "warning", "fatal"] {
        assert!(result.text.contains(needle), "missing {needle}");
    }
}

#[test]
fn non_critical_lines_are_folded_away() {
    let mut input = lines(12, "l");
    input.push_str("\ntraceback debug note\n");
    input.push_str(&lines(12, "m"));
    let result = compress(&input, &opts()).unwrap();
    assert!(!result.text.contains("traceback debug note"));
    assert!(result.text.contains("... [15 lines omitted]"));
}

#[test]
fn head_and_tail_are_always_kept() {
    let input = lines(24, "l");
    let result = compress(&input, &opts()).unwrap();
    assert_eq!(
        result.stats.kept_lines,
        DEFAULT_HEAD_LINES + DEFAULT_TAIL_LINES
    );
    assert!(
        result
            .text
            .starts_with("l1\nl2\nl3\nl4\nl5\n... [14 lines omitted]")
    );
    assert!(result.text.ends_with("l20\nl21\nl22\nl23\nl24"));
}

#[test]
fn custom_keep_pattern() {
    let mut input = lines(12, "l");
    input.push_str("\nTODO: fix this later\n");
    input.push_str(&lines(12, "m"));

    let no_keep = compress(&input, &opts()).unwrap();
    assert!(!no_keep.text.contains("TODO: fix this later"));

    let mut custom = opts();
    custom.keep_patterns.push("TODO".to_string());
    let with_keep = compress(&input, &custom).unwrap();
    assert!(with_keep.text.contains("TODO: fix this later"));
}

#[test]
fn small_output_passes_through_uncompressed() {
    let input = lines(8, "l");
    let result = compress(&input, &opts()).unwrap();
    assert_eq!(result.text, input);
    assert_eq!(result.stats.omitted_lines, 0);
    assert_eq!(result.stats.saved_percent, 0);
}

#[test]
fn collapse_threshold_turns_off_compression() {
    let mut input = lines(12, "l");
    input.push_str("\nerror: e\n");
    input.push_str(&lines(12, "m"));

    let mut wide = opts();
    wide.collapse_threshold = 26;
    let result = compress(&input, &wide).unwrap();
    assert_eq!(result.text, input);
    assert_eq!(result.stats.omitted_lines, 0);

    let mut narrow = opts();
    narrow.collapse_threshold = 0;
    let result = compress(&input, &narrow).unwrap();
    assert!(result.text.contains("... [7 lines omitted]"));
    assert!(result.stats.omitted_lines > 0);
}

#[test]
fn empty_output() {
    let result = compress("", &opts()).unwrap();
    assert!(result.text.is_empty());
    assert_eq!(result.stats.total_lines, 0);
    assert_eq!(result.stats.saved_percent, 0);
}

#[test]
fn invalid_pattern_errors_with_pattern_name() {
    let mut custom = opts();
    custom.keep_patterns.push("[".to_string());
    let err = compress("whatever", &custom).unwrap_err();
    let message = err.to_string();
    assert!(
        message.contains('['),
        "message should name the pattern: {message}"
    );
}

#[test]
fn invalid_pattern_errors_even_for_small_outputs() {
    let mut custom = opts();
    custom.keep_patterns.push("[".to_string());
    assert!(compress("tiny", &custom).is_err());
}

#[test]
fn output_is_byte_stable() {
    let mut input = lines(30, "l");
    input.push_str("\nerror: e\n");
    input.push_str(&lines(30, "m"));
    let a = compress(&input, &opts()).unwrap();
    let b = compress(&input, &opts()).unwrap();
    assert_eq!(a.text, b.text);
    assert_eq!(a.stats, b.stats);
    assert_eq!(a.text, compress(&input, &opts()).unwrap().text);
}

#[test]
fn single_line_output_is_kept_as_is() {
    let result = compress("error: single", &opts()).unwrap();
    assert_eq!(result.text, "error: single");
}

#[test]
fn estimate_tokens_is_bytes_over_four() {
    assert_eq!(estimate_tokens("abcdefgh"), 2);
    assert_eq!(estimate_tokens("abc"), 0);
}

#[test]
fn saved_percent_is_zero_for_passthrough() {
    let input = lines(8, "l");
    let result = compress(&input, &opts()).unwrap();
    assert_eq!(result.stats.saved_percent, 0);
}

#[test]
fn empty_keep_list_matches_nothing() {
    let mut input = lines(12, "l");
    input.push_str("\nthis line must be omitted\n");
    input.push_str(&lines(12, "m"));

    let mut custom = opts();
    custom.keep_patterns.clear();
    let result = compress(&input, &custom).unwrap();
    assert!(!result.text.contains("this line must be omitted"));
    assert_eq!(
        result.stats.kept_lines,
        DEFAULT_HEAD_LINES + DEFAULT_TAIL_LINES
    );
}

#[test]
fn per_pattern_anchors_survive_individual_compilation() {
    // `^PASS` must not match mid-line occurrences, even when combined with
    // other patterns.
    let mut input = lines(12, "l");
    input.push_str("\nxxPASSxx\n");
    input.push_str("PASS: real hit\n");
    input.push_str(&lines(12, "m"));

    let mut custom = opts();
    custom.keep_patterns.clear();
    custom.keep_patterns.push("^PASS".to_string());
    let result = compress(&input, &custom).unwrap();
    assert!(!result.text.contains("xxPASSxx"), "unanchored match leaked");
    assert!(result.text.contains("PASS: real hit"));
}
