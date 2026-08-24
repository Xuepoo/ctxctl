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
}

impl Server {
    fn spawn() -> Self {
        let workspace = std::env::temp_dir().join(format!(
            "ctxctl-mcp-ws-{}-{}",
            std::process::id(),
            NEXT_WORKSPACE.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&workspace).expect("create server workspace");
        let mut child = Command::new(env!("CARGO_BIN_EXE_ctxctl"))
            .arg("mcp")
            .current_dir(&workspace)
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
