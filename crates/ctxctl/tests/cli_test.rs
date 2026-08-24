//! Integration tests for the ctxctl binary: four commands, JSON contract,
//! exit codes (cli-contract.md §5), config resolution (§6), and byte stability.

use serde_json::Value;
use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_ctxctl");
const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sample.rs");
const PY_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sample.py");
const GO_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sample.go");
const JS_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sample.js");
const JAVA_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sample.java");
const DEPS_RS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/deps.rs");
const DEPS_PY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/deps.py");
const DEPS_GO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/deps.go");
const DEPS_TS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/deps.ts");
const DEPS_C: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/deps.c");
const DEPS_CPP: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/deps.cpp");
const DEPS_CS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/deps.cs");
const DEPS_RB: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/deps.rb");
const DEPS_LUA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/deps.lua");
const C_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sample.c");
const CPP_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sample.cpp");
const CS_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sample.cs");
const RB_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sample.rb");
const LUA_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sample.lua");

fn base() -> Command {
    let mut cmd = Command::new(BIN);
    cmd.env("XDG_CONFIG_HOME", tmp_dir("xdg-isolation"));
    cmd
}

fn run(args: &[&str]) -> Output {
    base().args(args).output().expect("run ctxctl")
}

fn run_in(dir: &std::path::Path, args: &[&str]) -> Output {
    base()
        .current_dir(dir)
        .args(args)
        .output()
        .expect("run ctxctl")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// Exec text output minus the `$ cmd` echo line.
fn body(output: &Output) -> String {
    stdout(output)
        .lines()
        .skip(1)
        .collect::<Vec<_>>()
        .join("\n")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn tmp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ctxctl-it-{name}"));
    // tmp_dir uses a fixed path per test name; clear any stale state from a
    // previous run so re-running the suite is deterministic (e.g. a leftover
    // symlink would otherwise fail with "File exists").
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn outline_text_reports_symbols_and_savings() {
    let output = run(&["outline", FIXTURE]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("4 symbols"), "unexpected: {text}");
    assert!(text.contains("saved ~"), "missing savings: {text}");
    assert!(text.contains("L:4-6"), "no line numbers: {text}");
    assert!(
        text.contains("pub fn add(a: i32, b: i32) -> i32"),
        "no signature: {text}"
    );
    assert!(text.contains("struct  Point"), "unexpected: {text}");
    assert!(text.contains("const   ANSWER"), "unexpected: {text}");
}

#[test]
fn outline_json_contract() {
    let output = run(&["outline", "--json", FIXTURE]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let value: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["tool"], "outline");
    assert_eq!(value["path"], FIXTURE);
    assert_eq!(value["language"], "rust");
    assert_eq!(value["symbols"].as_array().map(Vec::len), Some(4));
    let first = &value["symbols"][0];
    assert!(first.get("name").is_some());
    assert!(first.get("kind").is_some());
    assert!(first.get("start_line").is_some());
    assert!(first.get("end_line").is_some());
    assert!(first.get("signature").is_some());
    assert!(
        first.get("byte_range").is_none(),
        "byte_range must not leak into the contract"
    );
    assert!(
        first.get("doc_comment").is_some(),
        "doc_comment should be present by default"
    );
    // tokens_after counts the actual payload bytes; on a tiny fixture the
    // JSON envelope can rival the file, so only sanity bounds apply here.
    assert!(value["saved"]["tokens_before"].as_u64().unwrap() > 0);
    assert!(value["saved"]["tokens_after"].as_u64().unwrap() > 0);
    assert!(value["saved"]["percent"].as_u64().unwrap() <= 100);
}

#[test]
fn outline_saved_reflects_actual_output_size() {
    let dir = tmp_dir("outline-large");
    let path = dir.join("big.rs");
    let mut src = String::new();
    for i in 0..20 {
        src.push_str(&format!("pub fn func_{i}(a: i32, b: i32) -> i32 {{\n"));
        for j in 0..25 {
            src.push_str(&format!("    let v{j} = a * {j} + b - {i};\n"));
        }
        src.push_str("    a + b\n}\n");
    }
    std::fs::write(&path, &src).expect("write fixture");
    let output = run(&["outline", "--json", path.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let value: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert!(value["saved"]["percent"].as_u64().unwrap() > 0);
    assert!(
        value["saved"]["tokens_before"].as_u64().unwrap()
            > value["saved"]["tokens_after"].as_u64().unwrap()
    );
}

#[test]
fn token_accounting_uses_cl100k_bpe() {
    // Pin the real cl100k_base count for a CJK-heavy fixture: 57 tokens
    // (bytes/4 would report 44). Contract: cli-contract.md §8.
    let dir = tmp_dir("token-bpe");
    let path = dir.join("cjk.rs");
    let src = "// 中文注释：这是一个用于测试真实 token 计数的文件。\n\
               // 英文注释 here.\n\
               pub fn hello() -> i32 {\n\
                   let greeting = \"你好，世界\";\n\
                   greeting.len() as i32\n\
               }\n";
    std::fs::write(&path, src).expect("write fixture");
    let output = run(&["outline", "--json", path.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let value: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(value["saved"]["tokens_before"].as_u64().unwrap(), 57);
    assert!(value["saved"]["percent"].as_u64().unwrap() <= 100);
}

#[test]
fn outline_format_json_is_alias_of_json_flag() {
    let a = stdout(&run(&["outline", "--json", FIXTURE]));
    let b = stdout(&run(&["outline", "--format", "json", FIXTURE]));
    assert_eq!(a, b);
}

#[test]
fn outline_no_doc_and_no_lines_flags() {
    let output = run(&["outline", "--json", "--no-doc", "--no-lines", FIXTURE]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let value: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert!(value["symbols"][0].get("doc_comment").is_none());
    assert!(value["symbols"][0].get("start_line").is_none());
    assert!(value["symbols"][0].get("end_line").is_none());
}

#[test]
fn outline_missing_file_exits_1() {
    let output = run(&["outline", "/tmp/opencode/does-not-exist.rs"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("failed to read"));
}

#[test]
fn outline_unsupported_extension_exits_2() {
    let path = tmp_dir("unsupported").join("nope.xyz");
    std::fs::write(&path, "whatever").expect("write fixture");
    let output = run(&["outline", path.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("unsupported extension"));
}

#[test]
fn python_outline_text_and_json() {
    let output = run(&["outline", PY_FIXTURE]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("class Point"), "unexpected: {text}");
    assert!(
        text.contains("def add(a: int, b: int) -> int"),
        "unexpected: {text}"
    );

    let json_output = run(&["outline", "--json", PY_FIXTURE]);
    assert_eq!(json_output.status.code(), Some(0));
    let value: Value = serde_json::from_str(&stdout(&json_output)).expect("valid json");
    assert_eq!(value["language"], "python");
    let names: Vec<&str> = value["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["Point", "__init__", "norm", "add"]);
    assert_eq!(value["symbols"][0]["doc_comment"], serde_json::Value::Null);
    assert_eq!(value["symbols"][1]["doc_comment"], "A point in 2D space.");
}

#[test]
fn python_symbol_slices_original_source() {
    let output = run(&["symbol", PY_FIXTURE, "--name", "norm"]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("# norm"), "no locator: {text}");
    assert!(text.contains("def norm(self)"), "unexpected: {text}");
    assert!(text.contains("self.x * self.x"), "unexpected: {text}");
    assert!(!text.contains("class Point"), "slice too wide: {text}");
}

#[test]
fn go_outline_and_symbol() {
    let output = run(&["outline", "--json", GO_FIXTURE]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let value: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(value["language"], "go");
    let names: Vec<&str> = value["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["Point", "Norm", "Add", "MaxRetries"]);
    assert_eq!(
        value["symbols"][1]["doc_comment"],
        "Norm returns the distance from the origin."
    );

    let sym = run(&["symbol", GO_FIXTURE, "--name", "Add"]);
    assert_eq!(sym.status.code(), Some(0), "stderr: {}", stderr(&sym));
    let text = stdout(&sym);
    assert!(
        text.contains("func Add(a, b int) int {"),
        "unexpected: {text}"
    );
    assert!(text.contains("return a + b"), "unexpected: {text}");
}

#[test]
fn read_works_on_python_and_go() {
    let py = run(&["read", PY_FIXTURE, "--lines", "6-8"]);
    assert_eq!(py.status.code(), Some(0), "stderr: {}", stderr(&py));
    assert!(stdout(&py).contains("class Point:"));
    let go = run(&["read", GO_FIXTURE, "--lines", "11-13"]);
    assert_eq!(go.status.code(), Some(0), "stderr: {}", stderr(&go));
    assert!(stdout(&go).contains("func (p *Point) Norm() float64 {"));
}

#[test]
fn read_preserves_crlf_line_endings() {
    let dir = tmp_dir("crlf");
    let file = dir.join("crlf.txt");
    std::fs::write(&file, "one\r\ntwo\r\nthree\r\n").expect("write crlf file");
    let output = run(&["read", file.to_str().unwrap(), "--lines", "1-3"]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let text = stdout(&output);
    assert!(
        text.contains("one\r\ntwo\r\nthree\r\n"),
        "endings: {text:?}"
    );
}

#[test]
fn javascript_outline_symbol_and_deps() {
    let output = run(&["outline", "--json", JS_FIXTURE]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let value: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(value["language"], "javascript");
    let names: Vec<&str> = value["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        vec!["os", "User", "greet", "formatName", "MAX_RETRIES"]
    );

    let sym = run(&["symbol", JS_FIXTURE, "--name", "greet"]);
    assert_eq!(sym.status.code(), Some(0), "stderr: {}", stderr(&sym));
    assert!(stdout(&sym).contains("greet()"));

    let deps = run(&["deps", "--json", JS_FIXTURE]);
    let value: Value = serde_json::from_str(&stdout(&deps)).expect("valid json");
    assert_eq!(value["language"], "javascript");
    let imports: Vec<(String, String)> = value["imports"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["target"].as_str().unwrap().to_string(),
                i["kind"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        imports,
        vec![
            ("express".to_string(), "external".to_string()),
            ("./helpers".to_string(), "local".to_string()),
            ("os".to_string(), "external".to_string()),
        ]
    );
}

#[test]
fn java_outline_symbol_and_deps_local_probe() {
    let output = run(&["outline", "--json", JAVA_FIXTURE]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let value: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(value["language"], "java");
    let names: Vec<&str> = value["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        vec!["Point", "x", "y", "norm", "Repo", "all", "App", "main"]
    );

    let sym = run(&["symbol", JAVA_FIXTURE, "--name", "norm"]);
    assert_eq!(sym.status.code(), Some(0), "stderr: {}", stderr(&sym));
    assert!(stdout(&sym).contains("Math.sqrt"));

    // com.example.util.Helper resolves to a local package dir under the cwd.
    let dir = tmp_dir("java-local-probe");
    std::fs::create_dir_all(dir.join("com/example/util")).expect("create dirs");
    std::fs::write(
        dir.join("com/example/util/Helper.java"),
        "package com.example.util;\n",
    )
    .expect("write file");
    let deps = run_in(&dir, &["deps", "--json", JAVA_FIXTURE]);
    let value: Value = serde_json::from_str(&stdout(&deps)).expect("valid json");
    let imports: Vec<(String, String)> = value["imports"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["target"].as_str().unwrap().to_string(),
                i["kind"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        imports,
        vec![
            ("java.util.List".to_string(), "external".to_string()),
            ("java.lang.Math".to_string(), "external".to_string()),
            ("com.example.util.Helper".to_string(), "local".to_string()),
        ]
    );
}

#[test]
fn c_outline_symbol_and_deps() {
    let output = run(&["outline", "--json", C_FIXTURE]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let value: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(value["language"], "c");
    let names: Vec<&str> = value["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        vec!["MAX_RETRIES", "Point", "x", "y", "add", "Color", "helper"]
    );
    assert_eq!(value["symbols"][1]["doc_comment"], "A 2D point.");

    let sym = run(&["symbol", C_FIXTURE, "--name", "add"]);
    assert_eq!(sym.status.code(), Some(0), "stderr: {}", stderr(&sym));
    let text = stdout(&sym);
    assert!(text.contains("int add(int a, int b)"), "unexpected: {text}");
    assert!(text.contains("return a + b;"), "unexpected: {text}");
    assert!(!text.contains("enum Color"), "slice too wide: {text}");

    // Quoted includes are local, angle includes are external.
    let deps = run(&["deps", "--json", DEPS_C]);
    let value: Value = serde_json::from_str(&stdout(&deps)).expect("valid json");
    assert_eq!(value["language"], "c");
    let imports: Vec<(String, String)> = value["imports"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["target"].as_str().unwrap().to_string(),
                i["kind"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        imports,
        vec![
            ("stdio.h".to_string(), "external".to_string()),
            ("stdlib.h".to_string(), "external".to_string()),
            ("util.h".to_string(), "local".to_string()),
            ("../common/defs.h".to_string(), "local".to_string()),
        ]
    );
}

#[test]
fn cpp_outline_compact_and_deps() {
    let output = run(&["outline", "--json", CPP_FIXTURE]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let value: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(value["language"], "cpp");
    let names: Vec<&str> = value["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        vec!["VERSION", "app", "Widget", "value", "reset", "sum"]
    );
    assert_eq!(value["symbols"][2]["doc_comment"], "A widget.");

    // compact keeps the template header.
    let sym = run(&["symbol", CPP_FIXTURE, "--name", "Widget", "--compact"]);
    assert_eq!(sym.status.code(), Some(0), "stderr: {}", stderr(&sym));
    let text = stdout(&sym);
    assert!(text.contains("template <typename T>"), "unexpected: {text}");
    assert!(text.contains("class Widget"), "unexpected: {text}");

    let deps = run(&["deps", "--json", DEPS_CPP]);
    let value: Value = serde_json::from_str(&stdout(&deps)).expect("valid json");
    let imports: Vec<(String, String)> = value["imports"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["target"].as_str().unwrap().to_string(),
                i["kind"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        imports,
        vec![
            ("vector".to_string(), "external".to_string()),
            ("local.hpp".to_string(), "local".to_string()),
            ("std".to_string(), "external".to_string()),
            ("std::vector".to_string(), "external".to_string()),
        ]
    );
}

#[test]
fn csharp_outline_and_deps() {
    let output = run(&["outline", "--json", CS_FIXTURE]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let value: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(value["language"], "csharp");
    let names: Vec<&str> = value["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        vec![
            "Demo.App", "Point", "x", "y", "Norm", "IRepo", "All", "Pair", "Status", "Vector2", "X"
        ]
    );

    let sym = run(&["symbol", CS_FIXTURE, "--name", "Norm"]);
    assert_eq!(sym.status.code(), Some(0), "stderr: {}", stderr(&sym));
    assert!(stdout(&sym).contains("Sqrt(x * x + y * y)"));

    let deps = run(&["deps", "--json", DEPS_CS]);
    let value: Value = serde_json::from_str(&stdout(&deps)).expect("valid json");
    let imports: Vec<(String, String)> = value["imports"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["target"].as_str().unwrap().to_string(),
                i["kind"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        imports,
        vec![
            ("System".to_string(), "external".to_string()),
            (
                "System.Collections.Generic".to_string(),
                "external".to_string()
            ),
            ("System.Math".to_string(), "external".to_string()),
            ("Demo.Utils".to_string(), "external".to_string()),
        ]
    );
}

#[test]
fn ruby_outline_symbol_and_deps() {
    let output = run(&["outline", "--json", RB_FIXTURE]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let value: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(value["language"], "ruby");
    let names: Vec<&str> = value["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        vec!["User", "greet", "build", "add", "Utils", "normalize"]
    );
    assert_eq!(value["symbols"][0]["doc_comment"], "A user entity.");

    let sym = run(&["symbol", RB_FIXTURE, "--name", "greet"]);
    assert_eq!(sym.status.code(), Some(0), "stderr: {}", stderr(&sym));
    assert!(stdout(&sym).contains("def greet(name)"));

    let deps = run(&["deps", "--json", DEPS_RB]);
    let value: Value = serde_json::from_str(&stdout(&deps)).expect("valid json");
    let imports: Vec<(String, String)> = value["imports"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["target"].as_str().unwrap().to_string(),
                i["kind"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        imports,
        vec![
            ("json".to_string(), "external".to_string()),
            ("sinatra/base".to_string(), "external".to_string()),
            ("helpers".to_string(), "local".to_string()),
            ("./local".to_string(), "local".to_string()),
            ("../shared/mixins".to_string(), "local".to_string()),
        ]
    );
}

#[test]
fn lua_outline_compact_and_deps() {
    let output = run(&["outline", "--json", LUA_FIXTURE]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let value: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(value["language"], "lua");
    let names: Vec<&str> = value["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        vec![
            "json",
            "helpers",
            "Counter",
            "Counter.new",
            "add",
            "Counter:increment",
            "MAX_RETRIES"
        ]
    );

    // compact uses -- markers and keeps the end closer.
    let sym = run(&["symbol", LUA_FIXTURE, "--name", "add", "--compact"]);
    assert_eq!(sym.status.code(), Some(0), "stderr: {}", stderr(&sym));
    let text = stdout(&sym);
    assert!(text.contains("-- ... ["), "lua marker: {text}");
    assert!(text.contains("end"), "end closer: {text}");

    let deps = run(&["deps", "--json", DEPS_LUA]);
    let value: Value = serde_json::from_str(&stdout(&deps)).expect("valid json");
    let imports: Vec<(String, String)> = value["imports"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["target"].as_str().unwrap().to_string(),
                i["kind"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        imports,
        vec![
            ("json".to_string(), "external".to_string()),
            ("socket".to_string(), "external".to_string()),
            ("./local".to_string(), "local".to_string()),
        ]
    );
}

#[test]
fn outline_json_error_contract() {
    let output = run(&["outline", "--json", "/tmp/opencode/does-not-exist.rs"]);
    assert_eq!(output.status.code(), Some(1));
    let value: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(value["error"]["code"], 1);
    assert!(
        value["error"]["message"]
            .as_str()
            .unwrap()
            .contains("failed to read")
    );
}

#[test]
fn symbol_slices_the_original_source() {
    let output = run(&["symbol", FIXTURE, "--name", "add"]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let text = stdout(&output);
    assert!(text.starts_with("# add"), "no locator: {text}");
    assert!(text.contains(":4-6"), "no line range: {text}");
    assert!(
        text.contains("pub fn add(a: i32, b: i32) -> i32 {"),
        "unexpected: {text}"
    );
    assert!(text.contains("a + b"), "unexpected: {text}");
    assert!(text.contains("saved ~"), "missing savings: {text}");
}

#[test]
fn symbol_kind_filter_disambiguates_same_name_symbols() {
    // Issue #6: a method and a same-named local variable. Source order picks
    // the variable; --kind method must pick the method instead.
    let dir = tmp_dir("symbol-kind");
    let path = dir.join("agent.ts");
    let src = r#"
const step = 1;

export class ReactLoopAgent {
  async step(): Promise<void> {
    void step;
  }
}
"#;
    std::fs::write(&path, src).expect("write fixture");
    let p = path.to_str().unwrap();

    let default_pick = run(&["symbol", "--json", p, "--name", "step"]);
    assert_eq!(default_pick.status.code(), Some(0));
    let value: Value = serde_json::from_str(&stdout(&default_pick)).expect("json");
    assert_eq!(value["symbol"]["kind"], "var");

    let method_pick = run(&["symbol", "--json", p, "--name", "step", "--kind", "method"]);
    assert_eq!(method_pick.status.code(), Some(0));
    let value: Value = serde_json::from_str(&stdout(&method_pick)).expect("json");
    assert_eq!(value["symbol"]["kind"], "method");
    assert!(value["slice"].as_str().unwrap().contains("async step"));

    let miss = run(&["symbol", p, "--name", "step", "--kind", "struct"]);
    assert_eq!(miss.status.code(), Some(4), "kind miss must exit 4");
}

#[test]
fn output_file_receives_the_full_payload() {
    // Issue #5: large payloads must be writable to a file, bypassing stdout
    // limits. The file receives exactly the stdout bytes; stdout stays empty.
    let dir = tmp_dir("output-file");
    let out_file = dir.join("out.json");
    let output = run(&[
        "outline",
        "--json",
        "--output",
        out_file.to_str().unwrap(),
        FIXTURE,
    ]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert!(stdout(&output).is_empty(), "stdout must stay clean");
    assert!(
        stderr(&output).contains("wrote"),
        "confirmation: {}",
        stderr(&output)
    );

    let file_bytes = std::fs::read(&out_file).expect("read output file");
    let value: Value = serde_json::from_slice(&file_bytes).expect("file is valid json");
    assert_eq!(value["tool"], "outline");
    assert_eq!(value["symbols"].as_array().map(Vec::len), Some(4));

    // The file must be byte-identical to a plain stdout run.
    let plain = run(&["outline", "--json", FIXTURE]);
    assert_eq!(std::fs::read(&out_file).unwrap(), plain.stdout);
}

#[test]
fn output_file_in_unwritable_dir_exits_1() {
    let output = run(&[
        "outline",
        "--output",
        "/nonexistent-dir-ctxctl/out.txt",
        FIXTURE,
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("failed to write"));
}

#[test]
fn symbol_json_contract() {
    let output = run(&["symbol", "--json", FIXTURE, "--name", "norm"]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let value: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["tool"], "symbol");
    assert_eq!(value["name"], "norm");
    assert_eq!(value["symbol"]["kind"], "function");
    assert!(value["slice"].as_str().unwrap().contains("fn norm"));
    assert!(value["saved"]["percent"].as_u64().unwrap() > 0);
}

#[test]
fn symbol_signature_only() {
    let output = run(&["symbol", FIXTURE, "--name", "add", "--signature"]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let text = stdout(&output);
    assert!(text.starts_with("# add"), "no locator: {text}");
    assert!(text.contains(":4-6"), "no line range: {text}");
    assert!(text.contains("saved ~"), "missing savings: {text}");
    assert!(
        text.trim_end()
            .ends_with("pub fn add(a: i32, b: i32) -> i32"),
        "unexpected: {text}"
    );
    assert!(
        !text.contains("a + b"),
        "body must not appear with --signature"
    );
}

#[test]
fn symbol_subrange_lines() {
    let output = run(&["symbol", FIXTURE, "--name", "add", "--lines", "2-3"]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("a + b"), "unexpected: {text}");
    assert!(text.contains('}'), "missing closing brace: {text}");
    assert!(
        !text.contains("pub fn add(a: i32, b: i32) -> i32 {"),
        "first line should be excluded"
    );
}

#[test]
fn symbol_subrange_out_of_bounds_exits_2() {
    let output = run(&["symbol", FIXTURE, "--name", "add", "--lines", "5-9"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("out of bounds"));
}

#[test]
fn csharp_deps_local_probe() {
    // Demo.Utils resolves to a local package dir under the cwd.
    let dir = tmp_dir("csharp-local-probe");
    std::fs::create_dir_all(dir.join("Demo/Utils")).expect("create dirs");
    std::fs::write(
        dir.join("Demo/Utils/Helper.cs"),
        "namespace Demo.Utils { }\n",
    )
    .expect("write");
    let deps = run_in(&dir, &["deps", "--json", DEPS_CS]);
    let value: Value = serde_json::from_str(&stdout(&deps)).expect("valid json");
    let imports: Vec<(String, String)> = value["imports"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["target"].as_str().unwrap().to_string(),
                i["kind"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        imports.last(),
        Some(&("Demo.Utils".to_string(), "local".to_string()))
    );
}

#[test]
fn symbol_subrange_rejects_multiple_ranges() {
    let output = run(&["symbol", FIXTURE, "--name", "add", "--lines", "1-2,4-5"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("single range"));
}

#[test]
fn symbol_compact_prunes_body() {
    let output = run(&["symbol", FIXTURE, "--name", "add", "--compact"]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let text = stdout(&output);
    assert!(
        text.contains("pub fn add(a: i32, b: i32) -> i32 {"),
        "signature kept: {text}"
    );
    assert!(
        text.contains("// ... [1 lines omitted]"),
        "no marker: {text}"
    );
    assert!(!text.contains("a + b"), "body must be folded: {text}");

    let json_output = run(&["symbol", "--json", FIXTURE, "--name", "add", "--compact"]);
    let value: Value = serde_json::from_str(&stdout(&json_output)).expect("valid json");
    assert_eq!(value["tool"], "symbol");
    assert_eq!(value["schema_version"], 1);
    assert!(
        value["compact"].as_str().unwrap().contains("// ... ["),
        "compact field: {value}"
    );
    assert!(
        value.get("slice").is_none(),
        "slice must be absent in compact mode"
    );
}

#[test]
fn symbol_compact_conflicts_with_signature_and_lines() {
    let with_signature = run(&[
        "symbol",
        FIXTURE,
        "--name",
        "add",
        "--compact",
        "--signature",
    ]);
    assert_eq!(with_signature.status.code(), Some(2));
    let with_lines = run(&[
        "symbol",
        FIXTURE,
        "--name",
        "add",
        "--compact",
        "--lines",
        "1-2",
    ]);
    assert_eq!(with_lines.status.code(), Some(2));
}

#[test]
fn symbol_not_found_exits_4() {
    let output = run(&["symbol", FIXTURE, "--name", "nope"]);
    assert_eq!(output.status.code(), Some(4));
    assert!(stderr(&output).contains("symbol not found: nope"));
}

#[test]
fn read_returns_exact_line_ranges() {
    let output = run(&["read", FIXTURE, "--lines", "1-3,4-4"]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("# "), "no locator: {text}");
    assert!(text.contains("//! Fixture module used by ctxctl integration tests."));
    assert!(text.contains("/// Adds two numbers."));
    assert!(text.contains("pub fn add(a: i32, b: i32) -> i32 {"));
    assert!(text.contains("Saved ~"), "missing savings: {text}");
}

#[test]
fn read_open_ended_range() {
    let output = run(&["read", FIXTURE, "--lines", "19-"]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert!(stdout(&output).contains("pub const ANSWER: i32 = 42;"));
}

#[test]
fn read_json_contract() {
    let output = run(&["read", "--json", FIXTURE, "--lines", "1-2"]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let value: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(value["tool"], "read");
    assert_eq!(value["ranges"][0]["start_line"], 1);
    assert_eq!(value["ranges"][0]["end_line"], 2);
    assert!(
        value["ranges"][0]["slice"]
            .as_str()
            .unwrap()
            .contains("Fixture module")
    );
}

#[test]
fn read_out_of_bounds_exits_2() {
    let output = run(&["read", FIXTURE, "--lines", "99-100"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("out of bounds"));
}

#[test]
fn read_invalid_range_exits_2() {
    let output = run(&["read", FIXTURE, "--lines", "abc"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("invalid range"));
}

fn printf_lines(count: usize, middle: &str, separator: &str) -> String {
    let mut parts: Vec<String> = (1..=count).map(|i| format!("l{i}")).collect();
    let mid = count / 2;
    parts.insert(mid, middle.to_string());
    let mut body = parts.join("\\n");
    body.push_str("\\n");
    format!("printf '{body}{separator}'")
}

#[test]
fn exec_compresses_output_and_reports_savings() {
    let cmd = printf_lines(26, "error: boom", "");
    let output = run(&["exec", &cmd]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let text = stdout(&output);
    assert!(text.starts_with("$ printf"), "command not echoed: {text}");
    let text = body(&output);
    assert!(text.contains("error: boom"), "critical line lost: {text}");
    assert_eq!(
        text.matches("... [8 lines omitted]").count(),
        2,
        "fold markers: {text}"
    );
    assert!(text.contains("Saved ~"), "no savings line: {text}");
}

#[test]
fn exec_streams_huge_output_bounded_memory() {
    // 200k lines (~1.5 MB): the streaming compressor keeps only the
    // head/tail windows and matches; output must render exactly like the
    // batch compressor would.
    let cmd = "sh -c 'seq 1 200000'";
    let output = run(&["exec", cmd]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let text = body(&output);
    assert!(
        text.starts_with("1\n2\n3\n4\n5\n... [199990 lines omitted]\n"),
        "head/marker: {text:?}"
    );
    assert!(text.contains("199996\n199997\n199998\n199999\n200000"));
    assert!(text.contains("Saved ~"), "savings: {text}");
    assert!(
        text.ends_with("tokens)\n") || text.ends_with("tokens)"),
        "savings line last: {text}"
    );
}

#[test]
fn exec_custom_keep_pattern_keeps_matching_lines() {
    let cmd = printf_lines(25, "TODO: later", "");
    let bare = run(&["exec", &cmd]);
    assert!(!body(&bare).contains("TODO: later"));

    let kept = run(&["exec", "--keep", "TODO", &cmd]);
    assert_eq!(kept.status.code(), Some(0));
    assert!(body(&kept).contains("TODO: later"));
}

#[test]
fn exec_json_contract() {
    let cmd = printf_lines(25, "error: x", "");
    let output = run(&["exec", "--json", &cmd]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let value: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["tool"], "exec");
    assert_eq!(value["exit_code"], 0);
    assert!(value["compressed"].as_str().unwrap().contains("error: x"));
    assert!(value["saved"]["percent"].as_u64().unwrap() > 0);
}

#[test]
fn exec_passes_through_exit_code() {
    let output = run(&["exec", "sh -c 'exit 3'"]);
    assert_eq!(output.status.code(), Some(3));
    let json_output = run(&["exec", "--json", "sh -c 'exit 3'"]);
    let value: Value = serde_json::from_str(&stdout(&json_output)).expect("valid json");
    assert_eq!(value["exit_code"], 3);
}

#[cfg(unix)]
#[test]
fn exec_signal_exit_code_is_128_plus_signal() {
    // A signal-killed child must not collapse to exit code 1.
    let output = run(&["exec", "sh -c 'kill -TERM $$'"]);
    assert_eq!(
        output.status.code(),
        Some(143),
        "stderr: {}",
        stderr(&output)
    );
    let json_output = run(&["exec", "--json", "sh -c 'kill -TERM $$'"]);
    let value: Value = serde_json::from_str(&stdout(&json_output)).expect("valid json");
    assert_eq!(value["exit_code"], 143);
}

#[test]
fn exec_invalid_keep_pattern_fails() {
    let output = run(&["exec", "--keep", "[", "true"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("invalid regex pattern"));
}

#[test]
fn exec_empty_keep_pattern_fails_without_spawning() {
    let dir = tmp_dir("exec-empty-keep");
    let probe = dir.join("probe");
    let cmd = format!("sh -c 'sleep 1; printf x > {}'", probe.display());
    let output = run(&["exec", "--keep", "", &cmd]);
    assert_eq!(output.status.code(), Some(1), "stderr: {}", stderr(&output));
    assert!(
        stderr(&output).contains("empty"),
        "stderr must name the problem: {}",
        stderr(&output)
    );
    // Validation happens before spawn, so nothing can have written the
    // probe; poll past the child's own sleep to catch a detached process.
    for _ in 0..30 {
        assert!(!probe.exists(), "command ran despite invalid --keep");
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[test]
fn exec_rejects_unquoted_metacharacters() {
    let output = run(&["exec", "true && echo hi"]);
    assert_eq!(output.status.code(), Some(1), "stderr: {}", stderr(&output));
    let err = stderr(&output);
    assert!(err.contains("single command"), "hint: {err}");
    assert!(err.contains("sh -c"), "hint: {err}");
}

#[test]
fn exec_allows_quoted_metacharacters() {
    let cmd = "printf 'a && b | c; d <e>f\\n'";
    let output = run(&["exec", cmd]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert!(
        stdout(&output).contains("a && b | c; d <e>f"),
        "quoted text must pass through: {}",
        stdout(&output)
    );
}

#[test]
fn exec_rejects_pipe_and_semicolon() {
    for cmd in ["ls | head -1", "true; echo done"] {
        let output = run(&["exec", cmd]);
        assert!(!output.status.success(), "cmd: {cmd}");
        assert!(
            stderr(&output).contains("sh -c"),
            "cmd: {cmd}, stderr: {}",
            stderr(&output)
        );
    }
}

#[test]
fn exec_merges_stderr_without_blank_line_gap() {
    // stdout ends with \n; the merged output must not gain an extra blank
    // line before the stderr content.
    let output = run(&["exec", "sh -c 'printf \"a\\nb\\n\"; echo ERR >&2'"]);
    assert_eq!(output.status.code(), Some(0));
    let text = body(&output);
    assert!(text.contains("b\nERR"), "no blank gap: {text:?}");
    assert!(!text.contains("b\n\nERR"), "blank gap: {text:?}");
}

#[test]
fn exec_keeps_rustc_location_after_error_header() {
    // rustc prints `   --> file:line:col` right under each diagnostic;
    // compression must not drop it or wedge an omit marker between.
    let mut cmd = String::from("printf '");
    for i in 1..=10 {
        cmd.push_str(&format!("compiling {i}\\n"));
    }
    cmd.push_str("error[E0308]: mismatched types\\n");
    cmd.push_str("   --> src/foo.rs:12:5\\n");
    for i in 1..=12 {
        cmd.push_str(&format!("more output {i}\\n"));
    }
    cmd.push('\'');
    let output = run(&["exec", &cmd]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let text = body(&output);
    assert!(
        text.contains("error[E0308]: mismatched types\n   --> src/foo.rs:12:5"),
        "header and location must stay adjacent: {text}"
    );
    assert!(text.contains("... [5 lines omitted]"), "folds: {text}");
}

#[test]
fn exec_ineffective_keep_warns_on_stderr_not_stdout() {
    // A pattern matching nearly every line defeats compression; the CLI
    // surfaces that on stderr while stdout stays pure machine data.
    let cmd = printf_lines(25, "filler", "");
    let output = run(&["exec", "--keep", "l", &cmd]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert!(
        stderr(&output).contains("keep patterns matched most of the output"),
        "warning on stderr: {:?}",
        stderr(&output)
    );
    assert!(
        !stdout(&output).contains("warning:"),
        "stdout must stay clean: {}",
        stdout(&output)
    );
    assert!(
        body(&output).contains("Saved ~"),
        "metrics unaffected by the warning"
    );
}

#[test]
fn exec_json_surfaces_ineffective_warning_in_payload() {
    // JSON consumers read the warning from the envelope field; stdout stays
    // parseable and free of raw notice text.
    let cmd = printf_lines(25, "filler", "");
    let output = run(&["exec", "--json", "--keep", "l", &cmd]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let value: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert!(
        value["warning"]
            .as_str()
            .unwrap_or_default()
            .contains("keep patterns matched most of the output"),
        "warning field: {value}"
    );
    assert!(value["compressed"].as_str().is_some());
}

#[test]
fn no_saved_suppresses_metrics() {
    let outline = run(&["outline", "--no-saved", FIXTURE]);
    assert!(!stdout(&outline).contains("saved"));

    let cmd = printf_lines(25, "error: x", "");
    let with_metrics = run(&["exec", &cmd]);
    assert!(body(&with_metrics).contains("Saved ~"));
    let without = run(&["exec", "--no-saved", &cmd]);
    assert!(!body(&without).contains("Saved"));
}

#[test]
fn explicit_config_file_changes_behavior() {
    let dir = tmp_dir("config-exec");
    let config = dir.join("config.toml");
    std::fs::write(
        &config,
        "[exec]\nkeep = [\"TODO\"]\nhead_lines = 2\ntail_lines = 2\ncollapse_threshold = 0\n",
    )
    .expect("write config");
    let cmd = printf_lines(26, "TODO: later", "");

    let bare = run(&["exec", &cmd]);
    assert!(!body(&bare).contains("TODO: later"));

    let configured = run(&["exec", "--config", config.to_str().unwrap(), cmd.as_str()]);
    assert_eq!(
        configured.status.code(),
        Some(0),
        "stderr: {}",
        stderr(&configured)
    );
    let text = body(&configured);
    assert!(text.contains("TODO: later"));
    assert_eq!(
        text.matches("... [11 lines omitted]").count(),
        2,
        "markers: {text}"
    );
}

#[test]
fn config_keep_replaces_defaults() {
    let dir = tmp_dir("config-replace");
    let config = dir.join("config.toml");
    std::fs::write(
        &config,
        "[exec]\nkeep = [\"TODO\"]\ncollapse_threshold = 0\n",
    )
    .expect("write config");
    let cmd = "printf 'l1\\nl2\\nl3\\nl4\\nl5\\nl6\\nerror: x\\nTODO: y\\nl9\\nl10\\nl11\\nl12\\nl13\\nl14\\n'";
    let output = run(&["exec", "--config", config.to_str().unwrap(), cmd]);
    let text = body(&output);
    assert!(
        text.contains("TODO: y"),
        "config pattern should keep TODO: {text}"
    );
    assert!(
        !text.contains("error: x"),
        "replaced defaults must not keep error: {text}"
    );
}

#[test]
fn project_config_discovery_walks_up() {
    let root = tmp_dir("discovery");
    let project = root.join("sub/deep");
    std::fs::create_dir_all(&project).expect("create dirs");
    std::fs::create_dir_all(root.join(".ctxctl")).expect("create config dir");
    std::fs::write(
        root.join(".ctxctl/config.toml"),
        "[general]\nshow_saved = false\n",
    )
    .expect("write project config");
    let output = run_in(&project, &["outline", FIXTURE]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert!(!stdout(&output).contains("saved"));
}

#[test]
fn invalid_config_fails_with_exit_1() {
    let dir = tmp_dir("bad-config");
    let config = dir.join("config.toml");
    std::fs::write(&config, "not [valid toml").expect("write config");
    let output = run(&["outline", "--config", config.to_str().unwrap(), FIXTURE]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("invalid config"));
}

#[test]
fn paths_ignore_is_replaced_not_concatenated() {
    // Defaults ignore `node_modules`; a config listing only `vendor` must
    // REPLACE the list, so node_modules imports classify as external.
    let dir = tmp_dir("ignore-replace");
    let ts = dir.join("app.ts");
    std::fs::write(&ts, "import { x } from \"node_modules/pkg\";\n").expect("write");
    let config = dir.join("config.toml");
    std::fs::write(&config, "[paths]\nignore = [\"vendor\"]\n").expect("write");
    let output = run_in(
        &dir,
        &[
            "deps",
            "--json",
            "--config",
            config.to_str().unwrap(),
            "app.ts",
        ],
    );
    let value: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(value["imports"][0]["kind"], "external");

    std::fs::create_dir_all(dir.join("node_modules/pkg")).expect("create dirs");
    let with_default = run_in(&dir, &["deps", "--json", "app.ts"]);
    let value: Value = serde_json::from_str(&stdout(&with_default)).expect("valid json");
    assert_eq!(value["imports"][0]["kind"], "ignored");
}

#[test]
fn partial_section_keeps_other_section_defaults() {
    // A config touching only [exec].head_lines must leave the default keep
    // patterns active (key-wise merge, not whole-section replacement).
    let dir = tmp_dir("partial-section");
    std::fs::write(dir.join("config.toml"), "[exec]\nhead_lines = 1\n").expect("write");
    let cmd = printf_lines(25, "error: boom", "");
    let output = run(&[
        "exec",
        "--config",
        dir.join("config.toml").to_str().unwrap(),
        cmd.as_str(),
    ]);
    let text = body(&output);
    assert!(
        text.contains("error: boom"),
        "default keep must survive: {text}"
    );
}

#[test]
fn explicit_config_overrides_project_config() {
    let root = tmp_dir("explicit-wins");
    let project = root.join("proj");
    std::fs::create_dir_all(project.join(".ctxctl")).expect("create dirs");
    std::fs::write(
        project.join(".ctxctl/config.toml"),
        "[general]\nshow_saved = false\n",
    )
    .expect("write project config");
    let explicit = root.join("explicit.toml");
    std::fs::write(&explicit, "[general]\nshow_saved = true\n").expect("write explicit");
    let output = run_in(
        &project,
        &["outline", "--config", explicit.to_str().unwrap(), FIXTURE],
    );
    assert!(
        stdout(&output).contains("saved"),
        "explicit must win: {}",
        stdout(&output)
    );
    let without = run_in(&project, &["outline", FIXTURE]);
    assert!(!stdout(&without).contains("saved"));
}

#[test]
fn nearest_project_config_wins() {
    let root = tmp_dir("nearest-wins");
    let deep = root.join("outer/inner");
    std::fs::create_dir_all(&deep).expect("create dirs");
    std::fs::create_dir_all(root.join(".ctxctl")).expect("create outer config dir");
    std::fs::write(
        root.join(".ctxctl/config.toml"),
        "[general]\nshow_saved = false\n",
    )
    .expect("write outer");
    std::fs::create_dir_all(root.join("outer/.ctxctl")).expect("create inner config dir");
    std::fs::write(
        root.join("outer/.ctxctl/config.toml"),
        "[general]\nshow_saved = true\n",
    )
    .expect("write inner");
    let output = run_in(&deep, &["outline", FIXTURE]);
    assert!(stdout(&output).contains("saved"), "nearest config must win");
}

#[test]
fn outline_fold_threshold_folds_in_text_mode() {
    let dir = tmp_dir("fold");
    let config = dir.join("config.toml");
    std::fs::write(&config, "[outline]\nfold_threshold = 2\n").expect("write config");
    let output = run(&["outline", "--config", config.to_str().unwrap(), FIXTURE]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let text = stdout(&output);
    assert!(
        text.contains("... [2 symbols omitted]"),
        "no fold marker: {text}"
    );
    assert!(
        text.contains("struct  Point"),
        "first symbols still shown: {text}"
    );
    assert!(
        !text.contains("const   ANSWER"),
        "symbols beyond threshold must fold: {text}"
    );
}

#[test]
fn outline_fold_threshold_does_not_affect_json() {
    let dir = tmp_dir("fold-json");
    let config = dir.join("config.toml");
    std::fs::write(&config, "[outline]\nfold_threshold = 1\n").expect("write config");
    let output = run(&[
        "outline",
        "--json",
        "--config",
        config.to_str().unwrap(),
        FIXTURE,
    ]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let value: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(value["symbols"].as_array().map(Vec::len), Some(4));
}

#[test]
fn xdg_config_new_keys_parse() {
    let root = tmp_dir("xdg-newkeys");
    let xdg = root.join("ctxctl");
    std::fs::create_dir_all(&xdg).expect("create xdg dir");
    std::fs::write(
        xdg.join("config.toml"),
        "[paths]\nignore = [\"node_modules\", \"target\", \"dist\", \".git\", \"vendor\"]\n[outline]\nfold_threshold = 2\n",
    )
    .expect("write xdg config");
    let mut cmd = Command::new(BIN);
    cmd.env("XDG_CONFIG_HOME", &root).args(["outline", FIXTURE]);
    let output = cmd.output().expect("run ctxctl");
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert!(
        stdout(&output).contains("... [2 symbols omitted]"),
        "fold_threshold from XDG config not applied"
    );
}

#[test]
fn unknown_config_key_is_an_error() {
    let root = tmp_dir("xdg-unknown-key");
    let xdg = root.join("ctxctl");
    std::fs::create_dir_all(&xdg).expect("create xdg dir");
    std::fs::write(xdg.join("config.toml"), "[exec]\nhead_line = 3\n").expect("write xdg config");
    let mut cmd = Command::new(BIN);
    cmd.env("XDG_CONFIG_HOME", &root).args(["outline", FIXTURE]);
    let output = cmd.output().expect("run ctxctl");
    assert_eq!(output.status.code(), Some(1), "stderr: {}", stderr(&output));
    assert!(
        stderr(&output).contains("head_line"),
        "error must name the unknown key: {}",
        stderr(&output)
    );
}

#[test]
fn project_config_overrides_global_fold_threshold() {
    let root = tmp_dir("fold-merge");
    let xdg = root.join("xdg/ctxctl");
    std::fs::create_dir_all(&xdg).expect("create xdg dir");
    std::fs::write(xdg.join("config.toml"), "[outline]\nfold_threshold = 1\n")
        .expect("write xdg config");
    let project = root.join("proj/.ctxctl");
    std::fs::create_dir_all(&project).expect("create project dir");
    std::fs::write(
        project.join("config.toml"),
        "[outline]\nfold_threshold = 4\n",
    )
    .expect("write project config");
    let mut cmd = Command::new(BIN);
    cmd.env("XDG_CONFIG_HOME", root.join("xdg"))
        .current_dir(root.join("proj"))
        .args(["outline", FIXTURE]);
    let output = cmd.output().expect("run ctxctl");
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert!(
        !stdout(&output).contains("symbols omitted"),
        "project fold_threshold=4 must override global fold_threshold=1"
    );
}

#[test]
fn output_is_byte_stable() {
    let a = stdout(&run(&["outline", "--json", FIXTURE]));
    let b = stdout(&run(&["outline", "--json", FIXTURE]));
    assert_eq!(a, b);
    let cmd = printf_lines(25, "error: x", "");
    let c = stdout(&run(&["exec", "--json", &cmd]));
    let d = stdout(&run(&["exec", "--json", &cmd]));
    assert_eq!(c, d);
    let e = stdout(&run(&["deps", "--json", DEPS_RS]));
    let f = stdout(&run(&["deps", "--json", DEPS_RS]));
    assert_eq!(e, f);
}

#[test]
fn deps_rust_text_and_json() {
    let output = run(&["deps", DEPS_RS]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("5 imports"), "unexpected: {text}");
    assert!(
        text.contains("local     crate::lib::helper"),
        "unexpected: {text}"
    );
    assert!(
        text.contains("external  serde::Deserialize"),
        "unexpected: {text}"
    );
    assert!(text.contains("local     frontend"), "mod import: {text}");
    assert!(
        text.contains("local     frontend::api"),
        "use of a same-file mod must be local: {text}"
    );

    let json_output = run(&["deps", "--json", DEPS_RS]);
    let value: Value = serde_json::from_str(&stdout(&json_output)).expect("valid json");
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["tool"], "deps");
    assert_eq!(value["language"], "rust");
    let imports: Vec<(String, String)> = value["imports"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["target"].as_str().unwrap().to_string(),
                i["kind"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        imports,
        vec![
            ("frontend".to_string(), "local".to_string()),
            ("crate::lib::helper".to_string(), "local".to_string()),
            ("frontend::api".to_string(), "local".to_string()),
            ("serde::Deserialize".to_string(), "external".to_string()),
            (
                "std::collections::HashMap".to_string(),
                "external".to_string()
            ),
        ]
    );
    assert!(value["saved"]["percent"].as_u64().unwrap() > 0);
}

#[test]
fn deps_python_relative_imports_honor_ignore() {
    let dir = tmp_dir("deps-py-ignore");
    let config = dir.join("config.toml");
    std::fs::write(&config, "[paths]\nignore = [\"vendor_helpers\"]\n").expect("write config");

    let output = run(&[
        "deps",
        "--json",
        "--config",
        config.to_str().unwrap(),
        DEPS_PY,
    ]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let value: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(value["language"], "python");
    let imports: Vec<(String, String)> = value["imports"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["target"].as_str().unwrap().to_string(),
                i["kind"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        imports,
        vec![
            ("os".to_string(), "external".to_string()),
            ("sys".to_string(), "external".to_string()),
            ("numpy".to_string(), "external".to_string()),
            ("typing".to_string(), "external".to_string()),
            (".".to_string(), "local".to_string()),
            (".vendor_helpers".to_string(), "ignored".to_string()),
            (".models".to_string(), "local".to_string()),
            ("myproject.models".to_string(), "external".to_string()),
        ]
    );
}

#[test]
fn deps_go_local_via_cwd_existence_probe() {
    let dir = tmp_dir("deps-go-local");
    std::fs::create_dir_all(dir.join("localpkg/helper")).expect("create dir");
    std::fs::write(dir.join("localpkg/helper/help.go"), "package helper\n").expect("write file");
    let output = run_in(&dir, &["deps", "--json", DEPS_GO]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let value: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    let imports: Vec<(String, String)> = value["imports"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["target"].as_str().unwrap().to_string(),
                i["kind"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        imports,
        vec![
            ("fmt".to_string(), "external".to_string()),
            ("embed".to_string(), "external".to_string()),
            ("github.com/x/y".to_string(), "external".to_string()),
            ("localpkg/helper".to_string(), "local".to_string()),
        ]
    );
}

#[test]
fn deps_typescript_require_and_reexports() {
    let output = run(&["deps", "--json", DEPS_TS]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let value: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(value["language"], "typescript");
    let imports: Vec<(String, String)> = value["imports"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["target"].as_str().unwrap().to_string(),
                i["kind"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        imports,
        vec![
            ("express".to_string(), "external".to_string()),
            ("./helpers".to_string(), "local".to_string()),
            ("../lib/util".to_string(), "local".to_string()),
            ("./types".to_string(), "local".to_string()),
            ("path".to_string(), "external".to_string()),
            ("os".to_string(), "external".to_string()),
            ("./helpers2".to_string(), "local".to_string()),
        ]
    );
}

#[test]
fn deps_unsupported_extension_exits_2() {
    let path = tmp_dir("deps-unsupported").join("nope.xyz");
    std::fs::write(&path, "whatever").expect("write fixture");
    let output = run(&["deps", path.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("unsupported extension"));
}

#[test]
fn deps_slash_ignore_pattern_matches_relative_import() {
    // `src/vendor/*` must match a relative import resolved inside the file's
    // directory even though the fixture path is absolute.
    let root = tmp_dir("deps-slash-ignore");
    let vendor = root.join("proj/src/vendor");
    std::fs::create_dir_all(&vendor).expect("create dirs");
    let fixture = vendor.join("mod.ts");
    std::fs::write(&fixture, "import { helper } from \"./helper\";\n").expect("write fixture");
    let config = root.join("config.toml");
    std::fs::write(&config, "[paths]\nignore = [\"src/vendor/*\"]\n").expect("write config");

    let output = run_in(
        &root.join("proj"),
        &[
            "deps",
            "--json",
            "--config",
            config.to_str().unwrap(),
            fixture.to_str().unwrap(),
        ],
    );
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let value: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    let imports = value["imports"].as_array().unwrap();
    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0]["target"], "./helper");
    assert_eq!(imports[0]["kind"], "ignored");
}

#[test]
fn outline_signals_parse_failure() {
    let dir = tmp_dir("outline-parse-error");
    let path = dir.join("broken.ts");
    std::fs::write(&path, "fn one() {}\n").expect("write fixture");

    let output = run(&["outline", "--json", path.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(3),
        "parse failure must exit 3, stdout: {}",
        stdout(&output)
    );
    let value: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert!(
        value["parse_error"]["count"].as_u64().unwrap() > 0,
        "missing parse_error signal: {value}"
    );
    assert!(value["symbols"].as_array().is_some(), "no symbols key");

    let output = run(&["outline", path.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(3));
    assert!(
        stderr(&output).contains("parse failed"),
        "missing warning: {}",
        stderr(&output)
    );
    assert!(stdout(&output).contains("0 symbols"));

    let output = run(&["outline", "--json", JS_FIXTURE]);
    assert_eq!(output.status.code(), Some(0), "valid file must stay 0");
    let value: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert!(
        value.get("parse_error").is_none(),
        "no signal on clean parse"
    );
}

#[cfg(unix)]
#[test]
fn symlinked_project_config_is_followed() {
    use std::os::unix::fs::symlink;

    let root = tmp_dir("symlink-config");
    let real = root.join("real/.ctxctl");
    std::fs::create_dir_all(&real).expect("create real config dir");
    std::fs::write(
        real.join("config.toml"),
        "[exec]\nkeep = [\"golden-marker\"]\n",
    )
    .expect("write config");

    let work = root.join("work");
    std::fs::create_dir_all(&work).expect("create work dir");

    // Case 1: the config.toml file itself is a symlink.
    let link_dir = work.join(".ctxctl");
    std::fs::create_dir_all(&link_dir).expect("create link dir");
    symlink(real.join("config.toml"), link_dir.join("config.toml")).expect("link file");

    let mut out = String::new();
    for i in 0..40 {
        out.push_str(&format!("noise line {i}\n"));
    }
    out.push_str("golden-marker: this line must survive\n");
    let cmd = "cat testdata.txt";
    let data = work.join("testdata.txt");
    std::fs::write(&data, &out).expect("write data");

    let output = run_in(&work, &["exec", cmd]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert!(
        body(&output).contains("golden-marker"),
        "symlinked config.toml not applied: {}",
        stdout(&output)
    );

    // Case 2: the whole .ctxctl directory is a symlink.
    std::fs::remove_dir_all(work.join(".ctxctl")).expect("remove link dir");
    symlink(&real, work.join(".ctxctl")).expect("link dir");
    let output = run_in(&work, &["exec", cmd]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert!(
        body(&output).contains("golden-marker"),
        "symlinked .ctxctl dir not applied: {}",
        stdout(&output)
    );
}

// --- Input-file guardrails (CTX-0035) ---------------------------------------
// Every file-arg command must refuse non-regular files (a FIFO or char device
// hangs or exhausts memory on read) and oversized files (arbitrary tokenization
// cost) before touching the contents.

#[test]
fn directory_arg_is_rejected_as_not_a_regular_file() {
    let dir = tmp_dir("guardrail-dir");
    let d = dir.to_str().unwrap();
    for args in [
        vec!["outline", d],
        vec!["symbol", d, "--name", "add"],
        vec!["read", d, "--lines", "1-2"],
        vec!["deps", d],
    ] {
        let output = run(&args);
        assert_ne!(
            output.status.code(),
            Some(0),
            "{:?} must fail on a directory",
            args
        );
        let err = stderr(&output);
        assert!(err.contains(d), "must name the file: {err}");
        assert!(
            err.contains("not a regular file"),
            "must give reason: {err}"
        );
    }
}

#[cfg(unix)]
#[test]
fn character_device_arg_is_rejected_as_not_a_regular_file() {
    // /dev/null reads back empty (and /dev/zero would hang or exhaust
    // memory); only the metadata guard can reject a device file.
    let output = run(&["outline", "/dev/null"]);
    assert_ne!(output.status.code(), Some(0));
    let err = stderr(&output);
    assert!(err.contains("/dev/null"), "must name the file: {err}");
    assert!(
        err.contains("not a regular file"),
        "must give reason: {err}"
    );
}

#[test]
fn oversized_file_error_states_size_and_limit() {
    let dir = tmp_dir("guardrail-size");
    let file = dir.join("big.rs");
    let size = 21;
    std::fs::write(&file, "a".repeat(size)).expect("write oversized fixture");
    // Keep the cap tiny so CI never allocates real oversized fixtures.
    let config = dir.join("config.toml");
    std::fs::write(&config, "[limits]\nmax_file_bytes = 10\n").expect("write config");
    let output = run(&[
        "outline",
        "--config",
        config.to_str().unwrap(),
        file.to_str().unwrap(),
    ]);
    assert_ne!(output.status.code(), Some(0));
    let err = stderr(&output);
    assert!(err.contains("21"), "must state the actual size: {err}");
    assert!(err.contains("10"), "must state the limit: {err}");
    assert!(err.contains("max_file_bytes"), "must name the knob: {err}");
}

#[cfg(unix)]
#[test]
fn symlink_to_regular_source_is_accepted() {
    use std::os::unix::fs::symlink;

    let link = tmp_dir("guardrail-symlink").join("linked.rs");
    symlink(FIXTURE, &link).expect("link fixture");
    let output = run(&["outline", link.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert!(
        stdout(&output).contains("symbols"),
        "unexpected: {output:?}"
    );
}

#[test]
fn small_file_stays_under_configured_limit() {
    let dir = tmp_dir("guardrail-ok");
    let config = dir.join("config.toml");
    std::fs::write(&config, "[limits]\nmax_file_bytes = 1048576\n").expect("write config");
    let output = run(&["outline", "--config", config.to_str().unwrap(), FIXTURE]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert!(stdout(&output).contains("symbols"));
}
