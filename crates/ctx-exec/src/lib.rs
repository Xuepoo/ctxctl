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
}

/// The result of a compression pass: the rendered text plus its statistics.
#[derive(Debug, Clone)]
pub struct CompressResult {
    pub text: String,
    pub stats: CompressStats,
}

/// Token approximation of a text's cost: 4 bytes ~ 1 token, matching the
/// estimate used by `ctx-symbol`.
pub fn estimate_tokens(text: &str) -> usize {
    text.len() / 4
}

/// Compress command output.
///
/// Outputs with at most `options.collapse_threshold` lines pass through
/// unchanged. Otherwise kept lines are: the head summary, the tail summary,
/// and every line matching a default keep pattern or a user `--keep` pattern.
/// Omitted runs between kept lines fold into a single `... [N lines omitted]`
/// marker.
pub fn compress(output: &str, options: &CompressOptions) -> Result<CompressResult, ExecError> {
    let lines: Vec<&str> = output.lines().collect();
    let total = lines.len();
    let original_tokens = estimate_tokens(output);
    let matcher = KeepMatcher::new(options)?;

    if total <= options.collapse_threshold {
        return Ok(CompressResult {
            text: output.to_string(),
            stats: CompressStats {
                total_lines: total,
                kept_lines: total,
                omitted_lines: 0,
                original_tokens,
                compressed_tokens: original_tokens,
                saved_percent: 0,
            },
        });
    }

    let mut kept = Vec::with_capacity(total);
    for (i, line) in lines.iter().enumerate() {
        let in_summary = i < options.head_lines || i >= total.saturating_sub(options.tail_lines);
        kept.push(in_summary || matcher.is_match(line));
    }
    let kept_lines = kept.iter().filter(|k| **k).count();

    let text = render(&lines, &kept);
    let compressed_tokens = estimate_tokens(&text);
    let saved_percent = saved_pct(original_tokens, compressed_tokens);

    Ok(CompressResult {
        text,
        stats: CompressStats {
            total_lines: total,
            kept_lines,
            omitted_lines: total - kept_lines,
            original_tokens,
            compressed_tokens,
            saved_percent,
        },
    })
}

/// Render the kept-line mask into the final text with omission markers.
fn render(lines: &[&str], kept: &[bool]) -> String {
    let mut out = String::new();
    let mut omitted = 0usize;
    let mut first = true;
    for (i, line) in lines.iter().enumerate() {
        if kept[i] {
            if omitted > 0 {
                if !first {
                    out.push('\n');
                }
                out.push_str(&omit_marker(omitted));
                omitted = 0;
                first = false;
            }
            if !first {
                out.push('\n');
            }
            out.push_str(line);
            first = false;
        } else {
            omitted += 1;
        }
    }
    if omitted > 0 {
        if !first {
            out.push('\n');
        }
        out.push_str(&omit_marker(omitted));
    }
    out
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
        let mut patterns = Vec::with_capacity(options.keep_patterns.len());
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
        Ok(Self { patterns })
    }

    fn is_match(&self, line: &str) -> bool {
        self.patterns.iter().any(|re| re.is_match(line))
    }
}
