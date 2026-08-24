//! Integration tests for the `ctxctl mcp` stdio server: framing over a real
//! process, tool round-trips, and clean exit at EOF.

use std::io::{BufRead, BufReader, Write};
use std::process::ExitStatus;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Server {
    child: Child,
    stdin: Option<ChildStdin>,
    reader: BufReader<ChildStdout>,
}

impl Server {
    fn spawn() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_ctxctl"))
            .arg("mcp")
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

    /// Close stdin (EOF) and collect the exit status.
    fn shutdown(mut self) -> ExitStatus {
        self.stdin = None; // drops ChildStdin -> EOF on the server side
        self.child.wait().expect("wait after EOF")
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

/// Write a unique temp fixture; returns its path. The pid goes before the
/// name so the real extension stays terminal (language detection uses it).
fn temp_file(name: &str, body: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("ctxctl-mcp-it-{}-{name}", std::process::id()));
    std::fs::write(&path, body).expect("write fixture");
    path
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

    let source = std::env::temp_dir().join(format!("ctxctl-mcp-it-{}.txt", std::process::id()));
    std::fs::write(&source, "alpha\nbeta\n").expect("write fixture");
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
    std::fs::remove_file(source).ok();

    let status = server.shutdown();
    assert!(status.success(), "server should exit 0 at EOF");
}

#[test]
fn outline_parse_failure_becomes_is_error_with_detail() {
    let mut server = Server::spawn();
    let source = temp_file("broken.rs", "fn broken( {\n    let x = ;\n");
    let answer = server.call(
        10,
        "ctxctl_outline",
        &serde_json::json!({ "file": source.display().to_string() }),
    );
    server.shutdown();
    std::fs::remove_file(source).ok();
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
    let source = temp_file("plain.txt", "alpha\nbeta\n");
    let answer = server.call(
        13,
        "ctxctl_read",
        &serde_json::json!({
            "file": source.display().to_string(),
            "lines": "1-1",
            "bogus": true,
        }),
    );
    server.shutdown();
    std::fs::remove_file(source).ok();
    assert_eq!(answer["result"]["isError"], true, "{answer}");
    let text = result_text(&answer);
    assert!(text.contains("bogus"), "{text}");
}
