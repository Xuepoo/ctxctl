//! Degenerate-input corpus for the symbol engine: BOM-prefixed sources,
//! CRLF line endings across languages, empty files, and heavily ERROR-node
//! trees (garbage syntax). Nothing here may panic, and every output must be
//! a deterministic function of the input (CTX-0040).

use std::path::Path;

use ctx_symbol::{
    Symbol, compact_symbol, extract_symbols, outline, parse, parse_error_count, slice_by_name,
};

const BOM: &str = "\u{feff}";

fn path_of(name: &str) -> &'static Path {
    let leaked: &'static str = Box::leak(name.to_string().into_boxed_str());
    Path::new(leaked)
}

/// Two independent runs must agree exactly (symbols, order, ranges, docs).
fn assert_deterministic(source: &str, path: &Path) {
    let a = outline(source, path).expect("first parse");
    let b = outline(source, path).expect("second parse");
    assert_eq!(
        format!("{a:?}"),
        format!("{b:?}"),
        "nondeterministic symbols"
    );
    assert_eq!(
        parse_error_count(&parse(path, source).unwrap()),
        parse_error_count(&parse(path, source).unwrap()),
        "nondeterministic error count"
    );
}

/// Every byte range must land inside the original source and slice back out
/// as valid UTF-8.
fn assert_slices_in_bounds<'a>(source: &str, symbols: &'a [Symbol]) {
    for s in symbols {
        let bytes = source
            .as_bytes()
            .get(s.byte_range.clone())
            .unwrap_or_else(|| panic!("range {:?} of {} out of bounds", s.byte_range, s.name));
        std::str::from_utf8(bytes)
            .unwrap_or_else(|e| panic!("invalid utf-8 slice for {}: {e}", s.name));
        assert!(
            s.end_line >= s.start_line && s.start_line >= 1,
            "bad line span for {}: {}-{}",
            s.name,
            s.start_line,
            s.end_line
        );
    }
}

// --- BOM-prefixed sources ----------------------------------------------------

const BOM_RUST_SRC: &str =
    "\u{feff}/// Adds two numbers.\npub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n";

#[test]
fn bom_prefixed_rust_ranges_exclude_the_bom() {
    let source = BOM_RUST_SRC;
    let p = path_of("bom.rs");
    let symbols = outline(source, p).expect("parses despite BOM");
    assert!(
        !symbols.is_empty(),
        "a leading BOM must not suppress extraction"
    );
    for s in &symbols {
        assert!(
            s.byte_range.start >= BOM.len(),
            "{} range starts at {}, inside the {}-byte BOM",
            s.name,
            s.byte_range.start,
            BOM.len()
        );
        let text = std::str::from_utf8(&source.as_bytes()[s.byte_range.clone()]).unwrap();
        assert!(
            !text.contains('\u{feff}'),
            "{} slice includes the BOM: {text:?}",
            s.name
        );
    }
    assert_slices_in_bounds(source, &symbols);
    assert_deterministic(source, p);
}

#[test]
fn bom_prefixed_slice_by_name_returns_clean_source() {
    let slice = slice_by_name(BOM_RUST_SRC, path_of("bom.rs"), "add").expect("add found");
    assert!(slice.contains("pub fn add(a: i32, b: i32)"), "{slice:?}");
    assert!(
        !slice.contains('\u{feff}'),
        "BOM leaked into slice: {slice:?}"
    );
}

#[test]
fn bom_prefixed_sources_across_languages_are_tolerated() {
    // Each snippet must still yield at least one symbol after the BOM.
    let cases: &[(&str, String)] = &[
        ("bom.py", format!("{BOM}def add(a, b):\n    return a + b\n")),
        (
            "bom.js",
            format!("{BOM}export function add(a, b) {{\n  return a + b;\n}}\n"),
        ),
        (
            "bom.go",
            format!("{BOM}func Add(a, b int) int {{\n\treturn a + b\n}}\n"),
        ),
        (
            "bom.c",
            format!("{BOM}int add(int a, int b) {{\n    return a + b;\n}}\n"),
        ),
        (
            "bom.ts",
            format!(
                "{BOM}export function add(a: number, b: number): number {{\n  return a + b;\n}}\n"
            ),
        ),
    ];
    for (name, src) in cases {
        let p = path_of(name);
        let parsed = parse(p, src).unwrap_or_else(|e| panic!("{name}: {e}"));
        let symbols = extract_symbols(&parsed);
        assert!(
            !symbols.is_empty(),
            "{name}: a leading BOM must not suppress extraction"
        );
        for s in &symbols {
            assert!(
                s.byte_range.start >= BOM.len(),
                "{name}: {} range starts inside the BOM",
                s.name
            );
        }
        assert_slices_in_bounds(src, &symbols);
        assert_deterministic(src, p);
    }
}

