//! Integration tests for the `ctxctl mcp` stdio server: framing over a real
//! process, tool round-trips, clean exit at EOF, and untrusted-input
//! hardening (workspace pinning, exec child timeout).

use std::io::{BufRead, BufReader, Write};
use std::process::ExitStatus;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

/// Unique suffix for per-server workspaces; tests run in parallel.
static NEXT_WORKSPACE: AtomicU32 = AtomicU32::new(0);

struct Server {
    child: Child,
    stdin: Option<ChildStdin>,
    reader: BufReader<ChildStdout>,
    /// Directory the server was launched in: it pins this as its workspace
    /// root, so fixtures must be written here and referenced relatively.
    workspace: std::path::PathBuf,
    /// Isolated XDG config dir handed to the server; removed in `shutdown`.
    xdg_config: std::path::PathBuf,
}

impl Server {
    fn spawn() -> Self {
        let workspace = std::env::temp_dir().join(format!(
            "ctxctl-mcp-ws-{}-{}",
            std::process::id(),
            NEXT_WORKSPACE.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&workspace).expect("create server workspace");
        let xdg_config = xdg_isolation_dir();
        let mut child = Command::new(env!("CARGO_BIN_EXE_ctxctl"))
            .arg("mcp")
            .current_dir(&workspace)
            // Hermetic like the CLI suite's `base()`: the server loads the
            // same config precedence chain, so an ambient XDG config must
            // not leak into tool behavior.
            .env("XDG_CONFIG_HOME", &xdg_config)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn ctxctl mcp");
        let stdin = child.stdin.take().expect("stdin piped");
        let reader = BufReader::new(child.stdout.take().expect("stdout piped"));
        Self {
            child,
            stdin: Some(stdin),
            reader,
            workspace,
            xdg_config,
        }
    }

    fn send(&mut self, line: &str) {
        let stdin = self.stdin.as_mut().expect("stdin open");
        writeln!(stdin, "{line}").expect("write request");
        stdin.flush().expect("flush request");
    }

    /// Send one request and read its response line.
    fn request(&mut self, line: &str) -> serde_json::Value {
        self.send(line);
        let mut response = String::new();
        self.reader.read_line(&mut response).expect("read response");
        serde_json::from_str(&response).expect("response is valid JSON")
    }

    /// Close stdin (EOF) and collect the exit status, cleaning up the
    /// server workspace best-effort.
    fn shutdown(mut self) -> ExitStatus {
        self.stdin = None; // drops ChildStdin -> EOF on the server side
        let status = self.child.wait().expect("wait after EOF");
        std::fs::remove_dir_all(&self.workspace).ok();
        std::fs::remove_dir_all(&self.xdg_config).ok();
        status
    }

    /// Write a fixture inside the pinned workspace; returns the relative
    /// name to pass as a `file` argument.
    fn fixture(&self, name: &str, body: &str) -> String {
        std::fs::write(self.workspace.join(name), body).expect("write fixture");
        name.to_string()
    }

    /// Send one `tools/call` request and return its response.
    fn call(&mut self, id: u32, name: &str, arguments: &serde_json::Value) -> serde_json::Value {
        let line = format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"{name}","arguments":{}}}}}"#,
            arguments
        );
        self.request(&line)
    }
}

/// Fresh, empty XDG config dir so the spawned server cannot pick up an
/// ambient user config (the test host may legitimately run a tuned one).
/// Unique per server so parallel spawns cannot race; removed in `shutdown`.
fn xdg_isolation_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ctxctl-mcp-xdg-{}-{}",
        std::process::id(),
        NEXT_WORKSPACE.fetch_add(1, Ordering::Relaxed),
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create xdg isolation dir");
    dir
}

fn result_text(response: &serde_json::Value) -> &str {
    response["result"]["content"][0]["text"]
        .as_str()
        .expect("text content")
}

