//! Optional MCP stdio adapter (`ctxctl mcp`).
//!
//! Exposes the five CLI commands as MCP tools so agents that speak the
//! Model Context Protocol get first-class `outline` / `symbol` / `read` /
//! `deps` / `exec` entries without shell plumbing. The CLI stays the
//! canonical interface: this module is a thin translation layer that calls
//! the very same handlers and returns their rendered output as tool-result
//! text.
//!
//! Framing is newline-delimited JSON-RPC 2.0 (one message per line, UTF-8),
//! per the MCP stdio transport. Responses are deterministic — a pure
//! function of the request and the loaded config — so byte stability holds
//! on this path too.

use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::{Format, OutputCtx, SymbolKindArg, config::Config};

/// Latest MCP protocol version this server speaks. Sent verbatim in the
/// `initialize` result regardless of the client's offer (spec-compliant
/// downgrade negotiation).
const PROTOCOL_VERSION: &str = "2025-03-26";

/// Run the stdio server until EOF. Config is loaded once up front, exactly
/// like the CLI path (same precedence rules). The working directory at
/// launch is pinned as the workspace root: every file-bearing tool argument
/// is resolved against it and rejected if it escapes (the MCP surface
/// serves remote agents, so its inputs are untrusted — the interactive CLI
/// keeps unrestricted paths).
pub fn run(config_path: Option<&Path>) -> Result<ExitCode, crate::ExitError> {
    let config = crate::config::load(config_path).map_err(|e| crate::ExitError::new(1, e))?;
    let root = std::env::current_dir()
        .map_err(|e| crate::ExitError::new(1, format!("cannot determine workspace root: {e}")))?;
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| crate::ExitError::new(1, format!("stdin read failed: {e}")))?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => handle(&msg, &config, &root),
            Err(e) => Some(error_response(
                &Value::Null,
                -32700,
                format!("parse error: {e}"),
            )),
        };
        if let Some(response) = response {
            writeln!(stdout, "{response}")
                .map_err(|e| crate::ExitError::new(1, format!("stdout write failed: {e}")))?;
            stdout
                .flush()
                .map_err(|e| crate::ExitError::new(1, format!("stdout flush failed: {e}")))?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Handle one incoming message. Returns `None` for notifications (no `id`)
/// and messages without a method — they get no response. `root` is the
/// pinned workspace root every file argument is confined to.
fn handle(msg: &Value, config: &Config, root: &Path) -> Option<Value> {
    // Notifications carry no id and get no response.
    let id = msg.get("id");
    if id.is_none() || id.is_some_and(Value::is_null) {
        return None;
    }
    let id = id.expect("checked above");
    // A request (id present) without a usable method must be answered,
    // or the client would wait forever.
    let Some(method) = msg.get("method").and_then(Value::as_str) else {
        return Some(error_response(
            id,
            -32600,
            "invalid request: missing or non-string method".to_string(),
        ));
    };
    Some(match method {
        "initialize" => success(
            id,
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": {
                    "name": "ctxctl",
                    "version": env!("CARGO_PKG_VERSION"),
                },
            }),
        ),
        "ping" => success(id, json!({})),
        "tools/list" => success(id, json!({ "tools": tool_definitions() })),
        "tools/call" => {
            let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            match call_tool(name, params.get("arguments"), config, root) {
                Ok(text) => success(
                    id,
                    json!({ "content": [ { "type": "text", "text": text } ] }),
                ),
                Err(message) => success(
                    id,
                    json!({
                        "content": [ { "type": "text", "text": message } ],
                        "isError": true,
                    }),
                ),
            }
        }
        other => error_response(id, -32601, format!("method not found: {other}")),
    })
}

fn success(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_response(id: &Value, code: i64, message: String) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
}

// --- Tool table -------------------------------------------------------------

fn tool_definitions() -> Value {
    let defs = [
        (
            "ctxctl_outline",
            "Symbol outline of a source file: every definition with kind, line range, and signature, plus token-savings stats. Read this before opening a file.",
            json!({
                "file": string_desc("Path of the source file"),
                "no_doc": opt_bool("Omit doc comments"),
                "no_lines": opt_bool("Omit line numbers"),
            }),
            vec!["file"],
        ),
        (
            "ctxctl_symbol",
            "Original source slice of one symbol by exact name. compact=true returns a re-parseable signature + fold-marker view; lines takes a 1-based sub-range like 3-10.",
            json!({
                "file": string_desc("Path of the source file"),
                "name": string_desc("Exact symbol name"),
                "kind": json!({
                    "type": "string",
                    "enum": ["class","struct","enum","interface","function","method","module","const","var","trait","type","heading","rule","element"],
                    "description": "Restrict the match to a symbol kind"
                }),
                "signature": opt_bool("Return the signature only"),
                "compact": opt_bool("AST-pruned view: signature + fold marker"),
                "lines": opt_string("Sub-range within the symbol, e.g. 3-10"),
            }),
            vec!["file", "name"],
        ),
        (
            "ctxctl_read",
            "Raw 1-based line ranges from the original source (no AST). Omit `lines` (or pass an empty string) to receive the whole file.",
            json!({
                "file": string_desc("Path of the source file"),
                "lines": opt_string("Comma-separated inclusive ranges, e.g. \"100-150,200-210\". Formats: \"N\", \"N-M\", \"N-\" (open-ended to end of file). Omitted or empty = whole file."),
            }),
            vec!["file"],
        ),
        (
            "ctxctl_deps",
            "Import/module dependency graph of a file; each import is classified local, external, ignored, or unresolved.",
            json!({ "file": string_desc("Path of the source file") }),
            vec!["file"],
        ),
        (
            "ctxctl_exec",
            "Run a command and return only the signal: kept error/warning lines, head/tail summary, omitted middle folded into markers. Exit code is prefixed when non-zero. Far cheaper than raw output.",
            json!({
                "cmd": string_desc("Command line to run, e.g. \"cargo test -- --list\""),
                "keep": json!({
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Extra keep regexes (rg syntax), appended to the defaults"
                }),
                "head": opt_int("Override head summary lines"),
                "tail": opt_int("Override tail summary lines"),
            }),
            vec!["cmd"],
        ),
    ]
    .into_iter()
    .map(|(name, description, properties, required)| {
        json!({
            "name": name,
            "description": description,
            "inputSchema": {
                "type": "object",
                "properties": properties,
                "required": required,
                "additionalProperties": false,
            },
        })
    })
    .collect::<Vec<_>>();
    json!(defs)
}