// --- CRLF sources across languages -------------------------------------------

/// One compact multi-definition snippet per language, all `\r\n` line
/// endings. Bodies are long enough that compaction folds them.
const CRLF_CASES: &[(&str, &str)] = &[
    (
        "crlf.rs",
        "pub fn add(a: i32, b: i32) -> i32 {\r\n    let s = a + b;\r\n    let t = a * b;\r\n    s + t\r\n}\r\n\r\npub const ANSWER: i32 = 42;\r\n",
    ),
    (
        "crlf.py",
        "def add(a, b):\r\n    s = a + b\r\n    t = a * b\r\n    return s + t\r\n\r\nclass Point:\r\n    def norm(self):\r\n        return 1.0\r\n",
    ),
    (
        "crlf.js",
        "export function add(a, b) {\r\n  const s = a + b;\r\n  const t = a * b;\r\n  return s + t;\r\n}\r\n\r\nconst MAX = 3;\r\n",
    ),
    (
        "crlf.go",
        "func Add(a, b int) int {\r\n\ts := a + b\r\n\tt := a * b\r\n\treturn s + t\r\n}\r\n",
    ),
    (
        "crlf.c",
        "int add(int a, int b)\r\n{\r\n    int s = a + b;\r\n    int t = a * b;\r\n    return s + t;\r\n}\r\n",
    ),
    (
        "crlf.java",
        "public class Point {\r\n    private double x;\r\n\r\n    public double norm() {\r\n        return x;\r\n    }\r\n}\r\n",
    ),
];

#[test]
fn crlf_sources_across_languages_extract_and_compact_deterministically() {
    for (name, src) in CRLF_CASES {
        let p = path_of(name);
        let parsed = parse(p, src).unwrap_or_else(|e| panic!("{name}: {e}"));
        let symbols = extract_symbols(&parsed);
        assert!(!symbols.is_empty(), "{name}: CRLF suppressed extraction");
        assert_slices_in_bounds(src, &symbols);
        for s in &symbols {
            let compact = compact_symbol(&parsed, s);
            assert_eq!(
                compact,
                compact_symbol(&parsed, s),
                "{name}: nondeterministic compaction of {}",
                s.name
            );
            // A pure-CRLF source must stay CRLF-only in compact output: no
            // bare-LF terminators mixed in.
            assert_eq!(
                compact.matches('\n').count(),
                compact.matches("\r\n").count(),
                "{name}: {} compact mixes LF into CRLF output: {compact:?}",
                s.name
            );
        }
        assert_deterministic(src, p);
    }
}

#[test]
fn crlf_ranges_never_point_at_a_lone_cr() {
    // Slicing by byte range must respect the two-byte terminator: no slice
    // may end on a dangling `\r`, and none may start with one mid-line.
    for (name, src) in CRLF_CASES {
        let p = path_of(name);
        let symbols = outline(src, p).unwrap_or_else(|e| panic!("{name}: {e}"));
        for s in &symbols {
            let text = std::str::from_utf8(&src.as_bytes()[s.byte_range.clone()]).unwrap();
            assert!(
                !text.starts_with('\r') && !text.ends_with('\r'),
                "{name}: {} slice has a dangling CR: {text:?}",
                s.name
            );
        }
    }
}

// --- Empty files --------------------------------------------------------------

#[test]
fn empty_files_yield_no_symbols_across_languages() {
    for ext in [
        "rs", "py", "js", "ts", "go", "c", "cpp", "cs", "java", "rb", "lua", "md", "html", "css",
    ] {
        let p = path_of(&format!("empty.{ext}"));
        let parsed = parse(p, "").unwrap_or_else(|e| panic!("{ext}: {e}"));
        let symbols = extract_symbols(&parsed);
        assert!(symbols.is_empty(), "{ext}: empty file produced {symbols:?}");
        assert_eq!(
            parse_error_count(&parsed),
            0,
            "{ext}: empty file has errors"
        );
        assert_deterministic("", p);
    }
}