#[test]
fn stdio_round_trip_and_clean_exit() {
    let mut server = Server::spawn();

    let init = server.request(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test"}}}"#,
    );
    assert_eq!(init["result"]["serverInfo"]["name"], "ctxctl");

    // Notification: no response may be produced for it, so send it without
    // reading; the next response read must belong to the later request id.
    server.send(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
    let tools = server.request(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
    assert_eq!(tools["id"], 2);
    assert_eq!(tools["result"]["tools"].as_array().map(Vec::len), Some(5));

    let source = server.fixture("round-trip.txt", "alpha\nbeta\n");
    let call = format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"ctxctl_read","arguments":{{"file":{},"lines":"2-2"}}}}}}"#,
        serde_json::to_string(&source).expect("path json"),
    );
    let answer = server.request(&call);
    assert_eq!(answer["id"], 3);
    let text = answer["result"]["content"][0]["text"]
        .as_str()
        .expect("text");
    assert!(text.contains("beta"), "{text}");
    assert!(!text.contains("alpha"), "{text}");

    let status = server.shutdown();
    assert!(status.success(), "server should exit 0 at EOF");
}

#[test]
fn outline_parse_failure_becomes_is_error_with_detail() {
    let mut server = Server::spawn();
    let source = server.fixture("broken.rs", "fn broken( {\n    let x = ;\n");
    let answer = server.call(10, "ctxctl_outline", &serde_json::json!({ "file": source }));
    server.shutdown();
    assert_eq!(answer["id"], 10);
    // Exit 3 and the parse-error note must reach the client as an isError
    // result; before CTX-0032 they were dropped (silent incomplete list).
    assert_eq!(
        answer["result"]["isError"], true,
        "parse failure must be surfaced: {answer}"
    );
    let text = result_text(&answer);
    assert!(text.contains("exit code 3"), "{text}");
    assert!(text.contains("syntax error"), "{text}");
}

#[test]
fn mistyped_argument_names_the_key() {
    let mut server = Server::spawn();
    let answer = server.call(
        11,
        "ctxctl_exec",
        &serde_json::json!({ "cmd": "echo hi", "head": "3" }),
    );
    server.shutdown();
    // A string `head` used to fail as_u64 and silently fall back to the
    // config default; it must be rejected naming the bad argument.
    assert_eq!(answer["result"]["isError"], true, "{answer}");
    let text = result_text(&answer);
    assert!(text.contains("head"), "{text}");
    assert!(text.contains("integer"), "{text}");
}

#[test]
fn non_string_keep_item_is_rejected() {
    let mut server = Server::spawn();
    let answer = server.call(
        12,
        "ctxctl_exec",
        &serde_json::json!({ "cmd": "echo hi", "keep": ["error", 5] }),
    );
    server.shutdown();
    assert_eq!(answer["result"]["isError"], true, "{answer}");
    let text = result_text(&answer);
    assert!(text.contains("keep"), "{text}");
}

#[test]
fn unknown_argument_key_is_rejected() {
    let mut server = Server::spawn();
    let source = server.fixture("plain.txt", "alpha\nbeta\n");
    let answer = server.call(
        13,
        "ctxctl_read",
        &serde_json::json!({
            "file": source,
            "lines": "1-1",
            "bogus": true,
        }),
    );
    server.shutdown();
    assert_eq!(answer["result"]["isError"], true, "{answer}");
    let text = result_text(&answer);
    assert!(text.contains("bogus"), "{text}");
}

// --- Untrusted-input hardening (CTX-0033) ------------------------------------

#[test]
fn parent_traversal_file_arg_is_rejected() {
    let mut server = Server::spawn();
    // `../` climbs out of the pinned workspace; the file may or may not
    // exist, but the rule must fire before any read is attempted.
    let outside = server
        .workspace
        .parent()
        .expect("workspace has a parent")
        .join("outside.txt");
    std::fs::write(&outside, "secret\n").expect("write outside fixture");
    let answer = server.call(
        20,
        "ctxctl_read",
        &serde_json::json!({ "file": "../outside.txt", "lines": "1-1" }),
    );
    server.shutdown();
    std::fs::remove_file(outside).ok();
    assert_eq!(answer["result"]["isError"], true, "{answer}");
    let text = result_text(&answer);
    assert!(text.contains("../outside.txt"), "{text}");
    assert!(text.contains("path escapes workspace root"), "{text}");
    assert!(!text.contains("secret"), "content must not leak: {text}");
}

