//! `ctxctl` — CLI-first, stateless context layer for AI coding agents
//! (optional `mcp` adapter).
//!
//! Implements the CLI contract in `docs/cli-contract.md`:
//!
//! - `outline <file>` — symbol outline plus saved% token stats
//! - `symbol <file> --name <s>` — original source slice of one symbol
//! - `read <file> --lines 100-150,200-210` — raw slices of line ranges
//! - `deps <file>` — import/module edges classified local/external/ignored
//! - `exec <cmd> [--keep <pat>]` — run a command, compress its output
//! - `mcp` — serve all commands as MCP tools over stdio (optional adapter)
//!
//! Byte-stable by design: output is a pure function of the inputs and config.
//! No timestamps, no counters, no environment dependence.

mod config;
mod deps;
mod mcp;

use clap::{Parser, Subcommand, ValueEnum};
use config::Config;
use ctx_symbol::{ParsedSource, Symbol, SymbolKind};
use serde_json::json;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::process::Command as StdCommand;
use std::process::ExitCode;
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(
    name = "ctxctl",
    version,
    about = "CLI-first, stateless context layer for AI coding agents (optional MCP adapter)."
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
    /// Write the full payload to this file instead of stdout (bypasses
    /// stdout size limits on large outputs); a confirmation is printed to
    /// stderr.
    #[arg(long, value_name = "PATH", global = true)]
    output: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum Format {
    Text,
    Json,
}

/// Symbol kinds accepted by `symbol --kind`. Values match the outline JSON
/// contract's `kind` field exactly (e.g. `var`, not `variable`).
#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum SymbolKindArg {
    #[value(name = "class")]
    Class,
    #[value(name = "struct")]
    Struct,
    #[value(name = "enum")]
    Enum,
    #[value(name = "interface")]
    Interface,
    #[value(name = "function")]
    Function,
    #[value(name = "method")]
    Method,
    #[value(name = "module")]
    Module,
    #[value(name = "const")]
    Const,
    #[value(name = "var")]
    Variable,
    #[value(name = "trait")]
    Trait,
    #[value(name = "type")]
    Type,
    #[value(name = "heading")]
    Heading,
    #[value(name = "rule")]
    Rule,
    #[value(name = "element")]
    Element,
}

impl SymbolKindArg {
    fn to_symbol_kind(self) -> ctx_symbol::SymbolKind {
        match self {
            SymbolKindArg::Class => ctx_symbol::SymbolKind::Class,
            SymbolKindArg::Struct => ctx_symbol::SymbolKind::Struct,
            SymbolKindArg::Enum => ctx_symbol::SymbolKind::Enum,
            SymbolKindArg::Interface => ctx_symbol::SymbolKind::Interface,
            SymbolKindArg::Function => ctx_symbol::SymbolKind::Function,
            SymbolKindArg::Method => ctx_symbol::SymbolKind::Method,
            SymbolKindArg::Module => ctx_symbol::SymbolKind::Module,
            SymbolKindArg::Const => ctx_symbol::SymbolKind::Const,
            SymbolKindArg::Variable => ctx_symbol::SymbolKind::Variable,
            SymbolKindArg::Trait => ctx_symbol::SymbolKind::Trait,
            SymbolKindArg::Type => ctx_symbol::SymbolKind::Type,
            SymbolKindArg::Heading => ctx_symbol::SymbolKind::Heading,
            SymbolKindArg::Rule => ctx_symbol::SymbolKind::Rule,
            SymbolKindArg::Element => ctx_symbol::SymbolKind::Element,
        }
    }
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
        /// Restrict the match to a symbol kind (class, method, variable, …).
        /// Without it, the first same-name symbol in source order wins.
        #[arg(long, value_enum)]
        kind: Option<SymbolKindArg>,
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
    /// Serve outline/symbol/read/deps/exec as MCP tools over stdio
    /// (newline-delimited JSON-RPC 2.0). Optional adapter; the CLI remains
    /// the canonical interface.
    Mcp,
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

/// Shared output routing: format, metrics toggle, and the `--output` target.
/// `collect` captures payloads in memory instead of stdout/file — used by
/// the `mcp` server, which returns them as tool results.
struct OutputCtx<'a> {
    format: Format,
    show_saved: bool,
    output: Option<&'a Path>,
    collect: Option<&'a mut String>,
    /// Set when a tool finishes with partial output plus a caveat (e.g.
    /// outline's parse-failure note); embedders (the MCP server) attach it
    /// to the non-zero-exit error result.
    diagnostic: Option<String>,
}

