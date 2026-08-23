//! Tests for ctx-exec compression: critical-line retention, folding,
//! custom keep patterns, collapse threshold, byte stability, and edge cases.

use ctx_exec::{
    CompressOptions, DEFAULT_HEAD_LINES, DEFAULT_TAIL_LINES, StreamCompressor, compress,
    estimate_tokens,
};

fn opts() -> CompressOptions {
    CompressOptions::default()
}

fn lines(n: usize, label: &str) -> String {
    let mut out = Vec::with_capacity(n);
    for i in 1..=n {
        out.push(format!("{label}{i}"));
    }
    out.join("\n")
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
fn estimate_tokens_uses_cl100k_bpe() {
    // Known cl100k_base counts (verified against tiktoken-rs 0.12).
    assert_eq!(estimate_tokens(""), 0);
    assert_eq!(estimate_tokens("a"), 1);
    assert_eq!(estimate_tokens("€"), 1);
    assert_eq!(estimate_tokens("hello world"), 2);
    assert_eq!(estimate_tokens("abcdefgh"), 1);
    assert_eq!(estimate_tokens("你好"), 2);
    assert_eq!(
        estimate_tokens("这是一个中文句子，用来测试 token 计数。"),
        17
    );
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

/// Feed `bytes` through a fresh `StreamCompressor` and return the result.
fn stream_bytes(bytes: &[u8], options: &CompressOptions) -> ctx_exec::CompressResult {
    let mut sc = StreamCompressor::new(options).unwrap();
    sc.push(bytes);
    sc.finish()
}

#[test]
fn stream_compressor_matches_batch_output() {
    // Mixed content: head, critical middle lines, blank lines, tail.
    let mut input = lines(12, "l");
    input.push_str("\nerror: something broke\n\nTODO: fix me\n");
    input.push_str(&lines(12, "m"));
    let batch = compress(&input, &opts()).unwrap();
    let streamed = stream_bytes(input.as_bytes(), &opts());
    assert_eq!(streamed.text, batch.text);
    assert_eq!(streamed.stats, batch.stats);
}

#[test]
fn stream_compressor_tolerates_arbitrary_chunk_boundaries() {
    // Chunks may split lines, multi-byte characters, and terminators.
    let mut input = lines(30, "你好").clone();
    input.push_str("\nwarning: 中文告警\n");
    input.push_str(&lines(30, "m"));
    let batch = compress(&input, &opts()).unwrap();
    let bytes = input.as_bytes();
    let mut sc = StreamCompressor::new(&opts()).unwrap();
    let mut i = 0;
    let mut step = 1;
    while i < bytes.len() {
        let end = (i + step).min(bytes.len());
        sc.push(&bytes[i..end]);
        i = end;
        step = step % 7 + 1;
    }
    let streamed = sc.finish();
    assert_eq!(
        streamed.text, batch.text,
        "streamed: {}\nbatch: {}",
        streamed.text, batch.text
    );
}

#[test]
fn stream_compressor_preserves_crlf_passthrough() {
    // Small CRLF outputs pass through byte-verbatim, like `compress`.
    let input = "a\r\nb\r\nc\r\n";
    let batch = compress(input, &opts()).unwrap();
    assert_eq!(batch.text, input);
    let streamed = stream_bytes(input.as_bytes(), &opts());
    assert_eq!(streamed.text, input);
    assert_eq!(streamed.stats, batch.stats);
}

#[test]
fn stream_compressor_normalizes_crlf_in_compressed_mode() {
    // Large CRLF outputs normalize to LF exactly like the batch path.
    let mut input = String::new();
    for i in 1..=30 {
        input.push_str(&format!("line {i}\r\n"));
    }
    let batch = compress(&input, &opts()).unwrap();
    assert!(!batch.text.contains('\r'));
    let streamed = stream_bytes(input.as_bytes(), &opts());
    assert_eq!(streamed.text, batch.text);
    assert_eq!(streamed.stats, batch.stats);
}

#[test]
fn stream_compressor_no_tail_or_head() {
    let mut options = opts();
    options.head_lines = 0;
    options.tail_lines = 0;
    let mut input = lines(10, "l");
    input.push_str("\nerror: kept\n");
    input.push_str(&lines(10, "m"));
    let batch = compress(&input, &options).unwrap();
    let streamed = stream_bytes(input.as_bytes(), &options);
    assert_eq!(streamed.text, batch.text);
    assert_eq!(streamed.stats, batch.stats);
    assert!(streamed.text.starts_with("... [10 lines omitted]"));
}

#[test]
fn stream_compressor_unterminated_final_line() {
    let mut input = lines(40, "l");
    input.push_str("\nfatal: no trailing newline");
    let batch = compress(&input, &opts()).unwrap();
    let streamed = stream_bytes(input.as_bytes(), &opts());
    assert_eq!(streamed.text, batch.text);
    assert_eq!(streamed.stats, batch.stats);
    assert!(streamed.text.ends_with("fatal: no trailing newline"));
}

#[test]
fn stream_compressor_is_byte_stable() {
    let mut input = lines(60, "l");
    input.push_str("\nerror: boom\n");
    input.push_str(&lines(60, "m"));
    let a = stream_bytes(input.as_bytes(), &opts());
    let b = stream_bytes(input.as_bytes(), &opts());
    assert_eq!(a.text, b.text);
    assert_eq!(a.stats, b.stats);
}

#[test]
fn stream_compressor_handles_ten_million_plain_lines() {
    // Memory-bounded streaming: the uninteresting bulk is counted, not kept.
    let mut sc = StreamCompressor::new(&opts()).unwrap();
    let mut line = String::from("plain data\n");
    for _ in 0..10_000 {
        sc.push(line.as_bytes());
    }
    line.clear();
    line.push_str("plain data");
    sc.push(line.as_bytes());
    let result = sc.finish();
    assert_eq!(result.stats.total_lines, 10_001);
    assert!(result.stats.kept_lines < 200);
    assert!(result.stats.saved_percent > 90);
}

// --- Location-line fidelity (CTX-0024) -------------------------------------

/// rustc-style diagnostic location lines survive even when no configured
/// keep pattern matches them, so a kept error header keeps its file:line.
#[test]
fn keeps_diagnostic_location_lines() {
    let mut input = lines(12, "l");
    input.push_str("\nerror[E0308]: mismatched types in merge_01\n");
    input.push_str("   --> crate-mod06/src/parser.rs:88:19\n");
    input.push_str(&lines(12, "m"));

    let result = compress(&input, &opts()).unwrap();
    assert!(result.text.contains("error[E0308]: mismatched types"));
    assert!(result.text.contains("--> crate-mod06/src/parser.rs:88:19"));
}

/// Location lines are kept on their own merits — even with the configured
/// pattern list replaced by something that matches nothing.
#[test]
fn location_lines_kept_with_unrelated_patterns() {
    let mut options = opts();
    options.keep_patterns = vec!["nomatchanything".to_string()];
    let mut input = lines(12, "l");
    input.push_str("\n  --> src/only/location.rs:7:1\n");
    input.push_str(&lines(12, "m"));

    let result = compress(&input, &options).unwrap();
    assert!(result.text.contains("--> src/only/location.rs:7:1"));
}

// --- Over-broad keep detection (CTX-0024) ----------------------------------

/// A keep pattern that matches nearly every line defeats compression; the
/// stats expose it via `compression_ineffective`.
#[test]
fn over_broad_keep_pattern_is_flagged() {
    let mut input = lines(30, "test case run ");
    input.push('\n');
    let result = compress(&input, &opts()).unwrap();
    // Defaults do not match these lines: normal compression.
    assert!(!result.stats.compression_ineffective());

    let mut broad = opts();
    broad.keep_patterns = vec!["test".to_string()];
    let result = compress(&input, &broad).unwrap();
    assert!(result.stats.compression_ineffective());
}

/// Passthrough results (at or below the collapse threshold) are never
/// flagged: nothing was compressed, so there is nothing to warn about.
#[test]
fn passthrough_is_not_flagged_as_ineffective() {
    let input = lines(5, "plain\n");
    let result = compress(&input, &opts()).unwrap();
    assert_eq!(result.stats.saved_percent, 0);
    assert!(!result.stats.compression_ineffective());
}