#[test]
fn absolute_path_file_arg_is_rejected() {
    let mut server = Server::spawn();
    let answer = server.call(
        21,
        "ctxctl_read",
        &serde_json::json!({ "file": "/etc/passwd", "lines": "1-1" }),
    );
    server.shutdown();
    assert_eq!(answer["result"]["isError"], true, "{answer}");
    let text = result_text(&answer);
    assert!(text.contains("/etc/passwd"), "{text}");
    assert!(text.contains("path escapes workspace root"), "{text}");
    assert!(
        !text.to_lowercase().contains("root:x:"),
        "content must not leak: {text}"
    );
}

#[test]
fn nested_relative_file_arg_still_works() {
    // Pinning must not break legitimate in-workspace access.
    let mut server = Server::spawn();
    std::fs::create_dir_all(server.workspace.join("sub/dir")).expect("mkdir");
    let source = server.fixture("sub/dir/nested.txt", "deep line\n");
    let answer = server.call(
        22,
        "ctxctl_read",
        &serde_json::json!({ "file": source, "lines": "1-1" }),
    );
    server.shutdown();
    // Success results omit `isError`; it must certainly not be set.
    assert_ne!(
        answer["result"]["isError"].as_bool(),
        Some(true),
        "{answer}"
    );
    let text = result_text(&answer);
    assert!(text.contains("deep line"), "{text}");
}

#[test]
fn hanging_exec_child_is_killed_at_timeout() {
    let started = std::time::Instant::now();
    let mut server = Server::spawn();
    // Far beyond the 30s bound; without the timeout this call only returns
    // (successfully) when the child exits on its own.
    let answer = server.call(23, "ctxctl_exec", &serde_json::json!({ "cmd": "sleep 60" }));
    let elapsed = started.elapsed();
    server.shutdown();
    assert_eq!(
        answer["result"]["isError"], true,
        "timeout must surface as isError after {elapsed:?}: {answer}"
    );
    let text = result_text(&answer);
    assert!(text.contains("timeout"), "{text}");
    assert!(
        elapsed < std::time::Duration::from_secs(35),
        "server must kill the child instead of waiting for it; took {elapsed:?}"
    );
}

// --- Tool matrix (CTX-0040) ---------------------------------------------------
//
// Every tool must complete a full stdio JSON-RPC round-trip. Fixtures are
// embedded from the package's tests/fixtures and written into the server's
// pinned workspace, then referenced relatively: CTX-0033 rejects anything
// outside that workspace, including machine-local absolute paths.

/// Repo fixture embedded at compile time, written into each server workspace.
const SAMPLE_RS: &str = include_str!("fixtures/sample.rs");

/// Repo fixture embedded at compile time, written into each server workspace.
const DEPS_RS: &str = include_str!("fixtures/deps.rs");

/// A deterministic >collapse-threshold output with one critical line, as a
/// single quoted `printf` command (metacharacter validation passes it).
fn noisy_printf_cmd() -> String {
    let mut body = String::new();
    for i in 1..=12 {
        body.push_str(&format!("step {i}\\n"));
    }
    body.push_str("error: boom\\n");
    for i in 13..=25 {
        body.push_str(&format!("step {i}\\n"));
    }
    format!("printf '{body}'")
}

