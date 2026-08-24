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
            "Raw 1-based line ranges from the original source (no AST), e.g. \"100-150,200-210\". Open-ended ranges like 40- are allowed.",
            json!({
                "file": string_desc("Path of the source file"),
                "lines": string_desc("Comma-separated inclusive ranges, e.g. 100-150,200-210"),
            }),
            vec!["file", "lines"],
        ),
        (
            "ctxctl_deps",
            "Import/module dependency graph of a file; each import is classified local, external, or ignored.",
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

fn arg_schema(tool: &str) -> Option<&'static [(&'static str, ArgKind)]> {
    match tool {
        "ctxctl_outline" => Some(OUTLINE_ARGS),
        "ctxctl_symbol" => Some(SYMBOL_ARGS),
        "ctxctl_read" => Some(READ_ARGS),
        "ctxctl_deps" => Some(DEPS_ARGS),
        "ctxctl_exec" => Some(EXEC_ARGS),
        _ => None,
    }
}

/// Check arguments against the advertised schema before dispatch: unknown
/// keys and mistyped values are rejected naming the offending argument
/// instead of being coerced or defaulted. Required-key presence and
/// emptiness stay with the handlers (`require_str`). Explicit `null`
/// counts as absent — clients omit optional fields that way.
fn validate_arguments(tool: &str, args: &Value) -> Result<(), String> {
    let Some(schema) = arg_schema(tool) else {
        return Ok(()); // unknown tools are rejected by `run_tool`
    };
    let object = args
        .as_object()
        .ok_or_else(|| "arguments must be an object".to_string())?;
    for key in object.keys() {
        if !schema.iter().any(|(known, _)| *known == key.as_str()) {
            return Err(format!("unknown argument: {key}"));
        }
    }
    for (key, kind) in schema {
        let Some(value) = object.get(*key) else {
            continue;
        };
        if !value.is_null() && !kind.accepts(value) {
            return Err(format!("argument `{key}` must be {}", kind.label()));
        }
    }
    Ok(())
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
    let args = arguments
        .filter(|value| !value.is_null())
        .cloned()
        .unwrap_or_else(|| json!({}));
    validate_arguments(name, &args)?;
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
            let file = require_path(root, args, "file")?;
            crate::run_outline(&file, flag(args, "no_doc"), flag(args, "no_lines"), ctx, config)
        }
        "ctxctl_symbol" => {
            let file = require_path(root, args, "file")?;
            let symbol = require_str(args, "name")?;
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
        }
        "ctxctl_read" => {
            let file = require_path(root, args, "file")?;
            let lines = require_str(args, "lines")?;
            crate::run_read(&file, lines, ctx, config)
        }
        "ctxctl_deps" => {
            let file = require_path(root, args, "file")?;
            crate::run_deps(&file, ctx, config)
        }
        "ctxctl_exec" => {
            let cmd = require_str(args, "cmd")?.to_string();
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
        other => return Err(format!("unknown tool: {other}")),
    }
    .map_err(|e| e.message)?;
    Ok(exit_status(&code))
}

/// Fetch a required file argument and confine it to `root`. The MCP surface
/// serves remote agents, so `file` values are untrusted input.
fn require_path(root: &Path, args: &Value, key: &str) -> Result<PathBuf, String> {
    pinned_path(root, key, require_str(args, key)?)
}

/// Resolve one tool-provided path against the workspace root.
///
/// Rejected outright: absolute paths, any `..` that would climb above the
/// root, and existing targets whose symlink resolution lands outside the
/// root. Everything else is returned joined under `root` (not canonicalized,
/// so handlers keep their own not-found diagnostics).
fn pinned_path(root: &Path, key: &str, raw: &str) -> Result<PathBuf, String> {
    let escape = |detail: &str| {
        format!("invalid argument `{key}`: {raw}: {detail}path escapes workspace root")
    };
    let candidate = Path::new(raw);
    if candidate.is_absolute() {
        return Err(escape("absolute paths are not allowed; "));
    }
    let mut normalized = PathBuf::new();
    for component in candidate.components() {
        match component {
            std::path::Component::Normal(part) => normalized.push(part),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    return Err(escape(""));
                }
            }
            // Prefix/RootDir cannot occur in a relative Unix path; reject
            // defensively instead of silently stripping them.
            _ => return Err(escape("unsupported absolute-like component; ")),
        }
    }
    let joined = root.join(normalized);
    // Symlink hardening: an in-root link must not point out of the tree.
    // (Lexical containment above already rules out textual escapes; this
    // catches links to existing outside targets.)
    let base = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    if let Ok(resolved) = joined.canonicalize()
        && !resolved.starts_with(&base)
    {
        return Err(escape("resolved symlink target is outside the workspace; "));
    }
    Ok(joined)
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