fn run(cli: &Cli) -> Result<ExitCode, ExitError> {
    let config = config::load(cli.config.as_deref()).map_err(|e| ExitError::new(1, e))?;
    let mut ctx = OutputCtx {
        format: if cli.json { Format::Json } else { cli.format },
        show_saved: config.general.show_saved && !cli.no_saved,
        output: cli.output.as_deref(),
        collect: None,
        diagnostic: None,
    };
    match &cli.command {
        Command::Outline {
            file,
            no_doc,
            no_lines,
        } => run_outline(file, *no_doc, *no_lines, &mut ctx, &config),
        Command::Symbol {
            file,
            name,
            kind,
            signature,
            compact,
            lines,
        } => run_symbol(
            file,
            name,
            kind.map(SymbolKindArg::to_symbol_kind),
            *signature,
            *compact,
            lines.as_deref(),
            &mut ctx,
            &config,
        ),
        Command::Read { file, lines } => run_read(file, lines, &mut ctx, &config),
        Command::Deps { file } => run_deps(file, &mut ctx, &config),
        Command::Exec {
            cmd,
            keep,
            head,
            tail,
        } => run_exec(cmd, keep, *head, *tail, &mut ctx, &config),
        Command::Mcp => mcp::run(cli.config.as_deref()),
    }
}

/// Deliver the final payload: `collect` captures it in memory (MCP mode);
/// `--output` writes it to a file (bypassing stdout size limits; a
/// confirmation goes to stderr); otherwise it is printed to stdout. The
/// target receives exactly the stdout bytes.
fn deliver(text: &str, ctx: &mut OutputCtx) -> Result<(), ExitError> {
    deliver_bytes(text.as_bytes(), ctx)
}

/// Byte-exact variant of [`deliver`] for compressed command output:
/// passthrough bytes reach the file/stdout targets verbatim. Only the
/// `collect` buffer degrades non-UTF-8 output lossily, because MCP JSON
/// text content must be valid UTF-8.
fn deliver_bytes(bytes: &[u8], ctx: &mut OutputCtx) -> Result<(), ExitError> {
    if let Some(buf) = ctx.collect.as_deref_mut() {
        buf.push_str(&String::from_utf8_lossy(bytes));
        return Ok(());
    }
    match ctx.output {
        Some(path) => {
            std::fs::write(path, bytes).map_err(|e| {
                ExitError::new(1, format!("failed to write {}: {e}", path.display()))
            })?;
            eprintln!("wrote {}", path.display());
        }
        None => {
            let mut stdout = std::io::stdout();
            stdout
                .write_all(bytes)
                .map_err(|e| ExitError::new(1, format!("failed to write stdout: {e}")))?;
            stdout
                .flush()
                .map_err(|e| ExitError::new(1, format!("failed to flush stdout: {e}")))?;
        }
    }
    Ok(())
}

