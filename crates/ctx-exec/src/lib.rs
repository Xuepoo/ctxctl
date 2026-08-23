//! `ctx-exec` — token-efficient command output compression.
//!
//! Compresses captured command output while keeping the signal: lines that
//! match critical patterns (error, warning, failed, panic, …) or user-supplied
//! `--keep` patterns survive verbatim, the first/last few lines form a summary
//! head/tail, and everything in between folds into a single deterministic
//! marker: `... [N lines omitted]`.
//!
//! Matching is rule-driven with the `regex` crate — the same engine ripgrep
//! uses — so patterns follow rg's default regex syntax (case-insensitive by
//! default, matching rg's default behavior).
//!
//! Byte-stable by design: output is a pure function of the input text and the
//! options. No timestamps, no counters, no environment dependence.

use regex::{Regex, RegexBuilder};
use serde::Serialize;

/// Default number of leading lines kept verbatim as the head summary.
pub const DEFAULT_HEAD_LINES: usize = 5;

/// Default number of trailing lines kept verbatim as the tail summary.
pub const DEFAULT_TAIL_LINES: usize = 5;

/// Default keep patterns (cli-contract.md §7). Lines matching any of these
/// (case-insensitive) are always kept, wherever they appear in the output.
pub const DEFAULT_KEEP_PATTERNS: &[&str] = &["error", "warning", "failed", "panic", "fatal"];

/// Implicit keep patterns appended to every configured list. These preserve
/// diagnostic *location* lines (`   --> src/foo.rs:12:34`, rustc/cargo
/// style) that carry the file:line a reader needs next to a kept error
/// header but that contain no keep-pattern word themselves.
pub const IMPLICIT_KEEP_PATTERNS: &[&str] = &["^\\s+-->"];

/// A folded result that saved less than this many percent is flagged by
/// [`CompressStats::compression_ineffective`] — typically an over-broad
/// keep-pattern set matching most of the output.
pub const INEFFECTIVE_SAVED_PERCENT_MAX: u32 = 10;

/// Outputs at or below this many lines are passed through uncompressed.
pub const DEFAULT_COLLAPSE_THRESHOLD: usize = 20;

/// Errors produced by the compression engine.
#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    #[error("invalid regex pattern `{pattern}`: {message}")]
    InvalidPattern { pattern: String, message: String },
}

/// Tuning knobs for [`compress`].
#[derive(Debug, Clone)]
pub struct CompressOptions {
    /// Complete keep-pattern list (rg syntax); matching lines are kept
    /// verbatim. Starts from [`DEFAULT_KEEP_PATTERNS`]; replace the whole list
    /// to drop the defaults.
    pub keep_patterns: Vec<String>,
    /// Leading lines kept verbatim as the head summary.
    pub head_lines: usize,
    /// Trailing lines kept verbatim as the tail summary.
    pub tail_lines: usize,
    /// Outputs with at most this many lines pass through uncompressed.
    pub collapse_threshold: usize,
}

impl Default for CompressOptions {
    fn default() -> Self {
        Self {
            keep_patterns: DEFAULT_KEEP_PATTERNS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            head_lines: DEFAULT_HEAD_LINES,
            tail_lines: DEFAULT_TAIL_LINES,
            collapse_threshold: DEFAULT_COLLAPSE_THRESHOLD,
        }
    }
}

/// Deterministic statistics about a compression pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CompressStats {
    pub total_lines: usize,
    pub kept_lines: usize,
    pub omitted_lines: usize,
    pub original_tokens: usize,
    pub compressed_tokens: usize,
    pub saved_percent: u32,
    /// True when the fold path ran (output exceeded the collapse threshold);
    /// false for passthrough results.
    pub collapsed: bool,
}

impl CompressStats {
    /// True when compression ran but saved at most
    /// [`INEFFECTIVE_SAVED_PERCENT_MAX`] percent — the signature of an
    /// over-broad keep-pattern set matching most of the output. Passthrough
    /// results are never flagged.
    pub fn compression_ineffective(&self) -> bool {
        self.collapsed && self.saved_percent <= INEFFECTIVE_SAVED_PERCENT_MAX
    }
}

/// The result of a compression pass: the rendered text plus its statistics.
#[derive(Debug, Clone)]
pub struct CompressResult {
    pub text: String,
    pub stats: CompressStats,
}

/// Real token count via the cl100k_base BPE tokenizer (GPT-4-class; see
/// cli-contract.md §8). A deterministic function of the text, so byte
/// stability is unaffected. The bundled encoding is parsed once and cached.
pub fn estimate_tokens(text: &str) -> usize {
    static BPE: std::sync::OnceLock<tiktoken_rs::CoreBPE> = std::sync::OnceLock::new();
    let bpe = BPE.get_or_init(|| {
        tiktoken_rs::cl100k_base().expect("bundled cl100k_base encoding is corrupt")
    });
    bpe.encode_with_special_tokens(text).len()
}

