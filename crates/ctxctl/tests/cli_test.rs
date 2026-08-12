//! Integration tests for the ctxctl binary: four commands, JSON contract,
//! exit codes (cli-contract.md §5), config resolution (§6), and byte stability.

use serde_json::Value;
use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_ctxctl");
const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sample.rs");
const PY_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sample.py");
const GO_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sample.go");

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
        text.contains("pub fn add(a: i32, b: i32) -> i32 {"),
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
    assert!(value["saved"]["percent"].as_u64().unwrap() > 0);
    assert!(
        value["saved"]["tokens_before"].as_u64().unwrap()
            > value["saved"]["tokens_after"].as_u64().unwrap()
    );
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
    assert!(text.contains("class Point:"), "unexpected: {text}");
    assert!(
        text.contains("def add(a: int, b: int) -> int:"),
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
            .ends_with("pub fn add(a: i32, b: i32) -> i32 {"),
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

#[test]
fn exec_invalid_keep_pattern_fails() {
    let output = run(&["exec", "--keep", "[", "true"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("invalid regex pattern"));
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
}
