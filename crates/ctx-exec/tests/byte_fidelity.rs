//! Byte-fidelity and bounded-memory tests (CTX-0036).
//!
//! Covers three contracts:
//! 1. Passthrough preserves raw bytes verbatim, invalid UTF-8 included;
//!    only the compression path is lossy by declaration.
//! 2. A single line longer than the pending budget is deterministically
//!    truncated: same input -> same output, announced by a marker line,
//!    memory stays bounded, and the stream resumes normally afterwards.
//! 3. The ineffective-compression warning fires only when keep patterns
//!    actually contributed kept lines outside the head/tail windows.

use ctx_exec::{CompressOptions, StreamCompressor, compress};

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

fn stream_bytes(bytes: &[u8], options: &CompressOptions) -> ctx_exec::CompressResult {
    let mut sc = StreamCompressor::new(options).unwrap();
    sc.push(bytes);
    sc.finish()
}

// --- Finding 2: passthrough preserves raw bytes -----------------------------

#[test]
fn passthrough_preserves_invalid_utf8_byte_exact() {
    // Three lines, well under the collapse threshold -> passthrough path.
    let input: &[u8] = b"status ok\n\xff\xfe broken \x80 bytes\ntail\n";
    let result = stream_bytes(input, &opts());
    assert_eq!(result.text, input, "passthrough must not mutate bytes");
}

#[test]
fn passthrough_preserves_invalid_utf8_across_chunk_boundaries() {
    // The invalid sequence straddles a chunk split; still byte-exact.
    let input: &[u8] = b"line one\n\xff\xfe tail";
    let mut sc = StreamCompressor::new(&opts()).unwrap();
    sc.push(&input[..11]);
    sc.push(&input[11..]);
    let result = sc.finish();
    assert_eq!(result.text, input);
}

#[test]
fn compressed_path_is_declared_lossy_for_invalid_utf8() {
    // Past the collapse threshold the fold path renders lines through
    // lossy conversion; output is valid UTF-8 with replacement chars.
    let mut input = b"first\n\xff\xfe bad line\n".to_vec();
    for i in 0..25 {
        input.extend_from_slice(format!("filler {i}\n").as_bytes());
    }
    let options = CompressOptions {
        collapse_threshold: 10,
        ..opts()
    };
    let result = stream_bytes(&input, &options);
    assert!(
        String::from_utf8_lossy(&result.text).contains('\u{fffd}'),
        "lossy marker expected"
    );
}

// --- Finding 1: bounded pending buffer, deterministic truncation ------------

/// Same huge newline-free input pushed twice (different chunkings) must
/// render identical bytes, carry an explicit truncation marker, and stay
/// bounded instead of echoing megabytes back.
#[test]
fn overlong_newline_free_stream_is_truncated_deterministically() {
    const BUDGET: usize = 1024 * 1024; // matches PENDING_BUDGET_BYTES
    let input = vec![b'x'; 3 * BUDGET];

    let whole = stream_bytes(&input, &opts());
    let mut sc = StreamCompressor::new(&opts()).unwrap();
    let mut i = 0;
    while i < input.len() {
        let end = (i + 7919).min(input.len());
        sc.push(&input[i..end]);
        i = end;
    }
    let chunked = sc.finish();

    assert_eq!(whole.text, chunked.text, "deterministic across chunkings");
    assert_eq!(whole.stats, chunked.stats);
    let text = String::from_utf8_lossy(&whole.text);
    assert!(
        text.contains("... [line truncated at"),
        "truncation must be announced by a deterministic marker: {}",
        whole.text.len()
    );
    assert!(
        whole.text.len() < input.len(),
        "output must be bounded, got {}",
        whole.text.len()
    );
}

/// After a truncated over-long line terminates, later lines are processed
/// normally again: keep patterns match, counts are exact. The giant line is
/// fed without its terminator first, as a real stream would.
#[test]
fn streaming_resumes_after_truncated_line() {
    const BUDGET: usize = 1024 * 1024; // matches PENDING_BUDGET_BYTES
    let mut sc = StreamCompressor::new(&opts()).unwrap();

    // Push 1: an unterminated line exceeding the budget.
    sc.push(&vec![b'x'; BUDGET + 4096]);
    // Push 2: its terminator plus normal traffic.
    let mut rest = b"\n".to_vec();
    for i in 0..25 {
        rest.extend_from_slice(format!("filler {i}\n").as_bytes());
    }
    rest.extend_from_slice(b"error: boom\n");
    sc.push(&rest);

    let result = sc.finish();
    let text = String::from_utf8_lossy(&result.text);
    assert!(text.contains("error: boom"), "{text}");
    assert!(text.contains("... [line truncated at"), "{text}");
    // Giant line + marker + 25 fillers + error line.
    assert_eq!(result.stats.total_lines, 28);
}

/// A terminated over-long line (has `\n` within budget) is never truncated;
/// only the buffering of unterminated data is capped.
#[test]
fn terminated_long_line_within_budget_is_not_truncated() {
    let mut input = vec![b'y'; BUDGET_TEST_LINE];
    input.push(b'\n');
    input.extend_from_slice(b"plain tail\n");
    let result = stream_bytes(&input, &opts());
    let text = String::from_utf8_lossy(&result.text);
    assert!(!text.contains("truncated"));
    let head: String = text.chars().take(16).collect();
    assert!(head.chars().all(|c| c == 'y'), "long line kept: {head}");
}

const BUDGET_TEST_LINE: usize = 900 * 1024;

// --- Finding 3: warning only when keeps contributed -------------------------

/// When head+tail windows alone cover the whole collapsed output there is
/// nothing to warn about: no keep pattern matched anything, folding simply
/// had almost no middle to omit. Requires `collapse_threshold` below the
/// combined window size, e.g. 9 lines with 4+4 windows and threshold 8.
#[test]
fn window_only_collapse_is_not_flagged_ineffective() {
    let mut options = opts();
    options.collapse_threshold = 8;
    options.head_lines = 4;
    options.tail_lines = 4;
    let input = lines(9, "plain line ");
    let result = compress(&input, &options).unwrap();
    assert!(result.stats.collapsed, "9 lines exceed threshold 8");
    assert_eq!(
        result.stats.pattern_kept_lines, 0,
        "no keep pattern can match these lines"
    );
    assert!(
        !result.stats.compression_ineffective(),
        "window-only collapse must not warn: saved {}%",
        result.stats.saved_percent
    );
}