/// Compress command output.
///
/// Outputs with at most `options.collapse_threshold` lines pass through
/// unchanged. Otherwise kept lines are: the head summary, the tail summary,
/// and every line matching a default keep pattern or a user `--keep` pattern.
/// Omitted runs between kept lines fold into a single `... [N lines omitted]`
/// marker.
///
/// This is the one-shot entry point; it feeds the whole text through a
/// [`StreamCompressor`], so both paths render byte-identical output.
pub fn compress(output: &str, options: &CompressOptions) -> Result<CompressResult, ExecError> {
    let mut compressor = StreamCompressor::new(options)?;
    compressor.push(output.as_bytes());
    Ok(compressor.finish())
}

/// Streaming compressor: feed raw output bytes incrementally (as a command
/// produces them) and render the same compressed view as [`compress`] on
/// finish. Memory stays bounded by the head/tail windows and the lines that
/// match keep patterns — the mass of uninteresting middle lines is only
/// counted, never stored, so a command emitting gigabytes cannot exhaust
/// memory.
///
/// Deterministic: for a given byte stream and options, [`StreamCompressor::finish`]
/// always returns the same result, byte-stable with [`compress`].
pub struct StreamCompressor {
    matcher: KeepMatcher,
    head_lines: usize,
    tail_lines: usize,
    collapse_threshold: usize,
    /// Total lines seen so far (including a final unterminated line).
    total: usize,
    /// Completed head lines (the first `head_lines` lines).
    head: Vec<String>,
    /// The last `tail_lines` lines with their keep-pattern match flags.
    ring: std::collections::VecDeque<(String, bool)>,
    /// Keep-pattern matches that left the tail window: `(omitted_run_before, line)`.
    kept_mid: Vec<(usize, String)>,
    /// Consecutive non-kept middle lines since the last kept line.
    run: usize,
    /// Incremental cl100k token count of the raw bytes fed so far.
    raw_tokens: usize,
    /// Raw bytes of the first `collapse_threshold` lines, held verbatim for
    /// the passthrough case.
    prefix: Vec<u8>,
    /// Number of complete lines buffered in `prefix`.
    prefix_lines: usize,
    /// Bytes of the current (unterminated) line.
    pending: Vec<u8>,
}

impl StreamCompressor {
    /// Create a streaming compressor for `options`.
    pub fn new(options: &CompressOptions) -> Result<Self, ExecError> {
        Ok(Self {
            matcher: KeepMatcher::new(options)?,
            head_lines: options.head_lines,
            tail_lines: options.tail_lines,
            collapse_threshold: options.collapse_threshold,
            total: 0,
            head: Vec::new(),
            ring: std::collections::VecDeque::new(),
            kept_mid: Vec::new(),
            run: 0,
            raw_tokens: 0,
            prefix: Vec::new(),
            prefix_lines: 0,
            pending: Vec::new(),
        })
    }

    /// Feed a chunk of raw output bytes. Chunks may split lines anywhere.
    pub fn push(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
        while let Some(pos) = self.pending.iter().position(|b| *b == b'\n') {
            let chunk: Vec<u8> = self.pending.drain(..=pos).collect();
            let mut line = chunk.as_slice();
            line = &line[..line.len() - 1]; // strip '\n'
            if line.last() == Some(&b'\r') {
                line = &line[..line.len() - 1];
            }
            self.consume(chunk.as_slice(), line);
        }
    }