fn string_desc(description: &str) -> Value {
    json!({ "type": "string", "description": description })
}

fn opt_string(description: &str) -> Value {
    json!({ "type": "string", "description": description })
}

fn opt_bool(description: &str) -> Value {
    json!({ "type": "boolean", "description": description })
}

fn opt_int(description: &str) -> Value {
    json!({ "type": "integer", "description": description })
}

// --- Argument validation -----------------------------------------------------

/// Expected shape of one tool argument, mirroring the advertised
/// inputSchema so mismatches are rejected instead of silently coerced.
#[derive(Clone, Copy)]
enum ArgKind {
    Str,
    Bool,
    Int,
    StrList,
}

impl ArgKind {
    fn label(self) -> &'static str {
        match self {
            Self::Str => "a string",
            Self::Bool => "a boolean",
            Self::Int => "an integer",
            Self::StrList => "an array of strings",
        }
    }

    fn accepts(self, value: &Value) -> bool {
        match self {
            Self::Str => value.is_string(),
            Self::Bool => value.is_boolean(),
            // Integers only: a string or float `head` must not fall back to
            // the config default unnoticed.
            Self::Int => value.is_u64(),
            Self::StrList => value
                .as_array()
                .is_some_and(|items| items.iter().all(Value::is_string)),
        }
    }
}

const OUTLINE_ARGS: &[(&str, ArgKind)] = &[
    ("file", ArgKind::Str),
    ("no_doc", ArgKind::Bool),
    ("no_lines", ArgKind::Bool),
];
const SYMBOL_ARGS: &[(&str, ArgKind)] = &[
    ("file", ArgKind::Str),
    ("name", ArgKind::Str),
    ("kind", ArgKind::Str),
    ("signature", ArgKind::Bool),
    ("compact", ArgKind::Bool),
    ("lines", ArgKind::Str),
];
const READ_ARGS: &[(&str, ArgKind)] = &[("file", ArgKind::Str), ("lines", ArgKind::Str)];
const DEPS_ARGS: &[(&str, ArgKind)] = &[("file", ArgKind::Str)];
const EXEC_ARGS: &[(&str, ArgKind)] = &[
    ("cmd", ArgKind::Str),
    ("keep", ArgKind::StrList),
    ("head", ArgKind::Int),
    ("tail", ArgKind::Int),
];

/// The tool registry: every exposed tool with its argument table. Declared
/// alphabetically so every consumer that iterates it (`tools/list` order is
/// defined separately; error enumerations and usage hints) emits the same
/// deterministic order.
const TOOL_TABLE: &[(&str, &[(&str, ArgKind)])] = &[
    ("ctxctl_deps", DEPS_ARGS),
    ("ctxctl_exec", EXEC_ARGS),
    ("ctxctl_outline", OUTLINE_ARGS),
    ("ctxctl_read", READ_ARGS),
    ("ctxctl_symbol", SYMBOL_ARGS),
];

fn arg_schema(tool: &str) -> Option<&'static [(&'static str, ArgKind)]> {
    TOOL_TABLE
        .iter()
        .find(|(name, _)| *name == tool)
        .map(|(_, schema)| *schema)
}