fn run_outline(
    path: &Path,
    no_doc: bool,
    no_lines: bool,
    ctx: &mut OutputCtx,
    config: &Config,
) -> Result<ExitCode, ExitError> {
    let source = read_source(path, config.limits.max_file_bytes)?;
    let (parsed, file_tokens) = parse_and_tokens(path, &source, ctx.show_saved)?;
    let file_tokens = file_tokens.unwrap_or(0);
    let symbols = ctx_symbol::extract_symbols(&parsed);
    let show_doc = config.outline.show_doc && !no_doc;

    // Parse failures are signaled, not hidden: tree-sitter recovers, so the
    // partial symbol list is still delivered, but the JSON envelope gains a
    // `parse_error` field, text mode prints a warning on stderr, and the
    // exit code is 3 (§4.1) so agents can tell "no symbols" from "broken
    // syntax".
    let parse_errors = ctx_symbol::parse_error_count(&parsed);
    let parse_failed = parse_errors > 0;
    if parse_failed {
        // The MCP collector has no stderr channel; carry the note so the
        // server can attach it to the non-zero-exit error result.
        ctx.diagnostic = Some(format!(
            "tree-sitter reported {parse_errors} syntax error node(s); symbol list may be incomplete"
        ));
    }

    if ctx.format == Format::Json {
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
        if parse_failed {
            payload["parse_error"] = json!({
                "count": parse_errors,
                "message": "tree-sitter reported syntax errors; symbol list may be incomplete",
            });
        }
        if ctx.show_saved {
            // `tokens_after` counts the bytes actually delivered (the
            // serialized payload), not a sum of symbol slice estimates.
            let delivered_tokens = tokens(&payload.to_string());
            payload["saved"] = json!({
                "tokens_before": file_tokens,
                "tokens_after": delivered_tokens,
                "percent": saved_pct(file_tokens, delivered_tokens),
            });
        }
        deliver(&format!("{payload}\n"), ctx)?;
        return Ok(if parse_failed {
            ExitCode::from(3)
        } else {
            ExitCode::SUCCESS
        });
    }

    // [outline] fold_threshold (§7): fold the symbol list in text mode when it
    // exceeds the threshold. JSON stays complete (machine contract).
    let fold_limit = config.outline.fold_threshold;
    let folded = symbols.len() > fold_limit;
    let shown: &[Symbol] = if folded {
        &symbols[..fold_limit]
    } else {
        &symbols
    };

    let mut body = String::new();
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
            body.push_str(&format!(
                "  {:<7} {:<name_w$} {:<9}    {}\n",
                kind_alias(s.kind),
                s.name,
                loc,
                s.signature,
            ));
        }
    }
    if folded {
        body.push_str(&format!(
            "  ... [{} symbols omitted]\n",
            symbols.len() - fold_limit
        ));
    }

    // `tokens_after` measures the bytes actually delivered (the symbol list
    // render), not a sum of symbol slice estimates — nested definitions used
    // to be double-counted, inflating the estimate beyond the file itself.
    let delivered_tokens = tokens(&body);
    let saved = saved_pct(file_tokens, delivered_tokens);
    let header = if ctx.show_saved {
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
    deliver(&format!("{header}\n{body}"), ctx)?;
    if parse_failed {
        eprintln!(
            "warning: parse failed ({} error node(s)); symbol list may be incomplete",
            parse_errors
        );
    }
    Ok(if parse_failed {
        ExitCode::from(3)
    } else {
        ExitCode::SUCCESS
    })
}

#[allow(clippy::too_many_arguments)]
fn run_symbol(
    path: &Path,
    name: &str,
    kind: Option<ctx_symbol::SymbolKind>,
    signature_only: bool,
    compact: bool,
    subrange: Option<&str>,
    ctx: &mut OutputCtx,
    config: &Config,
) -> Result<ExitCode, ExitError> {
    let source = read_source(path, config.limits.max_file_bytes)?;
    let (parsed, file_tokens) = parse_and_tokens(path, &source, ctx.show_saved)?;
    let file_tokens = file_tokens.unwrap_or(0);
    let symbols = ctx_symbol::extract_symbols(&parsed);
    let symbol = symbols
        .iter()
        .filter(|s| kind.is_none_or(|k| s.kind == k))
        .find(|s| s.name == name)
        .ok_or_else(|| {
            let hint = match kind {
                Some(k) => format!(" (kind {:?})", k),
                None => String::new(),
            };
            ExitError::new(4, format!("symbol not found: {name}{hint}"))
        })?;
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
    let delivered_tokens = tokens(&slice);
    let saved = saved_pct(file_tokens, delivered_tokens);

    if ctx.format == Format::Json {
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
        if ctx.show_saved {
            payload["saved"] = json!({
                "tokens_before": file_tokens,
                "tokens_after": delivered_tokens,
                "percent": saved,
            });
        }
        deliver(&format!("{payload}\n"), ctx)?;
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
    let header = if ctx.show_saved {
        format!(
            "# {}  {}  ({} tokens, saved ~{}%)",
            symbol.name,
            loc,
            group(delivered_tokens),
            saved,
        )
    } else {
        format!("# {}  {}", symbol.name, loc)
    };
    deliver(&format!("{header}\n{slice}\n"), ctx)?;
    Ok(ExitCode::SUCCESS)
}

fn run_read(
    path: &Path,
    raw: &str,
    ctx: &mut OutputCtx,
    config: &Config,
) -> Result<ExitCode, ExitError> {
    let source = read_source(path, config.limits.max_file_bytes)?;
    // Split on `\n` keeping the endings so CRLF slices stay verbatim
    // (byte-stability plus original line-ending fidelity).
    let file_lines: Vec<&str> = source.split_inclusive('\n').collect();
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
        .map(|(start, end)| (*start, *end, file_lines[start - 1..*end].concat()))
        .collect();
    let delivered_tokens: usize = slices.iter().map(|(_, _, text)| tokens(text)).sum();
    let file_tokens = tokens(&source);
    let saved = saved_pct(file_tokens, delivered_tokens);

    if ctx.format == Format::Json {
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
        if ctx.show_saved {
            payload["saved"] = json!({
                "tokens_before": file_tokens,
                "tokens_after": delivered_tokens,
                "percent": saved,
            });
        }
        deliver(&format!("{payload}\n"), ctx)?;
        return Ok(ExitCode::SUCCESS);
    }

    let mut out = String::new();
    for (start, end, text) in &slices {
        out.push_str(&format!("# {}:{}-{}\n", path.display(), start, end));
        out.push_str(text);
        out.push('\n');
    }
    if ctx.show_saved {
        out.push_str(&format!(
            "Saved ~{}% ({} -> {} tokens)\n",
            saved,
            group(file_tokens),
            group(delivered_tokens),
        ));
    }
    deliver(&out, ctx)?;
    Ok(ExitCode::SUCCESS)
}

/// Returns the first shell metacharacter outside quotes/backslash escapes.
/// `exec` spawns argv directly instead of through a shell, so an unquoted
/// metacharacter would silently become a plain argument ("pwd && ls" runs
/// pwd with junk args) rather than a compound command.
fn first_unquoted_metachar(cmd: &str) -> Option<char> {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for ch in cmd.chars() {
        match quote {
            Some('\'') => {
                if ch == '\'' {
                    quote = None;
                }
            }
            Some('"') => {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    quote = None;
                }
            }
            _ => {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '\'' || ch == '"' {
                    quote = Some(ch);
                } else if matches!(ch, '|' | '&' | ';' | '(' | ')' | '<' | '>' | '\n') {
                    return Some(ch);
                }
            }
        }
    }
    None
}

