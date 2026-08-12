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

fn rust_path() -> &'static Path {
    Path::new("sample.rs")
}

fn ts_path() -> &'static Path {
    Path::new("sample.ts")
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
fn unsupported_language_errors() {
    let path = Path::new("notes.txt");
    let res = ctx_symbol::outline("plain text", path);
    assert!(res.is_err());
}