#[test]
fn tool_matrix_round_trips_every_tool_over_stdio() {
    let mut server = Server::spawn();
    let sample = server.fixture("sample.rs", SAMPLE_RS);
    let deps_fixture = server.fixture("deps.rs", DEPS_RS);

    let init = server.request(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test"}}}"#,
    );
    assert_eq!(init["result"]["serverInfo"]["name"], "ctxctl");

    // outline
    let outline = server.call(10, "ctxctl_outline", &serde_json::json!({ "file": sample }));
    assert_eq!(outline["id"], 10);
    assert_eq!(outline["result"]["isError"], json_null(), "{outline}");
    let text = result_text(&outline);
    assert!(text.contains("4 symbols"), "{text}");
    assert!(text.contains("pub fn add(a: i32, b: i32) -> i32"), "{text}");
    assert!(text.contains("saved ~"), "{text}");

    // symbol
    let symbol = server.call(
        11,
        "ctxctl_symbol",
        &serde_json::json!({ "file": sample, "name": "add" }),
    );
    assert_eq!(symbol["result"]["isError"], json_null(), "{symbol}");
    let text = result_text(&symbol);
    assert!(text.starts_with("# add"), "no locator: {text}");
    assert!(
        text.contains("pub fn add(a: i32, b: i32) -> i32 {"),
        "{text}"
    );
    assert!(text.contains("a + b"), "{text}");
    // Byte stability holds per connection: an identical call returns an
    // identical payload.
    let again = server.call(
        12,
        "ctxctl_symbol",
        &serde_json::json!({ "file": sample, "name": "add" }),
    );
    assert_eq!(
        result_text(&symbol),
        result_text(&again),
        "identical tool calls must be byte-stable"
    );

    // read
    let read = server.call(
        13,
        "ctxctl_read",
        &serde_json::json!({ "file": sample, "lines": "4-4" }),
    );
    assert_eq!(read["result"]["isError"], json_null(), "{read}");
    let text = result_text(&read);
    assert!(
        text.contains("pub fn add(a: i32, b: i32) -> i32 {"),
        "{text}"
    );
    assert!(!text.contains("ANSWER"), "range too wide: {text}");

    // deps
    let deps = server.call(
        14,
        "ctxctl_deps",
        &serde_json::json!({ "file": deps_fixture }),
    );
    assert_eq!(deps["result"]["isError"], json_null(), "{deps}");
    let text = result_text(&deps);
    assert!(text.contains("5 imports"), "{text}");
    assert!(text.contains("serde::Deserialize"), "{text}");
    assert!(text.contains("local"), "{text}");

    // exec
    let cmd = noisy_printf_cmd();
    let exec = server.call(15, "ctxctl_exec", &serde_json::json!({ "cmd": cmd }));
    assert_eq!(exec["result"]["isError"], json_null(), "{exec}");
    let text = result_text(&exec);
    assert!(text.starts_with("$ "), "command not echoed: {text}");
    assert!(text.contains("error: boom"), "critical line lost: {text}");
    assert!(text.contains("lines omitted"), "no fold marker: {text}");
    assert!(text.contains("Saved ~"), "no savings line: {text}");

    let status = server.shutdown();
    assert!(status.success(), "server should exit 0 at EOF");
}

fn json_null() -> serde_json::Value {
    serde_json::Value::Null
}

#[test]
fn nonexistent_file_becomes_is_error_naming_the_problem() {
    // Relative name inside the pinned workspace so the read (not the
    // CTX-0033 escape guard) is what fails on the missing file.
    let mut server = Server::spawn();
    let answer = server.call(
        20,
        "ctxctl_read",
        &serde_json::json!({ "file": "missing.rs", "lines": "1-2" }),
    );
    let deps_answer = server.call(
        21,
        "ctxctl_deps",
        &serde_json::json!({ "file": "missing.rs" }),
    );
    server.shutdown();
    assert_eq!(answer["id"], 20);
    assert_eq!(answer["result"]["isError"], true, "{answer}");
    let text = result_text(&answer);
    assert!(text.contains("failed to read"), "{text}");
    assert_eq!(deps_answer["result"]["isError"], true, "{deps_answer}");
}

#[test]
fn mistyped_lines_argument_names_the_key() {
    // An integer `lines` used to be silently unusable; it must be rejected
    // naming both the key and the expected type.
    let mut server = Server::spawn();
    let sample = server.fixture("sample.rs", SAMPLE_RS);
    let answer = server.call(
        22,
        "ctxctl_read",
        &serde_json::json!({ "file": sample, "lines": 4 }),
    );
    server.shutdown();
    assert_eq!(answer["result"]["isError"], true, "{answer}");
    let text = result_text(&answer);
    assert!(text.contains("lines"), "{text}");
    assert!(text.contains("string"), "{text}");
}

#[test]
fn unknown_tool_name_becomes_is_error_result() {
    let mut server = Server::spawn();
    let answer = server.call(23, "ctxctl_nope", &serde_json::json!({ "whatever": true }));
    server.shutdown();
    assert_eq!(answer["id"], 23);
    assert_eq!(answer["result"]["isError"], true, "{answer}");
    let text = result_text(&answer);
    assert!(text.contains("unknown tool"), "{text}");
    assert!(text.contains("ctxctl_nope"), "{text}");
}