/// Maximum wall-clock time an `exec` child may run before it is killed.
const EXEC_TIMEOUT: Duration = Duration::from_secs(30);
/// Poll granularity while watching an `exec` child.
const EXEC_TIMEOUT_POLL: Duration = Duration::from_millis(25);

/// Outcome of bounding one child's lifetime.
#[derive(Debug)]
enum ExecWait {
    Exited(std::process::ExitStatus),
    /// Deadline hit; the child was killed and reaped.
    TimedOut,
}

/// Wait for `child` until `deadline`, then kill and reap it. Std-only:
/// polls `try_wait` at `poll` intervals so a fast exit returns promptly.
fn wait_bounded(child: &mut Child, deadline: Instant, poll: Duration) -> std::io::Result<ExecWait> {
    loop {
        match child.try_wait()? {
            Some(status) => return Ok(ExecWait::Exited(status)),
            None => {
                if Instant::now() >= deadline {
                    // Kill failure is unreachable for a live child; the
                    // subsequent wait surfaces anything real.
                    let _ = child.kill();
                    child.wait()?;
                    return Ok(ExecWait::TimedOut);
                }
                std::thread::sleep(poll);
            }
        }
    }
}

fn run_exec(
    cmd: &str,
    keep: &[String],
    head: Option<usize>,
    tail: Option<usize>,
    ctx: &mut OutputCtx,
    config: &Config,
) -> Result<ExitCode, ExitError> {
    if let Some(ch) = first_unquoted_metachar(cmd) {
        return Err(ExitError::new(
            1,
            format!(
                "exec runs a single command, not a shell; found unquoted `{ch}` — wrap compound commands as sh -c \"<command>\""
            ),
        ));
    }
    let words = shell_words::split(cmd)
        .map_err(|e| ExitError::new(1, format!("invalid command line: {e}")))?;
    if words.is_empty() {
        return Err(ExitError::new(1, "empty command"));
    }
    // Build and validate compression options BEFORE spawning so an invalid
    // --keep pattern aborts without leaving a detached child running.
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
    let mut compressor =
        ctx_exec::StreamCompressor::new(&options).map_err(|e| ExitError::new(1, e.to_string()))?;
    // Spawn with piped streams: stdout is compressed incrementally (bounded
    // memory even for huge outputs) while stderr drains on a thread to avoid
    // pipe-buffer deadlocks. The merge order stays stdout-then-stderr
    // (cli-contract.md §4.4).
    let mut child = StdCommand::new(&words[0])
        .args(&words[1..])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| ExitError::new(1, format!("failed to run `{cmd}`: {e}")))?;
    let stdout_pipe = child.stdout.take().expect("stdout was configured as piped");
    let mut stderr_pipe = child.stderr.take().expect("stderr was configured as piped");

    let stderr_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = std::io::Read::read_to_end(&mut stderr_pipe, &mut buf);
        buf
    });

    // Bound the child's lifetime: the MCP stdio server is single-threaded,
    // so a hung command would deadlock every later request (and block a CLI
    // caller indefinitely). The watcher kills at the deadline; both exit and
    // kill close the pipes, releasing the reads below.
    let (status_tx, status_rx) = std::sync::mpsc::channel();
    let deadline = Instant::now() + EXEC_TIMEOUT;
    let watcher = std::thread::spawn(move || {
        let mut child = child;
        if let Ok(outcome) = wait_bounded(&mut child, deadline, EXEC_TIMEOUT_POLL) {
            let _ = status_tx.send(outcome);
        }
    });

    let mut stdout_reader = std::io::BufReader::new(stdout_pipe);
    let mut stdout_empty = true;
    let mut stdout_ends_with_nl = false;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = std::io::Read::read(&mut stdout_reader, &mut buf)
            .map_err(|e| ExitError::new(1, format!("failed to read stdout: {e}")))?;
        if n == 0 {
            break;
        }
        stdout_empty = false;
        stdout_ends_with_nl = buf[n - 1] == b'\n';
        compressor.push(&buf[..n]);
    }
    let stderr = stderr_handle.join().unwrap_or_default();
    if !stdout_empty && !stderr.is_empty() && !stdout_ends_with_nl {
        compressor.push(b"\n");
    }
    if !stderr.is_empty() {
        compressor.push(&stderr);
    }
    let result = compressor.finish();
    let stats = result.stats;
    // Over-broad keep patterns defeat compression (e.g. a pattern that is a
    // case-insensitive substring of most lines); surface it instead of
    // silently shipping near-full output. Deterministic, so byte stability
    // is unaffected.
    let ineffective_warning = stats.compression_ineffective().then(|| {
        format!(
            "keep patterns matched most of the output; compression saved only {}% ({} -> {} tokens) — check --keep patterns",
            stats.saved_percent,
            group(stats.original_tokens),
            group(stats.compressed_tokens),
        )
    });
    // The watcher resolves before the pipes close (exit or kill), so this
    // recv never outlasts the reads above.
    let outcome = status_rx
        .recv()
        .map_err(|_| ExitError::new(1, format!("failed while watching `{cmd}`")))?;
    if watcher.join().is_err() {
        return Err(ExitError::new(1, format!("failed while watching `{cmd}`")));
    }
    let code = match outcome {
        ExecWait::Exited(status) => exit_code(&status),
        ExecWait::TimedOut => {
            return Err(ExitError::new(
                1,
                format!(
                    "command `{cmd}` exceeded the {}s execution timeout and was killed",
                    EXEC_TIMEOUT.as_secs()
                ),
            ));
        }
    };

    if ctx.format == Format::Json {
        // JSON strings must be valid UTF-8, so compressed bytes degrade
        // lossily only at this boundary.
        let mut payload = json!({
            "schema_version": 1,
            "tool": "exec",
            "cmd": cmd,
            "exit_code": code,
            "compressed": String::from_utf8_lossy(&result.text),
        });
        if ctx.show_saved {
            payload["saved"] = json!({
                "tokens_before": stats.original_tokens,
                "tokens_after": stats.compressed_tokens,
                "percent": stats.saved_percent,
            });
        }
        if let Some(warning) = &ineffective_warning {
            payload["warning"] = json!(warning);
        }
        deliver(&format!("{payload}\n"), ctx)?;
        return Ok(ExitCode::from(code as u8));
    }

    // Text mode carries the compressed bytes verbatim (passthrough stays
    // byte-exact on stdout and `--output`); only JSON/MCP degrade to lossy
    // strings, where validity is required.
    let mut out = format!("$ {cmd}\n").into_bytes();
    if !result.text.is_empty() {
        out.extend_from_slice(&result.text);
        out.push(b'\n');
    }
    if ctx.show_saved {
        out.extend_from_slice(
            format!(
                "Saved ~{}% ({} -> {} tokens)\n",
                stats.saved_percent,
                group(stats.original_tokens),
                group(stats.compressed_tokens),
            )
            .as_bytes(),
        );
    }
    // Over-broad-keep notice routing: text mode keeps stdout pure machine
    // data and warns on stderr (like outline's parse-failure warning); JSON
    // carries it as a `warning` payload field; the MCP collector has no
    // stderr channel to the agent, so there it stays in-band.
    if let Some(warning) = &ineffective_warning {
        if ctx.collect.is_some() {
            out.extend_from_slice(format!("warning: {warning}\n").as_bytes());
        } else {
            eprintln!("warning: {warning}");
        }
    }
    deliver_bytes(&out, ctx)?;
    Ok(ExitCode::from(code as u8))
}