/// Comma-separated list of registered tool names in deterministic
/// (alphabetical) order, for unknown-tool errors.
fn available_tools() -> String {
    TOOL_TABLE
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Check arguments against the advertised schema before dispatch: unknown
/// keys are rejected listing the valid ones, mistyped values are rejected
/// naming the argument and expected type. Benign coercion: a JSON number
/// for a string-typed argument becomes its decimal string form (`lines:
/// 100` → `"100"`), matching how agents actually call the tools. Required-
/// key presence and emptiness stay with the handlers (`require_str`).
/// Explicit `null` counts as absent — clients omit optional fields that way.
fn validate_arguments(tool: &str, args: &mut Value) -> Result<(), String> {
    let Some(schema) = arg_schema(tool) else {
        return Ok(()); // unknown tools are rejected by `run_tool`
    };
    if !args.is_object() {
        return Err("arguments must be an object".to_string());
    }
    let keys: Vec<String> = args
        .as_object()
        .expect("checked above")
        .keys()
        .cloned()
        .collect();
    for key in &keys {
        if !schema.iter().any(|(known, _)| known == key) {
            let valid = schema
                .iter()
                .map(|(known, _)| *known)
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!(
                "unknown argument `{key}`; valid arguments: {valid}"
            ));
        }
    }
    for (key, kind) in schema {
        let Some(value) = args.get_mut(*key) else {
            continue;
        };
        if value.is_null() {
            continue;
        }
        match kind {
            ArgKind::Str if value.is_number() => {
                *value = Value::String(value.as_number().expect("is_number").to_string());
            }
            _ if !kind.accepts(value) => {
                return Err(format!(
                    "argument `{key}` must be {}, got {}",
                    kind.label(),
                    json_type_name(value)
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Human-readable JSON type name for "got X" diagnostics.
fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

// --- Tool dispatch ----------------------------------------------------------

/// Execute one `tools/call`. Errors become `isError` results (never JSON-RPC
/// errors): a failing tool call is still a valid conversation event for the
/// agent.
fn call_tool(
    name: &str,
    arguments: Option<&Value>,
    config: &Config,
    root: &Path,
) -> Result<String, String> {
    let mut args = arguments
        .filter(|value| !value.is_null())
        .cloned()
        .unwrap_or_else(|| json!({}));
    validate_arguments(name, &mut args)?;
    let mut out = String::new();
    let (status, diagnostic) = {
        let mut ctx = OutputCtx {
            format: Format::Text,
            show_saved: config.general.show_saved,
            output: None,
            collect: Some(&mut out),
            diagnostic: None,
        };
        let status = run_tool(name, &args, &mut ctx, config, root)?;
        (status, ctx.diagnostic.clone())
    };
    if out.is_empty() {
        return Err("tool produced no output".to_string());
    }
    // The CLI conveys failure through the process exit status; MCP has no
    // channel for it, so any non-zero status becomes an isError result that
    // names the tool and code, carries the diagnostic (e.g. outline's
    // parse-error note), and keeps the partial payload below it — remote
    // agents can react without parsing rendered output.
    if let Some(code) = status {
        let mut message = format!("tool {name} failed with exit code {code}");
        if let Some(detail) = diagnostic {
            message.push_str(": ");
            message.push_str(&detail);
        }
        message.push('\n');
        message.push_str(&out);
        return Err(message);
    }
    Ok(out)
}

/// Numeric status behind an `ExitCode` (`None` = success). `ExitCode::to_u8`
/// is still unstable, so match against reconstructed candidates; every code
/// the CLI produces comes from a `u8`.
fn exit_status(code: &ExitCode) -> Option<u8> {
    (1..=u8::MAX).find(|&candidate| ExitCode::from(candidate) == *code)
}

/// Dispatch one tool invocation and render its payload into `ctx`, returning
/// its exit status (`None` on success). Non-zero codes mean partial results:
/// parse failures exit 3 with whatever tree-sitter recovered, child commands
/// propagate their exit code through `run_exec`.
fn run_tool(
    name: &str,
    args: &Value,
    ctx: &mut OutputCtx<'_>,
    config: &Config,
    root: &Path,
) -> Result<Option<u8>, String> {
    let code = match name {
        "ctxctl_outline" => {
            let file = require_file(name, root, args)?;
            crate::run_outline(&file, flag(args, "no_doc"), flag(args, "no_lines"), ctx, config)
        }
        "ctxctl_symbol" => {
            let file = require_file(name, root, args)?;
            let symbol = require_str(args, "name").map_err(|e| usage_hinted(name, e))?;
            let kind = match args.get("kind").and_then(Value::as_str) {
                Some(raw) => Some(parse_kind(raw).ok_or_else(|| {
                    format!(
                        "unknown kind `{raw}` (expected class|struct|enum|interface|function|method|module|const|var|trait|type|heading|rule|element)"
                    )
                })?),
                None => None,
            };
            crate::run_symbol(
                &file,
                symbol,
                kind.map(SymbolKindArg::to_symbol_kind),
                flag(args, "signature"),
                flag(args, "compact"),
                args.get("lines").and_then(Value::as_str),
                ctx,
                config,
            )
            .map_err(|e| with_self_healing_symbols(&file, symbol, config, e))
        }
        "ctxctl_read" => {
            let file = require_file(name, root, args)?;
            // `lines` mirrors the CLI contract: omitted or empty means the
            // whole file (an open-ended range achieves exactly that).
            let lines = args
                .get("lines")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .unwrap_or("1-");
            crate::run_read(&file, lines, ctx, config)
        }
        "ctxctl_deps" => {
            let file = require_file(name, root, args)?;
            crate::run_deps(&file, ctx, config)
        }
        "ctxctl_exec" => {
            let cmd = require_str(args, "cmd")
                .map_err(|e| usage_hinted(name, e))?
                .to_string();
            let keep: Vec<String> = args
                .get("keep")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            let head = args.get("head").and_then(Value::as_u64).map(|v| v as usize);
            let tail = args.get("tail").and_then(Value::as_u64).map(|v| v as usize);
            crate::run_exec(&cmd, &keep, head, tail, ctx, config)
        }
        other => {
            return Err(format!(
                "unknown tool `{other}`; available: {}",
                available_tools()
            ))
        }
    }
    .map_err(|e| e.message)?;
    Ok(exit_status(&code))
}

/// Fetch the required `file` argument (with usage hint on absence) and
/// confine it to `root`. The MCP surface serves remote agents, so `file`
/// values are untrusted input.
fn require_file(tool: &str, root: &Path, args: &Value) -> Result<PathBuf, String> {
    let raw = require_str(args, "file").map_err(|e| usage_hinted(tool, e))?;
    pinned_path(root, "file", raw)
}

/// Append the tool's argument-table example to a missing-argument error so
/// agents can self-correct without a round-trip to the schema.
fn usage_hinted(tool: &str, err: String) -> String {
    format!("{err}\n{}", usage_hint(tool))
}

/// Build `expected: {"key": <placeholder>, …}` from the tool's argument
/// table, so the hint can never drift from the schema.
fn usage_hint(tool: &str) -> String {
    let Some(schema) = arg_schema(tool) else {
        return String::new();
    };
    let members = schema
        .iter()
        .map(|(key, kind)| format!("\"{key}\": {}", placeholder(key, *kind)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("expected: {{{members}}}")
}

/// JSON-shaped placeholder for one argument in a usage hint.
fn placeholder(key: &str, kind: ArgKind) -> &'static str {
    match kind {
        ArgKind::Str => match key {
            "file" => "\"<workspace-relative or absolute path under workspace>\"",
            "name" => "\"<exact symbol name>\"",
            "cmd" => "\"<command line>\"",
            _ => "\"<string>\"",
        },
        ArgKind::Bool => "<boolean>",
        ArgKind::Int => "<integer>",
        ArgKind::StrList => "[\"<string>\"]",
    }
}

/// Resolve one tool-provided path against the workspace root.
///
/// Absolute paths are legal iff they stay inside the workspace after
/// normalization (lexical `.`/`..` resolution; existing targets are also
/// symlink-resolved). Relative paths resolve against `root` as before. Any
/// escape — lexical climb above the root, or an existing target whose
/// symlink resolution lands outside — is rejected. Accepted paths are
/// returned as given (normalized), not canonicalized, so handlers keep
/// their own not-found diagnostics.
fn pinned_path(root: &Path, key: &str, raw: &str) -> Result<PathBuf, String> {
    // Inputs are passed explicitly rather than captured so static analysis
    // credits every parameter read (code-scanning alert #2).
    let escape = |key: &str, raw: &str, detail: &str| {
        format!("invalid argument `{key}`: {raw}: {detail}path escapes workspace root")
    };
    let candidate = Path::new(raw);
    let base = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    if candidate.is_absolute() {
        // CTX-0047: absolute paths are legal iff they stay inside the
        // workspace. Normalize lexically first (`.` dropped, `..` applied
        // without touching the filesystem), then decide containment:
        // existing targets are symlink-resolved, nonexistent ones fall back
        // to the lexical form.
        let Some(normalized) = lexically_normalized_absolute(candidate) else {
            return Err(escape(key, raw, ""));
        };
        let inside = match normalized.canonicalize() {
            Ok(resolved) => resolved.starts_with(&base),
            Err(_) => normalized.starts_with(root),
        };
        if !inside {
            return Err(escape(key, raw, ""));
        }
        return Ok(normalized);
    }
    let mut normalized = PathBuf::new();
    for component in candidate.components() {
        match component {
            std::path::Component::Normal(part) => normalized.push(part),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    return Err(escape(key, raw, ""));
                }
            }
            // Prefix/RootDir cannot occur in a relative Unix path; reject
            // defensively instead of silently stripping them.
            _ => return Err(escape(key, raw, "unsupported absolute-like component; ")),
        }
    }
    let joined = root.join(normalized);
    // Symlink hardening: an in-root link must not point out of the tree.
    // (Lexical containment above already rules out textual escapes; this
    // catches links to existing outside targets.)
    if let Ok(resolved) = joined.canonicalize()
        && !resolved.starts_with(&base)
    {
        return Err(escape(
            key,
            raw,
            "resolved symlink target is outside the workspace; ",
        ));
    }
    Ok(joined)
}

/// Lexically normalize an absolute path without filesystem access: `.` is
/// dropped, `..` pops the previous component (a `..` at the root stays
/// there, POSIX-style). Returns `None` for paths with no anchor.
fn lexically_normalized_absolute(path: &Path) -> Option<PathBuf> {
    let mut anchor = PathBuf::new();
    let mut parts: Vec<std::ffi::OsString> = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::Prefix(prefix) => anchor.push(prefix.as_os_str()),
            std::path::Component::RootDir => anchor.push(std::path::MAIN_SEPARATOR.to_string()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                parts.pop();
            }
            std::path::Component::Normal(part) => parts.push(part.to_os_string()),
        }
    }
    if anchor.as_os_str().is_empty() {
        return None;
    }
    Some(parts.into_iter().fold(anchor, |acc, part| acc.join(part)))
}

/// Maximum candidates returned by [`ranked_suggestions`].
const SUGGESTION_LIMIT: usize = 5;
/// Maximum lines rendered by [`mini_outline`].
const MINI_OUTLINE_LIMIT: usize = 20;
/// Levenshtein ceiling for typo-tier suggestions (early-exit beyond this).
const TYPO_DISTANCE_MAX: usize = 2;

/// Analyze `file` once so a miss can carry suggestions and a mini-outline.
/// Best-effort — any read or parse failure yields `None` and the bare error
/// stays untouched. Honors the configured file-size limit; only runs on the
/// error path, never on successful lookups.
fn analyze_file(file: &Path, config: &Config) -> Option<Vec<ctx_symbol::Symbol>> {
    let source = crate::read_source(file, config.limits.max_file_bytes).ok()?;
    ctx_symbol::outline(&source, file).ok()
}

/// Rank look-alike symbol names for a failed exact-match lookup. Each
/// candidate scores in its best tier — case-insensitive prefix match, then
/// case-insensitive substring match, then bounded Levenshtein distance
/// <= 2 — ties are broken alphabetically, duplicates removed, capped at
/// [`SUGGESTION_LIMIT`]. Deterministic by construction.
fn ranked_suggestions(symbols: &[ctx_symbol::Symbol], query: &str) -> Vec<String> {
    let query = query.to_lowercase();
    let mut scored: Vec<(u8, String)> = Vec::new();
    for symbol in symbols {
        let lower = symbol.name.to_lowercase();
        let tier = if lower.starts_with(&query) {
            0
        } else if lower.contains(&query) {
            1
        } else if bounded_levenshtein(&lower, &query, TYPO_DISTANCE_MAX).is_some() {
            2
        } else {
            continue;
        };
        if !scored.iter().any(|(_, name)| name == &symbol.name) {
            scored.push((tier, symbol.name.clone()));
        }
    }
    scored.sort_unstable(); // tier ascending, then name ascending
    scored.truncate(SUGGESTION_LIMIT);
    scored.into_iter().map(|(_, name)| name).collect()
}

/// Bounded Levenshtein edit distance: `Some(distance)` when it is within
/// `max`, `None` as soon as the distance provably exceeds `max`. Time
/// O(len_a * len_b), space O(len_b); each DP row aborts once its minimum
/// exceeds `max`, and length differences beyond `max` skip entirely.
fn bounded_levenshtein(a: &str, b: &str, max: usize) -> Option<usize> {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > max {
        return None;
    }
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current: Vec<usize> = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        current[0] = i + 1;
        let mut row_min = current[0];
        for (j, cb) in b.iter().enumerate() {
            let substitution_cost = usize::from(ca != cb);
            let value = (previous[j] + substitution_cost)
                .min(previous[j + 1] + 1)
                .min(current[j] + 1);
            current[j + 1] = value;
            row_min = row_min.min(value);
        }
        if row_min > max {
            return None;
        }
        std::mem::swap(&mut previous, &mut current);
    }
    let distance = previous[b.len()];
    (distance <= max).then_some(distance)
}

/// Render a capped outline of the file's top-level symbols (`kind name`
/// lines sorted by byte position) so a missed lookup can recover without
/// another round-trip. Nested definitions are skipped by walking the
/// position-sorted symbols and dropping anything that starts inside an
/// already-accepted span. Returns `None` when there is nothing to show;
/// truncation is announced with `... and N more`.
fn mini_outline(symbols: &[ctx_symbol::Symbol]) -> Option<String> {
    let mut ordered: Vec<&ctx_symbol::Symbol> = symbols.iter().collect();
    ordered.sort_by_key(|symbol| symbol.byte_range.start);
    let mut top_level: Vec<&ctx_symbol::Symbol> = Vec::new();
    let mut span_end = 0usize;
    for symbol in ordered {
        if symbol.byte_range.start >= span_end {
            top_level.push(symbol);
            span_end = symbol.byte_range.end;
        }
    }
    let shown = MINI_OUTLINE_LIMIT.min(top_level.len());
    if shown == 0 {
        return None;
    }
    let mut block: Vec<String> = top_level[..shown]
        .iter()
        .map(|symbol| format!("{} {}", crate::kind_name(symbol.kind), symbol.name))
        .collect();
    if top_level.len() > shown {
        block.push(format!("... and {} more", top_level.len() - shown));
    }
    Some(block.join("\n"))
}

/// Enrich `symbol not found:` errors for self-healing: a ranked suggestions
/// line plus a blank-line-separated mini-outline of the file's top-level
/// symbols. The first line is preserved verbatim; all other errors pass
/// through untouched. Output depends only on file content and query, so the
/// message is byte-stable.
fn with_self_healing_symbols(
    file: &Path,
    query: &str,
    config: &Config,
    mut err: crate::ExitError,
) -> crate::ExitError {
    if !err.message.starts_with("symbol not found: ") {
        return err;
    }
    let Some(symbols) = analyze_file(file, config) else {
        return err;
    };
    let suggestions = ranked_suggestions(&symbols, query);
    if !suggestions.is_empty() {
        err.message.push_str("\nsuggestions: ");
        err.message.push_str(&suggestions.join(", "));
    }
    if let Some(outline) = mini_outline(&symbols) {
        err.message.push_str("\n\n");
        err.message.push_str(&outline);
    }
    err
}

fn require_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("missing or empty argument: {key}"))
}

