//! Unit tests for the ctx-symbol engine: extraction + slicing.

use std::path::Path;

const RUST_SAMPLE: &str = r#"
/// A handler for incoming requests.
pub struct RequestHandler {
    db: Database,
}

impl RequestHandler {
    /// Process a single request by id.
    pub async fn handle_request(&self, id: u64) -> Result<String, Error> {
        let row = self.db.get(id).await?;
        Ok(row.to_string())
    }
}

/// Standalone helper function.
pub fn parse_id(raw: &str) -> u64 {
    raw.trim().parse().unwrap_or(0)
}

const MAX_RETRIES: u32 = 3;
"#;

const TS_SAMPLE: &str = r#"
/** A user entity. */
export interface User {
  id: number;
  name: string;
}

export function formatName(user: User): string {
  return user.name.trim();
}

class Database {
  /** Connect to the store. */
  connect(): void {
    console.log('connecting');
  }
}
"#;

const PY_SAMPLE: &str = r#"
"""Module docstring, not a symbol doc."""

class Point:
    """A point in 2D space."""

    def __init__(self, x: float, y: float) -> None:
        self.x = x
        self.y = y

    def norm(self) -> float:
        return (self.x * self.x + self.y * self.y) ** 0.5

# A standalone helper.
def add(a: int, b: int) -> int:
    return a + b
"#;

const GO_SAMPLE: &str = r#"
// Point is a 2D point.
type Point struct {
	X float64
	Y float64
}

// Add sums two integers.
func Add(a, b int) int {
	return a + b
}

// Norm returns the distance from the origin.
func (p *Point) Norm() float64 {
	return p.X*p.X + p.Y*p.Y
}

const MAX_RETRIES = 3

var DefaultPoint = Point{X: 1, Y: 2}
"#;

fn rust_path() -> &'static Path {
    Path::new("sample.rs")
}

fn ts_path() -> &'static Path {
    Path::new("sample.ts")
}

fn py_path() -> &'static Path {
    Path::new("sample.py")
}

fn go_path() -> &'static Path {
    Path::new("sample.go")
}

#[test]
fn rust_extracts_struct_and_functions() {
    let symbols = ctx_symbol::outline(RUST_SAMPLE, rust_path()).unwrap();
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"RequestHandler"),
        "missing struct, got {names:?}"
    );
    assert!(
        names.contains(&"handle_request"),
        "missing method, got {names:?}"
    );
    assert!(names.contains(&"parse_id"), "missing fn, got {names:?}");
    assert!(
        names.contains(&"MAX_RETRIES"),
        "missing const, got {names:?}"
    );
}

#[test]
fn rust_byte_range_slices_original_text() {
    let symbols = ctx_symbol::outline(RUST_SAMPLE, rust_path()).unwrap();
    let handle = symbols.iter().find(|s| s.name == "handle_request").unwrap();
    let slice = &RUST_SAMPLE.as_bytes()[handle.byte_range.clone()];
    let text = std::str::from_utf8(slice).unwrap();
    assert!(text.contains("pub async fn handle_request"));
    assert!(text.contains("Ok(row.to_string())"));
}

#[test]
fn rust_signature_is_first_line() {
    let symbols = ctx_symbol::outline(RUST_SAMPLE, rust_path()).unwrap();
    let handle = symbols.iter().find(|s| s.name == "handle_request").unwrap();
    assert!(
        handle.signature.contains("pub async fn handle_request"),
        "sig: {}",
        handle.signature
    );
    // line numbers are 1-based
    assert!(handle.start_line >= 1);
}

#[test]
fn ts_extracts_interface_function_class() {
    let symbols = ctx_symbol::outline(TS_SAMPLE, ts_path()).unwrap();
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"User"), "missing interface: {names:?}");
    assert!(names.contains(&"formatName"), "missing fn: {names:?}");
    assert!(names.contains(&"Database"), "missing class: {names:?}");
    assert!(names.contains(&"connect"), "missing method: {names:?}");
}

#[test]
fn slice_by_name_returns_just_the_function() {
    let slice = ctx_symbol::slice_by_name(RUST_SAMPLE, rust_path(), "parse_id").unwrap();
    assert!(slice.contains("pub fn parse_id"));
    assert!(slice.contains("raw.trim().parse()"));
    // must NOT include the struct body that precedes it
    assert!(!slice.contains("RequestHandler"));
}

#[test]
fn python_extracts_classes_and_functions() {
    let symbols = ctx_symbol::outline(PY_SAMPLE, py_path()).unwrap();
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["Point", "__init__", "norm", "add"]);
    let point = symbols.iter().find(|s| s.name == "Point").unwrap();
    assert_eq!(point.kind, ctx_symbol::SymbolKind::Class);
    assert_eq!(symbols[0].kind, ctx_symbol::SymbolKind::Class);
    assert_eq!(symbols[3].kind, ctx_symbol::SymbolKind::Function);
}

#[test]
fn python_docstrings_are_attached() {
    let symbols = ctx_symbol::outline(PY_SAMPLE, py_path()).unwrap();
    // The module docstring documents the module, not the first symbol.
    let point = symbols.iter().find(|s| s.name == "Point").unwrap();
    assert_eq!(point.doc_comment, None);
    // The class body docstring sits above `__init__`.
    let init = symbols.iter().find(|s| s.name == "__init__").unwrap();
    assert_eq!(init.doc_comment.as_deref(), Some("A point in 2D space."));
    // `# comment` above a def is attached too
    let add = symbols.iter().find(|s| s.name == "add").unwrap();
    assert_eq!(add.doc_comment.as_deref(), Some("A standalone helper."));
}

#[test]
fn python_byte_range_slices_original_text() {
    let symbols = ctx_symbol::outline(PY_SAMPLE, py_path()).unwrap();
    let norm = symbols.iter().find(|s| s.name == "norm").unwrap();
    let slice = &PY_SAMPLE.as_bytes()[norm.byte_range.clone()];
    let text = std::str::from_utf8(slice).unwrap();
    assert!(text.contains("def norm(self)"));
    assert!(text.contains("self.x * self.x"));
    assert!(!text.contains("class Point"));
}

#[test]
fn go_extracts_funcs_methods_types_and_specs() {
    let symbols = ctx_symbol::outline(GO_SAMPLE, go_path()).unwrap();
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["Point", "Add", "Norm", "MAX_RETRIES", "DefaultPoint"]
    );
    assert_eq!(symbols[0].kind, ctx_symbol::SymbolKind::Type);
    assert_eq!(symbols[1].kind, ctx_symbol::SymbolKind::Function);
    assert_eq!(symbols[2].kind, ctx_symbol::SymbolKind::Method);
    assert_eq!(symbols[3].kind, ctx_symbol::SymbolKind::Const);
    assert_eq!(symbols[4].kind, ctx_symbol::SymbolKind::Variable);
    let norm = &symbols[2];
    assert_eq!(
        norm.doc_comment.as_deref(),
        Some("Norm returns the distance from the origin.")
    );
    assert!(norm.signature.contains("func (p *Point) Norm() float64"));
    let slice = &GO_SAMPLE.as_bytes()[norm.byte_range.clone()];
    let text = std::str::from_utf8(slice).unwrap();
    assert!(text.contains("p.X*p.X + p.Y*p.Y"));
    assert!(!text.contains("const MAX_RETRIES"));
}

#[test]
fn unsupported_language_errors() {
    let path = Path::new("notes.txt");
    let res = ctx_symbol::outline("plain text", path);
    assert!(res.is_err());
}