fn run_deps(path: &Path, ctx: &mut OutputCtx, config: &Config) -> Result<ExitCode, ExitError> {
    let source = read_source(path, config.limits.max_file_bytes)?;
    let (parsed, file_tokens) = parse_and_tokens(path, &source, ctx.show_saved)?;
    let file_tokens = file_tokens.unwrap_or(0);
    let imports = ctx_symbol::extract_imports(&parsed);
    let file_dir = path.parent().unwrap_or(Path::new("."));
    let resolved = deps::resolve(
        &imports,
        parsed.language.name(),
        file_dir,
        &config.paths.ignore,
    );
    let delivered_tokens: usize = resolved.iter().map(|r| tokens(&r.target)).sum();
    let saved = saved_pct(file_tokens, delivered_tokens);

    if ctx.format == Format::Json {
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
        if ctx.show_saved {
            payload["saved"] = json!({
                "tokens_before": file_tokens,
                "tokens_after": delivered_tokens,
                "percent": saved,
            });
        }
        deliver(&format!("{payload}\n"), ctx)?;
        return Ok(ExitCode::SUCCESS);
    }

    let header = if ctx.show_saved {
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
    let mut out = format!("{header}\n");
    if !resolved.is_empty() {
        let name_w = resolved
            .iter()
            .map(|r| r.target.len())
            .max()
            .unwrap_or(0)
            .max(10);
        for r in &resolved {
            out.push_str(&format!(
                "  {:<9} {:<name_w$}    L:{}\n",
                dep_kind_name(r.kind),
                r.target,
                r.line,
            ));
        }
    }
    deliver(&out, ctx)?;
    Ok(ExitCode::SUCCESS)
}

fn dep_kind_name(kind: deps::DepKind) -> &'static str {
    match kind {
        deps::DepKind::Local => "local",
        deps::DepKind::External => "external",
        deps::DepKind::Ignored => "ignored",
        deps::DepKind::Unresolved => "unresolved",
    }
}

