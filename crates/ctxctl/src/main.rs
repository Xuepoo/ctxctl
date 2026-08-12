//! `ctxctl` — pure CLI, zero-MCP, stateless context layer for AI coding agents.
//!
//! Implements the CLI contract in `ctxctl-docs/cli-contract.md` (v0.1):
//!
//! - `outline <file>` — symbol outline plus saved% token stats
//! - `symbol <file> --name <s>` — original source slice of one symbol
//! - `read <file> --lines 100-150,200-210` — raw slices of line ranges
//! - `exec <cmd> [--keep <pat>]` — run a command, compress its output
//!
//! Byte-stable by design: output is a pure function of the inputs and config.
//! No timestamps, no counters, no environment dependence.

mod config;
mod deps;

use clap::{Parser, Subcommand, ValueEnum};
use config::Config;
use ctx_symbol::{ParsedSource, Symbol, SymbolKind};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "ctxctl",
    version,
    about = "Pure CLI, zero-MCP, stateless context layer for AI coding agents."
)]
struct Cli {
    /// Explicit config file; highest priority (cli-contract.md §6).
    #[arg(long, value_name = "PATH", global = true)]
    config: Option<PathBuf>,
    /// Output format; `json` is the machine contract.
    #[arg(long, value_enum, default_value_t = Format::Text, global = true)]
    format: Format,
    /// Alias for --format=json.
    #[arg(long, global = true)]
    json: bool,
    /// Disable ANSI colors (accepted for contract compliance; output is
    /// already plain).
    #[arg(long, global = true)]
    no_color: bool,
    /// Suppress saved% metrics.
    #[arg(long, global = true)]
    no_saved: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum Format {
    Text,
    Json,
}

#[derive(Subcommand)]
enum Command {
    /// Print a symbol outline of a source file with token savings.
    Outline {
        /// Target file.
        file: PathBuf,
        /// Omit doc comments.
        #[arg(long)]
        no_doc: bool,
        /// Omit line numbers.
        #[arg(long)]
        no_lines: bool,
    },
    /// Print the original source slice of a single symbol.
    Symbol {
        /// Target file.
        file: PathBuf,
        /// Symbol name (exact).
        #[arg(long)]
        name: String,
        /// Return the signature only, not the body.
        #[arg(long, conflicts_with = "compact")]
        signature: bool,
        /// Return an AST-pruned view: signature + fold marker for the body.
        #[arg(long)]
        compact: bool,
        /// Sub-range within the symbol, 1-based, e.g. 3-10.
        #[arg(long, value_name = "N-M", conflicts_with = "compact")]
        lines: Option<String>,
    },
    /// Read a 1-based line range from the original source (no AST).
    Read {
        /// Target file.
        file: PathBuf,
        /// Line range(s), comma-separated, e.g. 100-150,200-210.
        #[arg(long)]
        lines: String,
    },
    /// Print the import/module dependency graph of a file.
    Deps {
        /// Target file.
        file: PathBuf,
    },
    /// Run a command and print its output compressed by ctx-exec.
    Exec {
        /// Command line to run; shell-word quoting applies, e.g. `"cargo test -- --list"`.
        #[arg(allow_hyphen_values = true)]
        cmd: String,
        /// Extra keep regex (rg syntax), appended to configured patterns.
        #[arg(long = "keep", value_name = "PATTERN", action = clap::ArgAction::Append)]
        keep: Vec<String>,
        /// Override configured head summary lines.
        #[arg(long)]
        head: Option<usize>,
        /// Override configured tail summary lines.
        #[arg(long)]
        tail: Option<usize>,
    },
}

/// An error carrying the exit code required by the CLI contract (§5).
struct ExitError {
    code: u8,
    message: String,
}

impl ExitError {
    fn new(code: u8, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ExitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(code) => code,
        Err(err) => {
            let json_mode = cli.json || cli.format == Format::Json;
            if json_mode {
                println!(
                    "{}",
                    json!({ "error": { "code": err.code, "message": err.message } })
                );
            } else {
                eprintln!("error: {}", err.message);
            }
            ExitCode::from(err.code)
        }
    }
}

fn run(cli: &Cli) -> Result<ExitCode, ExitError> {
    let config = config::load(cli.config.as_deref()).map_err(|e| ExitError::new(1, e))?;
    let format = if cli.json { Format::Json } else { cli.format };
    let show_saved = config.general.show_saved && !cli.no_saved;
    match &cli.command {
        Command::Outline {
            file,
            no_doc,
            no_lines,
        } => run_outline(file, *no_doc, *no_lines, format, show_saved, &config),
        Command::Symbol {
            file,
            name,
            signature,
            compact,
            lines,
        } => run_symbol(
            file,
            name,
            *signature,
            *compact,
            lines.as_deref(),
            format,
            show_saved,
        ),
        Command::Read { file, lines } => run_read(file, lines, format, show_saved),
        Command::Deps { file } => run_deps(file, format, show_saved, &config),
        Command::Exec {
            cmd,
            keep,
            head,
            tail,
        } => run_exec(cmd, keep, *head, *tail, format, show_saved, &config),
    }
}

fn run_outline(
    path: &Path,
    no_doc: bool,
    no_lines: bool,
    format: Format,
    show_saved: bool,
    config: &Config,
) -> Result<ExitCode, ExitError> {
    let source = read_source(path)?;
    let parsed = parse_or(path, &source)?;
    let symbols = ctx_symbol::extract_symbols(&parsed);
    let file_tokens = tokens(&source);
    let delivered_tokens: usize = symbols.iter().map(|s| s.byte_range.len() / 4).sum();
    let saved = saved_pct(file_tokens, delivered_tokens);
    let show_doc = config.outline.show_doc && !no_doc;

    if format == Format::Json {
        let entries: Vec<serde_json::Value> = symbols
            .iter()
            .map(|s| symbol_entry(s, !show_doc, no_lines))
            .collect();
        let mut payload = json!({
            "schema_version": 1,
            "tool": "outline",
            "path": path.display().to_string(),
            "language": parsed.language.name(),
            "symbols": entries,
        });
        if show_saved {
            payload["saved"] = json!({
                "tokens_before": file_tokens,
                "tokens_after": delivered_tokens,
                "percent": saved,
            });
        }
        println!("{payload}");
        return Ok(ExitCode::SUCCESS);
    }

    let header = if show_saved {
        format!(
            "# {}  [{} symbols, {} -> {} tokens, saved ~{}%]",
            path.display(),
            symbols.len(),
            human_bytes(source.len()),
            group(delivered_tokens),
            saved,
        )
    } else {
        format!("# {}  [{} symbols]", path.display(), symbols.len())
    };
    println!("{header}");

    // [outline] fold_threshold (§7): fold the symbol list in text mode when it
    // exceeds the threshold. JSON stays complete (machine contract).
    let fold_limit = config.outline.fold_threshold;
    let folded = symbols.len() > fold_limit;
    let shown: &[Symbol] = if folded {
        &symbols[..fold_limit]
    } else {
        &symbols
    };

    if !shown.is_empty() {
        let name_w = shown
            .iter()
            .map(|s| s.name.len())
            .max()
            .unwrap_or(0)
            .max(10);
        for s in shown {
            let loc = if no_lines {
                String::new()
            } else if s.start_line == s.end_line {
                format!("L:{}", s.start_line)
            } else {
                format!("L:{}-{}", s.start_line, s.end_line)
            };
            println!(
                "  {:<7} {:<name_w$} {:<9}    {}",
                kind_alias(s.kind),
                s.name,
                loc,
                s.signature,
            );
        }
    }
    if folded {
        println!("  ... [{} symbols omitted]", symbols.len() - fold_limit);
    }
    Ok(ExitCode::SUCCESS)
}

fn run_symbol(
    path: &Path,
    name: &str,
    signature_only: bool,
    compact: bool,
    subrange: Option<&str>,
    format: Format,
    show_saved: bool,
) -> Result<ExitCode, ExitError> {
    let source = read_source(path)?;
    let parsed = parse_or(path, &source)?;
    let symbols = ctx_symbol::extract_symbols(&parsed);
    let symbol = symbols
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| ExitError::new(4, format!("symbol not found: {name}")))?;
    let body = slice_text(&source, &symbol.byte_range)?;
    let slice = if compact {
        ctx_symbol::compact_symbol(&parsed, symbol)
    } else if signature_only {
        symbol.signature.clone()
    } else if let Some(range) = subrange {
        slice_lines(&body, range).map_err(|e| ExitError::new(2, e))?
    } else {
        body
    };
    let file_tokens = tokens(&source);
    let delivered_tokens = tokens(&slice);
    let saved = saved_pct(file_tokens, delivered_tokens);