    /// Render the final compressed view and statistics.
    pub fn finish(mut self) -> CompressResult {
        if !self.pending.is_empty() {
            // Final unterminated line: `str::lines` keeps a trailing `\r`
            // here, so no stripping.
            let chunk = std::mem::take(&mut self.pending);
            self.consume(&chunk, &chunk);
        }
        if self.total <= self.collapse_threshold {
            let text = String::from_utf8_lossy(&self.prefix).into_owned();
            let tokens = estimate_tokens(&text);
            return CompressResult {
                text,
                stats: CompressStats {
                    total_lines: self.total,
                    kept_lines: self.total,
                    omitted_lines: 0,
                    original_tokens: tokens,
                    compressed_tokens: tokens,
                    saved_percent: 0,
                    collapsed: false,
                },
            };
        }
        let kept_lines = self.head.len() + self.kept_mid.len() + self.ring.len();
        let mut out = String::new();
        let mut first = true;
        for line in &self.head {
            if !first {
                out.push('\n');
            }
            out.push_str(line);
            first = false;
        }
        for (omitted, line) in &self.kept_mid {
            if *omitted > 0 {
                if !first {
                    out.push('\n');
                }
                out.push_str(&omit_marker(*omitted));
                first = false;
            }
            if !first {
                out.push('\n');
            }
            out.push_str(line);
            first = false;
        }
        if self.run > 0 {
            if !first {
                out.push('\n');
            }
            out.push_str(&omit_marker(self.run));
            first = false;
        }
        for (line, _) in &self.ring {
            if !first {
                out.push('\n');
            }
            out.push_str(line);
            first = false;
        }
        let compressed_tokens = estimate_tokens(&out);
        CompressResult {
            text: out,
            stats: CompressStats {
                total_lines: self.total,
                kept_lines,
                omitted_lines: self.total - kept_lines,
                original_tokens: self.raw_tokens,
                compressed_tokens,
                saved_percent: saved_pct(self.raw_tokens, compressed_tokens),
                collapsed: true,
            },
        }
    }

    /// Process one complete line (`chunk` includes the `\n` terminator, or is
    /// the final unterminated line).
    fn consume(&mut self, chunk: &[u8], line: &[u8]) {
        self.total += 1;
        // Incremental token count. Chunks include their terminator, so counts
        // equal whole-text cl100k counts except for rare runs of 2+ blank
        // lines after punctuation (a regex chunk swallows several `\n`s at
        // once); the drift is a token or two and `saved%` is approximate by
        // contract.
        self.raw_tokens += estimate_tokens(&String::from_utf8_lossy(chunk));
        if self.total <= self.collapse_threshold {
            self.prefix.extend_from_slice(chunk);
            self.prefix_lines += 1;
            return;
        }
        if !self.prefix.is_empty() {
            let prefix = std::mem::take(&mut self.prefix);
            let mut rest: &[u8] = &prefix;
            let mut line_no = 0usize;
            while let Some(pos) = rest.iter().position(|b| *b == b'\n') {
                let mut l = &rest[..pos];
                if l.last() == Some(&b'\r') {
                    l = &l[..l.len() - 1];
                }
                line_no += 1;
                self.feed(l, line_no);
                rest = &rest[pos + 1..];
            }
            self.prefix_lines = 0;
        }
        self.feed(line, self.total);
    }

    /// Route one line through the keep/tail state machine. `line_no` is the
    /// line's 1-based position in the stream.
    fn feed(&mut self, line: &[u8], line_no: usize) {
        let text = String::from_utf8_lossy(line);
        let matched = self.matcher.is_match(&text);
        if line_no <= self.head_lines {
            self.head.push(text.into_owned());
        } else {
            self.ring.push_back((text.into_owned(), matched));
            if self.ring.len() > self.tail_lines {
                let (l, m) = self.ring.pop_front().expect("ring not empty");
                if m {
                    self.kept_mid.push((self.run, l));
                    self.run = 0;
                } else {
                    self.run += 1;
                }
            }
        }
    }
}

fn omit_marker(n: usize) -> String {
    format!("... [{n} lines omitted]")
}

fn saved_pct(original: usize, compressed: usize) -> u32 {
    if original == 0 {
        return 0;
    }
    ((original - compressed.min(original)) * 100 / original) as u32
}

/// Case-insensitive matcher over the keep patterns, compiled individually so
/// per-pattern anchoring and capture semantics survive. An empty pattern list
/// matches nothing — it must not degrade to a match-everything regex.
struct KeepMatcher {
    patterns: Vec<Regex>,
}

impl KeepMatcher {
    fn new(options: &CompressOptions) -> Result<Self, ExecError> {
        let mut patterns =
            Vec::with_capacity(options.keep_patterns.len() + IMPLICIT_KEEP_PATTERNS.len());
        for pattern in &options.keep_patterns {
            patterns.push(
                RegexBuilder::new(pattern)
                    .case_insensitive(true)
                    .build()
                    .map_err(|e| ExecError::InvalidPattern {
                        pattern: pattern.clone(),
                        message: e.to_string(),
                    })?,
            );
        }
        // Built-in location markers are compiled with the same settings and
        // cannot fail (constant, valid patterns).
        for pattern in IMPLICIT_KEEP_PATTERNS {
            patterns.push(
                RegexBuilder::new(pattern)
                    .case_insensitive(true)
                    .build()
                    .expect("implicit keep pattern is valid"),
            );
        }
        Ok(Self { patterns })
    }

    fn is_match(&self, line: &str) -> bool {
        self.patterns.iter().any(|re| re.is_match(line))
    }
}