/// Child exit code with the conventional signal encoding: 128 + signal when
/// the child was killed by one (otherwise the code is `None` and 1 is used).
fn exit_code(status: &std::process::ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            return 128 + sig;
        }
    }
    status.code().unwrap_or(1)
}

/// Read an input file behind two guardrails: the path must be a regular
/// file (`fs::metadata` follows symlinks, so linked sources stay valid) and
/// its size must not exceed `[limits] max_file_bytes`. Without the type
/// check a FIFO blocks forever or a char device like `/dev/zero` streams
/// until memory dies; without the size check every command reads and
/// tokenizes arbitrarily large files.
fn read_source(path: &Path, max_file_bytes: usize) -> Result<String, ExitError> {
    let meta = std::fs::metadata(path)
        .map_err(|e| ExitError::new(1, format!("failed to read {}: {e}", path.display())))?;
    if !meta.is_file() {
        return Err(ExitError::new(
            1,
            format!(
                "{}: not a regular file ({})",
                path.display(),
                non_regular_kind(&meta)
            ),
        ));
    }
    let len = meta.len();
    if len > max_file_bytes as u64 {
        return Err(ExitError::new(
            1,
            format!(
                "{}: file is {} bytes, exceeding max_file_bytes limit of {} bytes",
                path.display(),
                len,
                max_file_bytes
            ),
        ));
    }
    std::fs::read_to_string(path)
        .map_err(|e| ExitError::new(1, format!("failed to read {}: {e}", path.display())))
}

