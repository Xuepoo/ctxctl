//! Tests for the markup/document backends: HTML, CSS/SCSS, Markdown.

use ctx_symbol::{compact_symbol, extract_symbols, parse, parse_error_count};
use std::path::Path;

fn kinds(path: &str, src: &str) -> Vec<(String, String, usize, usize)> {
    let parsed = parse(Path::new(path), src).expect("parses");
    extract_symbols(&parsed)
        .into_iter()
        .map(|s| (s.name, format!("{:?}", s.kind), s.start_line, s.end_line))
        .collect()
}

// --- HTML -------------------------------------------------------------------

#[test]
fn html_extracts_id_elements_and_skips_plain_tags() {
    let src = "<html>\n<body>\n  <div id=\"app\" class=\"x\">\n    <p>hi</p>\n  </div>\n  <span id=plain>bare</span>\n</body>\n</html>\n";
    let syms = kinds("a.html", src);
    assert_eq!(syms.len(), 2, "{syms:?}");
    assert_eq!(&syms[0].0, "app");
    assert_eq!(&syms[0].1, "Element");
    assert_eq!((syms[0].2, syms[0].3), (3, 5));
    // Bare attribute value form (`id=plain`) is supported too.
    assert_eq!(&syms[1].0, "plain");
}

#[test]
fn html_htm_extension_routes() {
    let parsed = parse(Path::new("page.htm"), "<div id=\"root\"></div>").unwrap();
    assert_eq!(parsed.language.name(), "html");
}

// --- CSS / SCSS -------------------------------------------------------------

#[test]
fn css_rules_including_nested_media() {
    let src = ".btn { color: red }\n@media (max-width: 9px) {\n  #main { margin: 0 }\n}\nh1, h2 { font-size: 2rem }\n";
    let syms = kinds("a.css", src);
    let names: Vec<&str> = syms.iter().map(|s| s.0.as_str()).collect();
    assert_eq!(names, [".btn", "#main", "h1, h2"]);
    for s in &syms {
        assert_eq!(s.1, "Rule");
    }
}

#[test]
fn scss_routes_through_css_backend() {
    let parsed = parse(Path::new("style.scss"), ".card { padding: 1rem }\n").unwrap();
    assert_eq!(parsed.language.name(), "css");
    assert_eq!(extract_symbols(&parsed).len(), 1);
}

// --- Markdown ---------------------------------------------------------------

const MD_SAMPLE: &str = "\
# Getting started

Intro text here.

## Install

Run the installer.

## Configure

Edit the config file.

### Advanced

Deep settings.
";

#[test]
fn markdown_headings_with_section_spans() {
    let syms = kinds("readme.md", MD_SAMPLE);
    let names: Vec<&str> = syms.iter().map(|s| s.0.as_str()).collect();
    assert_eq!(
        names,
        ["Getting started", "Install", "Configure", "Advanced"]
    );

    // Every heading is kind Heading.
    for s in &syms {
        assert_eq!(s.1, "Heading");
    }

    // A heading's slice spans its whole section: `# Getting started` runs
    // to EOF; a mid-level section ends just before its next sibling;
    // parents include their nested subsections.
    assert_eq!((syms[0].2, syms[0].3), (1, 15));
    assert_eq!((syms[1].2, syms[1].3), (5, 8));
    assert_eq!((syms[2].2, syms[2].3), (9, 15));
    assert_eq!((syms[3].2, syms[3].3), (13, 15));
}

#[test]
fn markdown_setext_headings_recognized() {
    let src = "Title\n=====\n\nbody\n";
    let syms = kinds("notes.markdown", src);
    assert_eq!(syms.len(), 1);
    assert_eq!(&syms[0].0, "Title");
    assert_eq!((syms[0].2, syms[0].3), (1, 4));
}

#[test]
fn markdown_section_slice_covers_chapter() {
    // The user-facing payoff: slicing by a chapter name yields the whole
    // chapter text, not just the title line.
    let parsed = parse(Path::new("doc.md"), MD_SAMPLE).unwrap();
    let symbols = extract_symbols(&parsed);
    let configure = symbols
        .iter()
        .find(|s| s.name == "Configure")
        .expect("Configure present");
    let slice = &parsed.source[configure.byte_range.clone()];
    assert!(slice.contains("## Configure"));
    assert!(slice.contains("Deep settings."));
}

#[test]
fn markdown_leaf_section_compact_passes_prose_through() {
    // A leaf section (no subheadings) must pass through compact unchanged:
    // its `#` heading is markdown syntax, not a C preprocessor directive,
    // so the macro fold branch must never fire on it.
    let src = "## Install\n\nRun the installer.\n\nThen configure it.\n";
    let parsed = parse(Path::new("doc.md"), src).expect("parses");
    let install = extract_symbols(&parsed)
        .into_iter()
        .find(|s| s.name == "Install")
        .expect("Install present");
    let raw = src[install.byte_range.clone()].to_string();
    let compact = compact_symbol(&parsed, &install);
    assert_eq!(
        compact, raw,
        "leaf section passes through unchanged: {compact:?}"
    );
    assert!(
        !compact.contains("lines omitted]"),
        "no bare fold marker: {compact:?}"
    );
    assert!(compact.contains("Run the installer."), "prose kept");
    // The compact view is still valid markdown.
    let reparsed = parse(Path::new("doc.md"), &compact).unwrap();
    assert_eq!(
        parse_error_count(&reparsed),
        0,
        "compact re-parses: {compact:?}"
    );
}

#[test]
fn markdown_parent_section_compact_passes_through() {
    // Sections with subheadings already pass through; keep it that way so
    // both shapes stay consistent.
    let parsed = parse(Path::new("doc.md"), MD_SAMPLE).unwrap();
    for name in ["Getting started", "Configure"] {
        let sym = extract_symbols(&parsed)
            .into_iter()
            .find(|s| s.name == name)
            .expect("section present");
        let raw = MD_SAMPLE[sym.byte_range.clone()].to_string();
        let compact = compact_symbol(&parsed, &sym);
        assert_eq!(compact, raw, "{name} passes through unchanged: {compact:?}");
    }
}

#[test]
fn html_self_closing_elements_are_anchors_too() {
    let src = "<img id=\"logo\" src=\"x.png\"/>\n<br><p id=\"tail\">end</p>\n";
    let syms = kinds("icons.html", &src);
    let names: Vec<&str> = syms.iter().map(|s| s.0.as_str()).collect();
    assert_eq!(names, ["logo", "tail"]);
}
