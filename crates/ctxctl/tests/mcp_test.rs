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