/// Human-readable reason why a stat-ed path is not a regular file.
fn non_regular_kind(meta: &std::fs::Metadata) -> &'static str {
    if meta.is_dir() {
        return "directory";
    }
    special_file_kind(&meta.file_type())
}

#[cfg(unix)]
fn special_file_kind(ft: &std::fs::FileType) -> &'static str {
    use std::os::unix::fs::FileTypeExt;
    if ft.is_fifo() {
        "FIFO"
    } else if ft.is_char_device() {
        "character device"
    } else if ft.is_block_device() {
        "block device"
    } else if ft.is_socket() {
        "socket"
    } else {
        "special file"
    }
}

#[cfg(not(unix))]
fn special_file_kind(_ft: &std::fs::FileType) -> &'static str {
    "special file"
}

/// Parse the source and, in parallel, compute its cl100k token count when
/// `show_saved` is set. Both are pure functions of `source`, so results are
/// deterministic regardless of thread scheduling (byte stability unaffected).
/// Overlap pays off on large files: parsing a 4 MB file takes ~250 ms and
/// tokenizing it ~320 ms — parallel, they cost ~320 ms instead of ~570 ms.
fn parse_and_tokens(
    path: &Path,
    source: &str,
    show_saved: bool,
) -> Result<(ParsedSource, Option<usize>), ExitError> {
    if !show_saved {
        return Ok((parse_or(path, source)?, None));
    }
    std::thread::scope(|scope| {
        let parse = scope.spawn(|| parse_or(path, source));
        let count = scope.spawn(|| tokens(source));
        let parsed = parse
            .join()
            .map_err(|_| ExitError::new(1, "parse thread failed"))??;
        let count = count
            .join()
            .map_err(|_| ExitError::new(1, "token thread failed"))?;
        Ok((parsed, Some(count)))
    })
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
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let (start, end) = clamp_open_ends(ranges, lines.len())[0];
    if start > lines.len() {
        return Err(format!(
            "line range {start}-{end} out of bounds (symbol has {} lines)",
            lines.len()
        ));
    }
    Ok(lines[start - 1..end].concat())
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
    if !no_doc && let Some(doc) = &symbol.doc_comment {
        entry["doc_comment"] = json!(doc);
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
        SymbolKind::Module => "module",
        SymbolKind::Heading => "heading",
        SymbolKind::Rule => "rule",
        SymbolKind::Element => "element",
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
        SymbolKind::Module => "mod",
        SymbolKind::Heading => "head",
        SymbolKind::Rule => "rule",
        SymbolKind::Element => "elem",
    }
}

/// Real token count via the cl100k_base BPE tokenizer (cli-contract.md §8).
/// Deterministic, so byte stability is unaffected; the bundled encoding is
/// parsed once and cached.
fn tokens(text: &str) -> usize {
    static BPE: std::sync::OnceLock<tiktoken_rs::CoreBPE> = std::sync::OnceLock::new();
    let bpe = BPE.get_or_init(|| {
        tiktoken_rs::cl100k_base().expect("bundled cl100k_base encoding is corrupt")
    });
    bpe.encode_with_special_tokens(text).len()
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
        if i > 0 && (bytes.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

#[cfg(test)]
mod exec_timeout_tests {
    use super::*;

    #[test]
    fn fast_child_reports_exit_well_before_deadline() {
        let started = Instant::now();
        let mut child = StdCommand::new("true").spawn().expect("spawn true");
        let outcome = wait_bounded(
            &mut child,
            started + Duration::from_millis(150),
            Duration::from_millis(5),
        )
        .expect("wait succeeds");
        assert!(matches!(outcome, ExecWait::Exited(_)), "{outcome:?}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn hung_child_is_killed_promptly_at_deadline() {
        let started = Instant::now();
        let mut child = StdCommand::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep");
        let outcome = wait_bounded(
            &mut child,
            started + Duration::from_millis(150),
            Duration::from_millis(10),
        )
        .expect("wait succeeds");
        assert!(matches!(outcome, ExecWait::TimedOut), "{outcome:?}");
        // Kill must be prompt: well under the 30s the child would have run.
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "kill took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn timeout_error_message_mentions_timeout() {
        let err = ExitError::new(
            1,
            format!(
                "command `sleep 60` exceeded the {}s execution timeout and was killed",
                EXEC_TIMEOUT.as_secs()
            ),
        );
        assert!(err.message.contains("timeout"), "{}", err.message);
        assert_eq!(err.code, 1u8);
    }
}