#[test]
fn whitespace_only_files_behave_like_empty_files() {
    for body in ["\n", "\r\n\r\n", "   \n\t\n"] {
        let p = path_of("blank.rs");
        let symbols = outline(body, p).expect("parses");
        assert!(symbols.is_empty(), "{body:?}: produced {symbols:?}");
        assert_deterministic(body, p);
    }
}

#[test]
fn empty_file_slice_by_name_reports_not_found() {
    let err = slice_by_name("", path_of("empty.rs"), "anything").expect_err("must not find");
    assert!(
        err.to_string().contains("symbol not found"),
        "unexpected error: {err}"
    );
    // Same contract for whitespace-only files, deterministically.
    for body in ["\n", " \t\n"] {
        let err = slice_by_name(body, path_of("empty.rs"), "anything").expect_err("must not find");
        assert!(err.to_string().contains("symbol not found"), "{err}");
    }
}

// --- Heavily ERROR-node trees --------------------------------------------------

/// Garbage inputs that must produce ERROR/MISSING nodes without panicking.
const GARBAGE: &[&str] = &[
    r#"]}}}) ((( "]""#,
    r#"@@@@ $$$$ ^^^^ &&&&&"#,
    r#""""'''{{{ |||||"#,
    "(((((((((",
    r#"fn fn fn fn }}}}{{{{"#,
    r#"\\<<<>>>???"#,
];

#[test]
fn garbage_trees_never_panic_and_stay_deterministic() {
    for ext in ["rs", "py", "js", "go", "c"] {
        for (i, junk) in GARBAGE.iter().enumerate() {
            let name = format!("garbage{i}.{ext}");
            let p = path_of(&name);
            let parsed = parse(p, junk).unwrap_or_else(|e| panic!("{name}: {e}"));
            let _ = parse_error_count(&parsed); // must not panic
            let symbols = extract_symbols(&parsed);
            assert_slices_in_bounds(junk, &symbols);
            for s in &symbols {
                let _ = compact_symbol(&parsed, s); // must not panic
            }
            assert_deterministic(junk, p);
        }
    }
}

#[test]
fn garbage_trees_actually_report_errors() {
    // The corpus above must really be garbage: every grammar reports
    // ERROR/MISSING nodes for each junk input (otherwise these tests would
    // pass vacuously).
    for ext in ["rs", "py", "js", "go", "c"] {
        for (i, junk) in GARBAGE.iter().enumerate() {
            let name = format!("garbage{i}.{ext}");
            let parsed = parse(path_of(&name), junk).expect("parses with recovery");
            assert!(
                parse_error_count(&parsed) > 0,
                "{name}: {junk:?} parsed cleanly; corpus is not degenerate"
            );
        }
    }
}

#[test]
fn garbage_after_valid_prefix_keeps_the_valid_symbols() {
    // Error recovery must not eat the healthy part of a file.
    let src = "pub fn valid_one(a: i32) -> i32 {\n    a * 2\n}\n\n@@@ }}} broken garbage (((\n";
    let p = path_of("mixed.rs");
    let parsed = parse(p, src).expect("parses with recovery");
    assert!(
        parse_error_count(&parsed) > 0,
        "the garbage tail must be reported"
    );
    let symbols = extract_symbols(&parsed);
    let one = symbols
        .iter()
        .find(|s| s.name == "valid_one")
        .expect("valid symbol survives error recovery");
    let text = std::str::from_utf8(&src.as_bytes()[one.byte_range.clone()]).unwrap();
    assert!(text.contains("a * 2"), "unexpected slice: {text:?}");
    assert!(!text.contains("@@@"), "slice swallowed garbage: {text:?}");
    assert_slices_in_bounds(src, &symbols);
    assert_deterministic(src, p);
}

#[test]
fn bom_only_file_is_handled_without_panicking() {
    // A file containing nothing but the BOM: no definitions can exist, but
    // parsing, extraction, and slicing must all stay calm and stable.
    let p = path_of("bom-only.rs");
    let symbols = outline(BOM, p).expect("parses");
    assert!(symbols.is_empty(), "BOM-only file produced {symbols:?}");
    let err = slice_by_name(BOM, p, "anything").expect_err("nothing to find");
    assert!(err.to_string().contains("symbol not found"), "{err}");
    assert_deterministic(BOM, p);
}