    if format == Format::Json {
        let mut payload = json!({
            "schema_version": 1,
            "tool": "symbol",
            "path": path.display().to_string(),
            "language": parsed.language.name(),
            "name": symbol.name,
            "symbol": symbol_entry(symbol, false, false),
        });
        if compact {
            payload["compact"] = json!(slice);
        } else {
            payload["slice"] = json!(slice);
        }
        if show_saved {
            payload["saved"] = json!({
                "tokens_before": file_tokens,
                "tokens_after": delivered_tokens,
                "percent": saved,
            });
        }
        println!("{payload}");
        return Ok(ExitCode::SUCCESS);
    }

    let loc = if symbol.start_line == symbol.end_line {
        format!("{}:{}", path.display(), symbol.start_line)
    } else {
        format!(
            "{}:{}-{}",
            path.display(),
            symbol.start_line,
            symbol.end_line
        )
    };
    if show_saved {
        println!(
            "# {}  {}  ({} tokens, saved ~{}%)",
            symbol.name,
            loc,
            group(delivered_tokens),
            saved,
        );
    } else {
        println!("# {}  {}", symbol.name, loc);
    }
    println!("{slice}");
    Ok(ExitCode::SUCCESS)
}

fn run_read(
    path: &Path,
    raw: &str,
    format: Format,
    show_saved: bool,
) -> Result<ExitCode, ExitError> {
    let source = read_source(path)?;
    let file_lines: Vec<&str> = source.lines().collect();
    let ranges = parse_ranges(raw).map_err(|e| ExitError::new(2, e))?;
    let ranges: Vec<(usize, usize)> = clamp_open_ends(ranges, file_lines.len());
    for (start, end) in &ranges {
        if *start > file_lines.len() {
            return Err(ExitError::new(
                2,
                format!(
                    "line range {start}-{end} out of bounds (file has {} lines)",
                    file_lines.len()
                ),
            ));
        }
    }
    let slices: Vec<(usize, usize, String)> = ranges
        .iter()
        .map(|(start, end)| (*start, *end, file_lines[start - 1..*end].join("\n")))
        .collect();
    let delivered_tokens: usize = slices.iter().map(|(_, _, text)| tokens(text)).sum();
    let file_tokens = tokens(&source);
    let saved = saved_pct(file_tokens, delivered_tokens);

    if format == Format::Json {
        let ranges_json: Vec<serde_json::Value> = slices
            .iter()
            .map(
                |(start, end, text)| json!({ "start_line": start, "end_line": end, "slice": text }),
            )
            .collect();
        let mut payload = json!({
            "schema_version": 1,
            "tool": "read",
            "path": path.display().to_string(),
            "ranges": ranges_json,
        });
        if show_saved {
            payload["saved"] = json!({
                "tokens_before": file_tokens,
                "tokens_after": delivered_tokens,
                "percent": saved,
            });
        }
        println!("{payload}");
        return Ok(ExitCode::SUCCESS);
    }

    for (start, end, text) in &slices {
        println!("# {}:{}-{}", path.display(), start, end);
        println!("{text}");
    }
    if show_saved {
        println!(
            "Saved ~{}% ({} -> {} tokens)",
            saved,
            group(file_tokens),
            group(delivered_tokens),
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn run_exec(
    cmd: &str,
    keep: &[String],
    head: Option<usize>,
    tail: Option<usize>,
    format: Format,
    show_saved: bool,
    config: &Config,
) -> Result<ExitCode, ExitError> {
    let words = shell_words::split(cmd)
        .map_err(|e| ExitError::new(1, format!("invalid command line: {e}")))?;
    if words.is_empty() {
        return Err(ExitError::new(1, "empty command"));
    }
    let output = StdCommand::new(&words[0])
        .args(&words[1..])
        .output()
        .map_err(|e| ExitError::new(1, format!("failed to run `{cmd}`: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut raw = stdout.to_string();
    if !stdout.is_empty() && !stderr.is_empty() && !stdout.ends_with('\n') {
        raw.push('\n');
    }
    raw.push_str(&stderr);

    let mut options = ctx_exec::CompressOptions {
        keep_patterns: config.exec.keep.clone(),
        head_lines: config.exec.head_lines,
        tail_lines: config.exec.tail_lines,
        collapse_threshold: config.exec.collapse_threshold,
    };
    options.keep_patterns.extend(keep.iter().cloned());
    if let Some(head) = head {
        options.head_lines = head;
    }
    if let Some(tail) = tail {
        options.tail_lines = tail;
    }

    let result =
        ctx_exec::compress(&raw, &options).map_err(|e| ExitError::new(1, e.to_string()))?;
    let stats = result.stats;
    let code = output.status.code().unwrap_or(1);

    if format == Format::Json {
        let mut payload = json!({
            "schema_version": 1,
            "tool": "exec",
            "cmd": cmd,
            "exit_code": code,
            "compressed": result.text,
        });
        if show_saved {
            payload["saved"] = json!({
                "tokens_before": stats.original_tokens,
                "tokens_after": stats.compressed_tokens,
                "percent": stats.saved_percent,
            });
        }
        println!("{payload}");
        return Ok(ExitCode::from(code as u8));
    }

    println!("$ {cmd}");
    if !result.text.is_empty() {
        println!("{}", result.text);
    }
    if show_saved {
        println!(
            "Saved ~{}% ({} -> {} tokens)",
            stats.saved_percent,
            group(stats.original_tokens),
            group(stats.compressed_tokens),
        );
    }
    Ok(ExitCode::from(code as u8))
}

fn run_deps(
    path: &Path,
    format: Format,
    show_saved: bool,
    config: &Config,
) -> Result<ExitCode, ExitError> {
    let source = read_source(path)?;
    let parsed = parse_or(path, &source)?;
    let imports = ctx_symbol::extract_imports(&parsed);
    let file_dir = path.parent().unwrap_or(Path::new("."));
    let resolved = deps::resolve(
        &imports,
        parsed.language.name(),
        file_dir,
        &config.paths.ignore,
    );
    let file_tokens = tokens(&source);
    let delivered_tokens: usize = resolved.iter().map(|r| r.bytes / 4).sum();
    let saved = saved_pct(file_tokens, delivered_tokens);

    if format == Format::Json {
        let entries: Vec<serde_json::Value> = resolved
            .iter()
            .map(|r| json!({ "target": r.target, "kind": r.kind, "line": r.line }))
            .collect();
        let mut payload = json!({
            "schema_version": 1,
            "tool": "deps",
            "path": path.display().to_string(),
            "language": parsed.language.name(),
            "imports": entries,
        });
        if show_saved {
            payload["saved"] = json!({
                "tokens_before": file_tokens,
                "tokens_after": delivered_tokens,
                "percent": saved,
            });
        }
        println!("{payload}");
        return Ok(ExitCode::SUCCESS);
    }

    let header = if show_saved {
        format!(
            "# {}  [{} imports, {} -> {} tokens, saved ~{}%]",
            path.display(),
            resolved.len(),
            human_bytes(source.len()),
            group(delivered_tokens),
            saved,
        )
    } else {
        format!("# {}  [{} imports]", path.display(), resolved.len())
    };
    println!("{header}");
    if !resolved.is_empty() {
        let name_w = resolved
            .iter()
            .map(|r| r.target.len())
            .max()
            .unwrap_or(0)
            .max(10);
        for r in &resolved {
            println!(
                "  {:<9} {:<name_w$}    L:{}",
                dep_kind_name(r.kind),
                r.target,
                r.line,
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn dep_kind_name(kind: deps::DepKind) -> &'static str {
    match kind {
        deps::DepKind::Local => "local",
        deps::DepKind::External => "external",
        deps::DepKind::Ignored => "ignored",
    }
}

fn read_source(path: &Path) -> Result<String, ExitError> {
    std::fs::read_to_string(path)
        .map_err(|e| ExitError::new(1, format!("failed to read {}: {e}", path.display())))
}

fn parse_or(path: &Path, source: &str) -> Result<ParsedSource, ExitError> {
    ctx_symbol::parse(path, source).map_err(|e| match e {
        ctx_symbol::SymbolError::UnsupportedLanguage(p) => {
            ExitError::new(2, format!("unsupported extension: {p}"))
        }
        ctx_symbol::SymbolError::Parse(m) => ExitError::new(3, format!("parse failure: {m}")),
        ctx_symbol::SymbolError::Io(e) => ExitError::new(1, e.to_string()),
        ctx_symbol::SymbolError::NotFound(n) => ExitError::new(4, format!("symbol not found: {n}")),
    })
}

fn slice_text(source: &str, range: &std::ops::Range<usize>) -> Result<String, ExitError> {
    let bytes = source
        .as_bytes()
        .get(range.clone())
        .ok_or_else(|| ExitError::new(3, "symbol byte range out of bounds"))?;
    std::str::from_utf8(bytes)
        .map(|s| s.to_string())
        .map_err(|e| ExitError::new(3, format!("symbol slice is not valid UTF-8: {e}")))
}

/// Slice 1-based inclusive line numbers out of a multi-line text.
fn slice_lines(text: &str, range: &str) -> Result<String, String> {
    let ranges = parse_ranges(range)?;
    if ranges.len() > 1 {
        return Err("symbol --lines accepts a single range (e.g. 3-10)".to_string());
    }
    let lines: Vec<&str> = text.lines().collect();
    let (start, end) = clamp_open_ends(ranges, lines.len())[0];
    if start > lines.len() {
        return Err(format!(
            "line range {start}-{end} out of bounds (symbol has {} lines)",
            lines.len()
        ));
    }
    Ok(lines[start - 1..end].join("\n"))
}

/// Resolve open-ended ranges (`10-`) to an explicit end within `limit`.
fn clamp_open_ends(ranges: Vec<(usize, usize)>, limit: usize) -> Vec<(usize, usize)> {
    ranges
        .into_iter()
        .map(|(start, end)| (start, end.min(limit)))
        .collect()
}

/// Parse comma-separated, 1-based inclusive ranges: `100-150,200-210`, `42`,
/// or `10-` (open-ended). The end of an open range resolves to `usize::MAX`
/// and is clamped by the caller.
fn parse_ranges(raw: &str) -> Result<Vec<(usize, usize)>, String> {
    let tokens: Vec<&str> = raw
        .split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.is_empty() {
        return Err("empty line range".to_string());
    }
    tokens.iter().map(|tok| parse_range(tok)).collect()
}

fn parse_range(tok: &str) -> Result<(usize, usize), String> {
    let (start_s, end_s) = match tok.split_once('-') {
        Some((s, e)) => (s.trim(), e.trim()),
        None => (tok, tok),
    };
    let start: usize = start_s
        .parse()
        .map_err(|_| format!("invalid range `{tok}`"))?;
    if start == 0 {
        return Err("ranges are 1-based; start must be >= 1".to_string());
    }
    let end: usize = if end_s.is_empty() {
        usize::MAX
    } else {
        end_s
            .parse()
            .map_err(|_| format!("invalid range `{tok}`"))?
    };
    if start > end {
        return Err(format!("range {start}-{end} is inverted"));
    }
    Ok((start, end))
}

/// JSON symbol entry per the outline contract (§4.1).
fn symbol_entry(symbol: &Symbol, no_doc: bool, no_lines: bool) -> serde_json::Value {
    let mut entry = json!({
        "name": symbol.name,
        "kind": kind_name(symbol.kind),
        "signature": symbol.signature,
    });
    if !no_lines {
        entry["start_line"] = json!(symbol.start_line);
        entry["end_line"] = json!(symbol.end_line);
    }
    if !no_doc {
        if let Some(doc) = &symbol.doc_comment {
            entry["doc_comment"] = json!(doc);
        }
    }
    entry
}

fn kind_name(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Class => "class",
        SymbolKind::Function => "function",
        SymbolKind::Method => "method",
        SymbolKind::Struct => "struct",
        SymbolKind::Enum => "enum",
        SymbolKind::Interface => "interface",
        SymbolKind::Const => "const",
        SymbolKind::Variable => "var",
        SymbolKind::Trait => "trait",
        SymbolKind::Type => "type",
    }
}

fn kind_alias(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Class => "class",
        SymbolKind::Function => "fn",
        SymbolKind::Method => "method",
        SymbolKind::Struct => "struct",
        SymbolKind::Enum => "enum",
        SymbolKind::Interface => "interface",
        SymbolKind::Const => "const",
        SymbolKind::Variable => "var",
        SymbolKind::Trait => "trait",
        SymbolKind::Type => "type",
    }
}

/// Token approximation: 4 bytes ~ 1 token (cli-contract.md §8).
fn tokens(text: &str) -> usize {
    text.len() / 4
}

fn saved_pct(before: usize, after: usize) -> u32 {
    if before == 0 {
        return 0;
    }
    ((before - after.min(before)) * 100 / before) as u32
}

fn human_bytes(n: usize) -> String {
    if n < 1024 {
        format!("~{} B", n)
    } else {
        format!("~{:.1} KB", n as f64 / 1024.0)
    }
}

fn group(n: usize) -> String {
    let digits = n.to_string();
    let bytes = digits.as_bytes();
    let mut out = String::with_capacity(digits.len() + 2);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}