fn flag(args: &Value, key: &str) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn parse_kind(raw: &str) -> Option<SymbolKindArg> {
    let kinds = [
        ("class", SymbolKindArg::Class),
        ("struct", SymbolKindArg::Struct),
        ("enum", SymbolKindArg::Enum),
        ("interface", SymbolKindArg::Interface),
        ("function", SymbolKindArg::Function),
        ("method", SymbolKindArg::Method),
        ("module", SymbolKindArg::Module),
        ("const", SymbolKindArg::Const),
        ("var", SymbolKindArg::Variable),
        ("trait", SymbolKindArg::Trait),
        ("type", SymbolKindArg::Type),
        ("heading", SymbolKindArg::Heading),
        ("rule", SymbolKindArg::Rule),
        ("element", SymbolKindArg::Element),
    ];
    kinds
        .iter()
        .find(|(name, _)| *name == raw)
        .map(|(_, kind)| *kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn test_config() -> Config {
        crate::config::load(None).expect("default config loads")
    }

    /// Unique per-test workspace root (tests run in parallel).
    fn test_root() -> PathBuf {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "ctxctl-mcp-unit-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&dir).expect("create workspace");
        dir
    }

    /// Write a fixture inside `root`; returns the relative name.
    fn fixture(root: &Path, name: &str, body: &str) -> String {
        let path = root.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create fixture dirs");
        }
        let mut file = std::fs::File::create(&path).expect("create temp file");
        file.write_all(body.as_bytes()).expect("write temp file");
        name.to_string()
    }

    fn request(method: &str, params: Value) -> Value {
        json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params })
    }

    #[test]
    fn initialize_negotiates_and_identifies() {
        let msg = request(
            "initialize",
            json!({ "protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {} }),
        );
        let response = handle(&msg, &test_config(), &test_root()).expect("a response");
        assert_eq!(response["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(response["result"]["serverInfo"]["name"], "ctxctl");
        assert_eq!(response["id"], 1);
    }

    #[test]
    fn notifications_get_no_response() {
        let msg = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        assert!(handle(&msg, &test_config(), &test_root()).is_none());
    }

    #[test]
    fn request_without_method_gets_invalid_request() {
        // An id-carrying message without a method must be answered, or a
        // client would block forever waiting for its response.
        let msg = json!({ "jsonrpc": "2.0", "id": 9 });
        let response = handle(&msg, &test_config(), &test_root()).expect("a response");
        assert_eq!(response["error"]["code"], -32600);
        assert_eq!(response["id"], 9);
    }

    #[test]
    fn unknown_method_is_method_not_found() {
        let msg = request("no/such/method", json!({}));
        let response = handle(&msg, &test_config(), &test_root()).expect("a response");
        assert_eq!(response["error"]["code"], -32601);
    }

    #[test]
    fn tools_list_exposes_five_tools() {
        let msg = request("tools/list", json!({}));
        let response = handle(&msg, &test_config(), &test_root()).expect("a response");
        let tools = response["result"]["tools"].as_array().expect("tools array");
        let names: Vec<&str> = tools
            .iter()
            .map(|t| t["name"].as_str().expect("tool name"))
            .collect();
        assert_eq!(
            names,
            [
                "ctxctl_outline",
                "ctxctl_symbol",
                "ctxctl_read",
                "ctxctl_deps",
                "ctxctl_exec"
            ]
        );
        for tool in tools {
            assert_eq!(
                tool["inputSchema"]["type"], "object",
                "{} schema",
                tool["name"]
            );
        }
    }

    #[test]
    fn read_tool_returns_slice_text() {
        let root = test_root();
        let name = fixture(&root, "read.rs", "fn one() {}\nfn two() {}\n");
        let msg = request(
            "tools/call",
            json!({ "name": "ctxctl_read", "arguments": {
                "file": name, "lines": "1-1" } }),
        );
        let response = handle(&msg, &test_config(), &root).expect("a response");
        let text = response["result"]["content"][0]["text"]
            .as_str()
            .expect("text content");
        assert!(text.contains("fn one()"), "{text}");
        assert!(!text.contains("fn two"), "{text}");
        assert!(response["result"]["isError"].is_null());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn missing_file_becomes_is_error_result() {
        // Relative and in-workspace, so the failure is the handler's
        // not-found path rather than the workspace pin.
        let msg = request(
            "tools/call",
            json!({ "name": "ctxctl_read", "arguments": {
                "file": "nonexistent/ctxctl/test.rs", "lines": "1-2" } }),
        );
        let response = handle(&msg, &test_config(), &test_root()).expect("a response");
        assert_eq!(response["result"]["isError"], true);
        let text = response["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or("");
        assert!(!text.is_empty());
    }

    #[test]
    fn unknown_tool_is_is_error_result() {
        let msg = request("tools/call", json!({ "name": "nope" }));
        let response = handle(&msg, &test_config(), &test_root()).expect("a response");
        assert_eq!(response["result"]["isError"], true);
    }

    #[test]
    fn exec_nonzero_exit_becomes_is_error_result() {
        let msg = request(
            "tools/call",
            json!({ "name": "ctxctl_exec", "arguments": { "cmd": "false" } }),
        );
        let response = handle(&msg, &test_config(), &test_root()).expect("a response");
        assert_eq!(response["result"]["isError"], true);
        let text = response["result"]["content"][0]["text"]
            .as_str()
            .expect("text content");
        // The text names the tool and exit code; the compressed output
        // follows so no signal is lost.
        assert!(
            text.starts_with("tool ctxctl_exec failed with exit code 1\n"),
            "{text}"
        );
    }

    #[test]
    fn absolute_file_arg_is_rejected_before_any_read() {
        let root = test_root();
        for raw in ["/etc/passwd", "/"] {
            let msg = request(
                "tools/call",
                json!({ "name": "ctxctl_read", "arguments": { "file": raw, "lines": "1-1" } }),
            );
            let response = handle(&msg, &test_config(), &root).expect("a response");
            assert_eq!(response["result"]["isError"], true, "{raw}");
            let text = response["result"]["content"][0]["text"]
                .as_str()
                .expect("text content");
            assert!(text.contains("path escapes workspace root"), "{text}");
        }
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn traversal_file_arg_is_rejected() {
        let root = test_root();
        for raw in ["../outside.txt", "sub/../../outside.txt", ".."] {
            let msg = request(
                "tools/call",
                json!({ "name": "ctxctl_deps", "arguments": { "file": raw } }),
            );
            let response = handle(&msg, &test_config(), &root).expect("a response");
            assert_eq!(response["result"]["isError"], true, "{raw}");
            let text = response["result"]["content"][0]["text"]
                .as_str()
                .expect("text content");
            assert!(text.contains(raw), "{text}");
            assert!(text.contains("path escapes workspace root"), "{text}");
        }
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn pinned_path_error_strings_are_byte_stable() {
        // Characterization pins: these exact bytes are API surface (MCP
        // clients may match on them), so the error builder must never
        // reword them.
        let root = test_root();
        assert_eq!(
            pinned_path(&root, "file", "../outside.txt"),
            Err("invalid argument `file`: ../outside.txt: path escapes workspace root".to_string())
        );
        // Absolute escapes (existing target or not) share one message.
        assert_eq!(
            pinned_path(&root, "file", "/etc/passwd"),
            Err("invalid argument `file`: /etc/passwd: path escapes workspace root".to_string())
        );
        assert_eq!(
            pinned_path(&root, "file", "/definitely/not/existing/ctxctl/probe.txt"),
            Err(
                "invalid argument `file`: /definitely/not/existing/ctxctl/probe.txt: path escapes workspace root"
                    .to_string()
            )
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn pinned_path_accepts_absolute_paths_inside_root() {
        let root = test_root();
        let name = fixture(&root, "inner.txt", "content\n");
        // Existing in-root target, spelled absolutely: accepted as-is.
        let absolute = root.join(&name);
        assert_eq!(
            pinned_path(&root, "file", &absolute.display().to_string()),
            Ok(absolute),
        );
        // Nonexistent in-root target: lexical containment decides.
        let missing = root.join("not-yet.txt");
        assert_eq!(
            pinned_path(&root, "file", &missing.display().to_string()),
            Ok(missing),
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn pinned_path_normalizes_absolute_parent_walks() {
        // `..` inside an absolute path is resolved lexically; the result
        // stays accepted when it lands back inside the root.
        let root = test_root();
        fixture(&root, "inner.txt", "content\n");
        let walked = format!("{}/sub/../inner.txt", root.display());
        assert_eq!(
            pinned_path(&root, "file", &walked),
            Ok(root.join("inner.txt")),
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn validate_arguments_coerces_numbers_for_str_args() {
        for raw in [json!(100), json!(4)] {
            let mut args = json!({ "file": "x.rs", "lines": raw });
            validate_arguments("ctxctl_read", &mut args).expect("number coerces");
            let coerced = args["lines"].as_str().expect("string after coercion");
            assert_eq!(coerced, raw.as_i64().expect("int").to_string());
        }
    }

    #[test]
    fn validate_arguments_still_rejects_non_numeric_mistypes() {
        let mut bool_lines = json!({ "lines": true });
        assert_eq!(
            validate_arguments("ctxctl_read", &mut bool_lines),
            Err("argument `lines` must be a string, got boolean".to_string())
        );
        let mut object_file = json!({ "file": {} });
        assert_eq!(
            validate_arguments("ctxctl_deps", &mut object_file),
            Err("argument `file` must be a string, got object".to_string())
        );
        let mut string_head = json!({ "cmd": "echo", "head": "3" });
        assert_eq!(
            validate_arguments("ctxctl_exec", &mut string_head),
            Err("argument `head` must be an integer, got string".to_string())
        );
        let mut mixed_keep = json!({ "cmd": "echo", "keep": ["error", 5] });
        assert_eq!(
            validate_arguments("ctxctl_exec", &mut mixed_keep),
            Err("argument `keep` must be an array of strings, got array".to_string())
        );
        // Explicit null stays "absent".
        let mut null_lines = json!({ "file": "x.rs", "lines": null });
        validate_arguments("ctxctl_read", &mut null_lines).expect("null is absent");
    }

    #[test]
    fn unknown_argument_error_lists_valid_keys() {
        let mut args = json!({ "file": "x.rs", "encoding": "utf8" });
        assert_eq!(
            validate_arguments("ctxctl_read", &mut args),
            Err("unknown argument `encoding`; valid arguments: file, lines".to_string())
        );
    }

    #[test]
    fn usage_hints_are_built_from_the_argument_table() {
        assert_eq!(
            usage_hint("ctxctl_read"),
            "expected: {\"file\": \"<workspace-relative or absolute path under workspace>\", \"lines\": \"<string>\"}"
        );
        assert_eq!(
            usage_hint("ctxctl_outline"),
            "expected: {\"file\": \"<workspace-relative or absolute path under workspace>\", \"no_doc\": <boolean>, \"no_lines\": <boolean>}"
        );
        assert_eq!(
            usage_hint("ctxctl_exec"),
            "expected: {\"cmd\": \"<command line>\", \"keep\": [\"<string>\"], \"head\": <integer>, \"tail\": <integer>}"
        );
    }

    #[test]
    fn available_tools_lists_registry_in_deterministic_order() {
        assert_eq!(
            available_tools(),
            "ctxctl_deps, ctxctl_exec, ctxctl_outline, ctxctl_read, ctxctl_symbol"
        );
    }

    #[test]
    fn ranked_suggestions_score_tier_then_name_and_cap_at_five() {
        let source = "struct NodeHandle;\nstruct NodeState;\nstruct NodeThingy;\n\
                      struct WrapNode;\nstruct MyNodeState;\nfn unrelated() {}\n";
        let symbols =
            ctx_symbol::outline(source, std::path::Path::new("nodes.rs")).expect("fixture parses");
        // Prefix tier before substring tier regardless of alphabetical order.
        assert_eq!(
            ranked_suggestions(&symbols, "nodestate"),
            vec!["NodeState", "MyNodeState"]
        );
        // Substring-only hit.
        assert_eq!(ranked_suggestions(&symbols, "handl"), vec!["NodeHandle"]);
        // Typo candidate via bounded Levenshtein without any textual match.
        assert_eq!(
            ranked_suggestions(&symbols, "nodethingy1"),
            vec!["NodeThingy"]
        );
        // Beyond distance 2: silent.
        assert!(ranked_suggestions(&symbols, "zzz").is_empty());
    }

    #[test]
    fn mini_outline_lists_top_level_symbols_and_caps_at_twenty() {
        let mut source = String::new();
        for i in 0..25 {
            source.push_str(&format!("struct Top{i:02};\n"));
        }
        source.push_str("impl Wrapper {\n    fn inner_method(&self) {}\n}\n");
        let symbols =
            ctx_symbol::outline(&source, std::path::Path::new("tops.rs")).expect("fixture parses");
        let outline = mini_outline(&symbols).expect("non-empty outline");
        let lines: Vec<&str> = outline.lines().collect();
        assert_eq!(lines.len(), 21);
        assert_eq!(lines[0], "struct Top00");
        assert_eq!(lines[19], "struct Top19");
        assert_eq!(lines[20], "... and 6 more");
        assert!(
            !outline.contains("inner_method"),
            "nested symbols stay hidden"
        );
    }

    #[cfg(windows)]
    #[test]
    fn pinned_path_rejects_drive_relative_component() {
        // Prefix components cannot occur in relative Unix paths, so this
        // builder branch only fires on Windows drive-relative input.
        let root = test_root();
        assert_eq!(
            pinned_path(&root, "file", "C:outside.txt"),
            Err(
                "invalid argument `file`: C:outside.txt: unsupported absolute-like component; path escapes workspace root"
                    .to_string()
            )
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_escape_is_rejected() {
        let root = test_root();
        let outside =
            std::env::temp_dir().join(format!("ctxctl-mcp-unit-outside-{}", std::process::id()));
        std::fs::write(&outside, "secret\n").expect("write outside file");
        std::os::unix::fs::symlink(&outside, root.join("link.txt")).expect("create link");
        let msg = request(
            "tools/call",
            json!({ "name": "ctxctl_read", "arguments": { "file": "link.txt", "lines": "1-1" } }),
        );
        let response = handle(&msg, &test_config(), &root).expect("a response");
        std::fs::remove_file(&outside).ok();
        std::fs::remove_dir_all(&root).ok();
        assert_eq!(response["result"]["isError"], true);
        let text = response["result"]["content"][0]["text"]
            .as_str()
            .expect("text content");
        assert!(
            text.contains("resolved symlink target is outside the workspace"),
            "{text}"
        );
        assert!(!text.contains("secret"), "content must not leak: {text}");
    }
}
