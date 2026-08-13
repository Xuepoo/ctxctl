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

const PY_DECORATED_SAMPLE: &str = r#"
import functools

def deco(f):
    return f

@deco
def decorated(x):
    return x

"""Processes things."""
@deco
def processed(y):
    return y
"#;

const JS_SAMPLE: &str = r#"
// Fixture for the javascript backend.
import express from "express";
import { helper } from "./helpers";
const os = require("os");

/** A user entity. */
class User {
  /** Say hello. */
  greet() {
    return "hi";
  }
}

export function formatName(user) {
  return user.name.trim();
}

const MAX_RETRIES = 3;
var legacy = 1;
"#;

const JAVA_SAMPLE: &str = r#"
package com.example.app;

import java.util.List;
import static java.lang.Math.PI;
import com.example.util.Helper;

/** A 2D point. */
public class Point {
    private double x;
    private double y;

    /** Distance from the origin. */
    public double norm() {
        return Math.sqrt(x * x + y * y);
    }
}

/** A repository interface. */
public interface Repo {
    List<String> all();
}

public record Pair(int a, int b) {}

public enum Status { ON, OFF }

public class App {
    public App() {}
    public static void main(String[] args) {}
}
"#;

const C_SAMPLE: &str = r#"
// Fixture for the C backend.
#include <stdio.h>
#include "util.h"

#define MAX_RETRIES 3

/** A 2D point. */
typedef struct Point {
    double x;
    double y;
} Point;

/** Adds two numbers. */
int add(int a, int b) {
    return a + b;
}

enum Color { RED, GREEN };

static int helper(int v) {
    return v * 2;
}
"#;

const CPP_SAMPLE: &str = r#"
// Fixture for the C++ backend.
#include <vector>
#include "local.hpp"

#define VERSION 2

using namespace std;
using std::vector;
using Alias = int;

namespace app {
/** A widget. */
template <typename T>
class Widget {
public:
    T value;
    /** Resets the widget. */
    void reset() { value = T(); }
};
}

/** Computes the sum. */
template <typename T>
T sum(T a, T b) {
    return a + b;
}
"#;

const CSHARP_SAMPLE: &str = r#"
// Fixture for the C# backend.
using System;
using System.Collections.Generic;
using static System.Math;

namespace Demo.App {
    /// A 2D point.
    public class Point {
        private double x;
        private double y;

        /// Distance from the origin.
        public double Norm() {
            return Sqrt(x * x + y * y);
        }
    }

    public interface IRepo {
        List<string> All();
    }

    public record Pair(int A, int B);

    public enum Status { On, Off }

    public struct Vector2 {
        public double X;
    }
}
"#;

const RUBY_SAMPLE: &str = r#"
# Fixture for the ruby backend.
require 'json'
require_relative 'helpers'
require './local'

# A user entity.
class User
  # Says hello.
  def greet(name)
    "hi #{name}"
  end

  def self.build
    User.new
  end
end

# Computes a sum.
def add(a, b)
  a + b
end

module Utils
  def normalize(x)
    x.to_f
  end
end
"#;

const LUA_SAMPLE: &str = r#"
-- Fixture for the lua backend.
local json = require "json"
local helpers = require("./helpers")

-- A counter.
local Counter = {}
Counter.__index = Counter

function Counter.new()
  return setmetatable({ n = 0 }, Counter)
end

-- Adds two numbers.
local function add(a, b)
  return a + b
end

function Counter:increment()
  self.n = self.n + 1
end

local MAX_RETRIES = 3
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

fn js_path() -> &'static Path {
    Path::new("sample.js")
}

fn java_path() -> &'static Path {
    Path::new("Sample.java")
}

fn c_path() -> &'static Path {
    Path::new("sample.c")
}

fn cpp_path() -> &'static Path {
    Path::new("sample.cpp")
}

fn csharp_path() -> &'static Path {
    Path::new("Sample.cs")
}

fn ruby_path() -> &'static Path {
    Path::new("sample.rb")
}

fn lua_path() -> &'static Path {
    Path::new("sample.lua")
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
fn compact_is_smaller_and_parseable() {
    let parsed = ctx_symbol::parse(rust_path(), RUST_SAMPLE).unwrap();
    let symbols = ctx_symbol::extract_symbols(&parsed);
    let handle = symbols.iter().find(|s| s.name == "handle_request").unwrap();
    let raw = &RUST_SAMPLE.as_bytes()[handle.byte_range.clone()];
    let compact = ctx_symbol::compact_symbol(&parsed, handle);
    assert!(compact.len() < raw.len(), "compact must be smaller");
    assert!(compact.starts_with("pub async fn handle_request"));
    assert!(compact.contains("// ... ["));
    assert!(compact.trim_end().ends_with('}'));
    assert!(compact.ends_with('\n'), "compact must end with a newline");
    // Re-parse without errors: the fold marker is a line comment.
    let reparsed = ctx_symbol::parse(rust_path(), &compact).unwrap();
    assert!(
        !reparsed.tree.root_node().has_error(),
        "compact must re-parse: {compact}"
    );
}

#[test]
fn compact_keeps_python_decorators_and_signature() {
    let parsed = ctx_symbol::parse(py_path(), PY_DECORATED_SAMPLE).unwrap();
    let symbols = ctx_symbol::extract_symbols(&parsed);
    let decorated = symbols.iter().find(|s| s.name == "decorated").unwrap();
    let compact = ctx_symbol::compact_symbol(&parsed, decorated);
    assert!(compact.starts_with("@deco"), "decorator kept: {compact}");
    assert!(
        compact.contains("def decorated(x):"),
        "signature kept: {compact}"
    );
    assert!(compact.contains("# ... ["), "python marker: {compact}");
    let reparsed = ctx_symbol::parse(py_path(), &compact).unwrap();
    assert!(
        !reparsed.tree.root_node().has_error(),
        "compact must re-parse: {compact}"
    );
}

#[test]
fn c_extracts_functions_types_enums_consts_and_fields() {
    let symbols = ctx_symbol::outline(C_SAMPLE, c_path()).unwrap();
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["MAX_RETRIES", "Point", "x", "y", "add", "Color", "helper"]
    );
    assert_eq!(symbols[0].kind, ctx_symbol::SymbolKind::Const);
    assert_eq!(symbols[1].kind, ctx_symbol::SymbolKind::Type);
    assert_eq!(symbols[4].kind, ctx_symbol::SymbolKind::Function);
    assert_eq!(symbols[5].kind, ctx_symbol::SymbolKind::Enum);
    assert_eq!(symbols[1].doc_comment.as_deref(), Some("A 2D point."));
    // the typedef captures Point once (no duplicate struct_specifier)
    assert_eq!(names.iter().filter(|n| **n == "Point").count(), 1);
    // slicing works through the declarator chain
    let add = symbols.iter().find(|s| s.name == "add").unwrap();
    let slice = &C_SAMPLE.as_bytes()[add.byte_range.clone()];
    let text = std::str::from_utf8(slice).unwrap();
    assert!(text.contains("int add(int a, int b)"));
    assert!(text.contains("return a + b;"));
    assert!(!text.contains("enum Color"));
}

#[test]
fn cpp_extracts_classes_namespaces_and_templates() {
    let symbols = ctx_symbol::outline(CPP_SAMPLE, cpp_path()).unwrap();
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["VERSION", "app", "Widget", "value", "reset", "sum"]
    );
    assert_eq!(symbols[2].kind, ctx_symbol::SymbolKind::Class);
    assert_eq!(symbols[3].kind, ctx_symbol::SymbolKind::Variable);
    assert_eq!(symbols[5].kind, ctx_symbol::SymbolKind::Function);
    // template headers are included in the range (like python decorators)
    let widget = symbols.iter().find(|s| s.name == "Widget").unwrap();
    let slice = &CPP_SAMPLE.as_bytes()[widget.byte_range.clone()];
    let text = std::str::from_utf8(slice).unwrap();
    assert!(
        text.starts_with("template <typename T>"),
        "template header must be in the slice: {text}"
    );
    assert!(text.contains("class Widget"));
    assert_eq!(widget.doc_comment.as_deref(), Some("A widget."));
    // compact view keeps the template header and re-parses
    let parsed = ctx_symbol::parse(cpp_path(), CPP_SAMPLE).unwrap();
    let compact = ctx_symbol::compact_symbol(&parsed, widget);
    assert!(compact.starts_with("template <typename T>"));
    let reparsed = ctx_symbol::parse(cpp_path(), &compact).unwrap();
    assert!(!reparsed.tree.root_node().has_error(), "compact: {compact}");
}

#[test]
fn csharp_extracts_types_methods_properties_and_fields() {
    let symbols = ctx_symbol::outline(CSHARP_SAMPLE, csharp_path()).unwrap();
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "Demo.App", "Point", "x", "y", "Norm", "IRepo", "All", "Pair", "Status", "Vector2",
            "X",
        ]
    );
    assert_eq!(symbols[0].kind, ctx_symbol::SymbolKind::Type); // namespace
    assert_eq!(symbols[1].kind, ctx_symbol::SymbolKind::Class);
    assert_eq!(symbols[2].kind, ctx_symbol::SymbolKind::Variable);
    assert_eq!(symbols[5].kind, ctx_symbol::SymbolKind::Interface);
    assert_eq!(symbols[7].kind, ctx_symbol::SymbolKind::Class); // record
    assert_eq!(symbols[8].kind, ctx_symbol::SymbolKind::Enum);
    assert_eq!(symbols[9].kind, ctx_symbol::SymbolKind::Struct);
    assert_eq!(symbols[1].doc_comment.as_deref(), Some("A 2D point."));
    let norm = symbols.iter().find(|s| s.name == "Norm").unwrap();
    let slice = &CSHARP_SAMPLE.as_bytes()[norm.byte_range.clone()];
    let text = std::str::from_utf8(slice).unwrap();
    assert!(text.contains("Sqrt(x * x + y * y)"));
    assert!(!text.contains("class Point"));
}

#[test]
fn ruby_extracts_methods_classes_modules_and_docs() {
    let symbols = ctx_symbol::outline(RUBY_SAMPLE, ruby_path()).unwrap();
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["User", "greet", "build", "add", "Utils", "normalize"]
    );
    assert_eq!(symbols[0].kind, ctx_symbol::SymbolKind::Class);
    assert_eq!(symbols[1].kind, ctx_symbol::SymbolKind::Function);
    assert_eq!(symbols[4].kind, ctx_symbol::SymbolKind::Type); // module
    assert_eq!(symbols[0].doc_comment.as_deref(), Some("A user entity."));
    let greet = symbols.iter().find(|s| s.name == "greet").unwrap();
    let slice = &RUBY_SAMPLE.as_bytes()[greet.byte_range.clone()];
    let text = std::str::from_utf8(slice).unwrap();
    assert!(text.contains("def greet(name)"));
    assert!(text.contains("\"hi #{name}\""));
    assert!(!text.contains("class User"));
}

#[test]
fn lua_extracts_functions_and_variables() {
    let symbols = ctx_symbol::outline(LUA_SAMPLE, lua_path()).unwrap();
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "json",
            "helpers",
            "Counter",
            "Counter.new",
            "add",
            "Counter:increment",
            "MAX_RETRIES",
        ]
    );
    assert_eq!(symbols[0].kind, ctx_symbol::SymbolKind::Variable);
    assert_eq!(symbols[4].kind, ctx_symbol::SymbolKind::Function);
    assert_eq!(symbols[4].doc_comment.as_deref(), Some("Adds two numbers."));
    // `function Counter.new()` -> name keeps the dotted path
    let new = symbols.iter().find(|s| s.name == "Counter.new").unwrap();
    let slice = &LUA_SAMPLE.as_bytes()[new.byte_range.clone()];
    let text = std::str::from_utf8(slice).unwrap();
    assert!(text.contains("function Counter.new()"));
    // compact view uses -- comments and keeps the end closer
    let parsed = ctx_symbol::parse(lua_path(), LUA_SAMPLE).unwrap();
    let add = symbols.iter().find(|s| s.name == "add").unwrap();
    let compact = ctx_symbol::compact_symbol(&parsed, add);
    assert!(compact.contains("-- ... ["), "lua marker: {compact}");
    assert!(
        compact.trim_end().ends_with("end"),
        "end closer kept: {compact}"
    );
    let reparsed = ctx_symbol::parse(lua_path(), &compact).unwrap();
    assert!(!reparsed.tree.root_node().has_error(), "compact: {compact}");
}

#[test]
fn c_globals_pointers_and_prototypes() {
    let src = r#"
int values[10];
int *next(void);
int (*fp)(int);
struct S { int a; } inst;
"#;
    let symbols = ctx_symbol::outline(src, c_path()).unwrap();
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    // prototypes are skipped; globals, function pointers, structs, and
    // instantiated instances are extracted (the declaration node precedes
    // its wrapped struct_specifier)
    assert_eq!(names, vec!["values", "fp", "inst", "S", "a"]);
    assert_eq!(symbols[1].kind, ctx_symbol::SymbolKind::Variable); // fp
    assert_eq!(symbols[2].kind, ctx_symbol::SymbolKind::Variable); // inst
    // function pointer slice points at the declaration
    let fp = symbols.iter().find(|s| s.name == "fp").unwrap();
    let slice = &src.as_bytes()[fp.byte_range.clone()];
    assert!(
        std::str::from_utf8(slice)
            .unwrap()
            .contains("int (*fp)(int)")
    );
}

#[test]
fn cpp_special_members_and_operators() {
    let src = r#"
class W {
    ~W() {}
    W& operator=(const W& o) { return *this; }
    W(int x) : x_(x) {}
    int x_;
};
"#;
    let symbols = ctx_symbol::outline(src, cpp_path()).unwrap();
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["W", "~W", "operator=", "W", "x_"]);
    assert_eq!(symbols[1].kind, ctx_symbol::SymbolKind::Function);
    assert_eq!(symbols[2].kind, ctx_symbol::SymbolKind::Function);
    let dtor = symbols.iter().find(|s| s.name == "~W").unwrap();
    let slice = &src.as_bytes()[dtor.byte_range.clone()];
    assert!(std::str::from_utf8(slice).unwrap().contains("~W()"));
}

#[test]
fn c_typedef_compact_keeps_the_close() {
    let src = r#"
typedef struct Point {
    double x;
    double y;
} Point;

typedef struct {
    int a;
} Anon;
"#;
    let parsed = ctx_symbol::parse(c_path(), src).unwrap();
    let symbols = ctx_symbol::extract_symbols(&parsed);
    for s in symbols {
        let compact = ctx_symbol::compact_symbol(&parsed, &s);
        let reparsed = ctx_symbol::parse(c_path(), &compact).unwrap();
        assert!(
            !reparsed.tree.root_node().has_error(),
            "{} compact: {compact}",
            s.name
        );
        let multi_line = s.byte_range.start..s.byte_range.end;
        let _ = multi_line;
        if src.as_bytes()[s.byte_range.clone()]
            .windows(2)
            .any(|w| w == b"\n")
        {
            assert!(compact.contains("... ["), "{} folded: {compact}", s.name);
        }
    }
}

/// Helper: compact every multi-line symbol of `src` and assert the compact
/// re-parses whenever the raw slice does (the corpus regression criterion).
fn assert_compacts_reparse(src: &str, path: &Path) {
    let parsed = ctx_symbol::parse(path, src).unwrap();
    for s in ctx_symbol::extract_symbols(&parsed) {
        let raw = std::str::from_utf8(&src.as_bytes()[s.byte_range.clone()]).unwrap();
        let raw_ok = ctx_symbol::parse(path, raw)
            .map(|r| !r.tree.root_node().has_error())
            .unwrap_or(false);
        if !raw_ok {
            continue;
        }
        let compact = ctx_symbol::compact_symbol(&parsed, &s);
        let re_ok = ctx_symbol::parse(path, &compact)
            .map(|r| !r.tree.root_node().has_error())
            .unwrap_or(false);
        assert!(re_ok, "{} compact: {compact}", s.name);
    }
}

#[test]
fn compact_comment_quotes_do_not_break_folding() {
    // A `"` inside a body comment must not toggle the multi-line-string
    // state; the body still folds and re-parses.
    let src = r#"
export function foo(x) {
  const a = 1;
  // " odd quote in comment
  const b = 2;
  const c = 3;
  return a + b;
}
"#;
    let parsed = ctx_symbol::parse(ts_path(), src).unwrap();
    let foo = ctx_symbol::extract_symbols(&parsed)
        .into_iter()
        .find(|s| s.name == "foo")
        .unwrap();
    let compact = ctx_symbol::compact_symbol(&parsed, &foo);
    assert!(
        compact.contains("... [5 lines omitted]"),
        "compact: {compact}"
    );
    assert!(compact.contains('}'), "closer kept: {compact}");
    let re = ctx_symbol::parse(ts_path(), &compact).unwrap();
    assert!(!re.tree.root_node().has_error(), "reparse: {compact}");
}

#[test]
fn compact_hash_comments_are_not_preprocessor_directives() {
    // Python `#` lines inside a body are comments, not preprocessor
    // directives — folding must proceed (and re-parse).
    let src = r#"
def foo(x):
    a = 1
    # " odd quote in comment
    b = 2
    c = 3
    return a + b
"#;
    let parsed = ctx_symbol::parse(py_path(), src).unwrap();
    let foo = ctx_symbol::extract_symbols(&parsed)
        .into_iter()
        .find(|s| s.name == "foo")
        .unwrap();
    let compact = ctx_symbol::compact_symbol(&parsed, &foo);
    assert!(
        compact.contains("... [5 lines omitted]"),
        "compact: {compact}"
    );
    let re = ctx_symbol::parse(py_path(), &compact).unwrap();
    assert!(!re.tree.root_node().has_error(), "reparse: {compact}");
}

#[test]
fn compact_paren_in_comment_does_not_hold_open_balance() {
    // A `(` inside a comment must not count as a pending opener; otherwise
    // the fold slides to the end of the body.
    let src = r#"
def foo(x):
    a = 1
    # unbalanced ( in comment
    b = 2
    return a + b
"#;
    let parsed = ctx_symbol::parse(py_path(), src).unwrap();
    let foo = ctx_symbol::extract_symbols(&parsed)
        .into_iter()
        .find(|s| s.name == "foo")
        .unwrap();
    let compact = ctx_symbol::compact_symbol(&parsed, &foo);
    assert!(
        compact.contains("... [4 lines omitted]"),
        "compact: {compact}"
    );
    let re = ctx_symbol::parse(py_path(), &compact).unwrap();
    assert!(!re.tree.root_node().has_error(), "reparse: {compact}");
}

#[test]
fn compact_preproc_macro_drops_closer() {
    // A `\`-continued macro: the fold marker splices into the directive, so
    // the `} while (0)` closer must not be kept outside it.
    let src = r#"
#define CLEANUP(x) \
  do { int r = (x); \
       if (r) return r; \
  } while (0)

#define CAT(a, b) a##b
"#;
    let parsed = ctx_symbol::parse(c_path(), src).unwrap();
    let symbols = ctx_symbol::extract_symbols(&parsed);
    assert!(symbols.len() >= 2);
    for s in symbols {
        let compact = ctx_symbol::compact_symbol(&parsed, &s);
        if s.name == "CLEANUP" {
            assert!(compact.trim_end().ends_with(']'), "macro: {compact}");
        }
        let re = ctx_symbol::parse(c_path(), &compact).unwrap();
        assert!(!re.tree.root_node().has_error(), "macro: {compact}");
    }
}

#[test]
fn compact_slides_past_expression_continuations() {
    // go var with `+` continuations: folding mid-expression would orphan
    // the `+`; the compact must pass the whole declaration through.
    let src = r#"
var repeatedSpaces = "" +
	strings.Repeat(" ", 256) +
	strings.Repeat(" ", 256)

var x = f(1, 2)
"#;
    let parsed = ctx_symbol::parse(go_path(), src).unwrap();
    for sym in ctx_symbol::extract_symbols(&parsed) {
        let compact = ctx_symbol::compact_symbol(&parsed, &sym);
        let re = ctx_symbol::parse(go_path(), &compact).unwrap();
        assert!(!re.tree.root_node().has_error(), "{}: {compact}", sym.name);
    }
    let parsed = ctx_symbol::parse(go_path(), src).unwrap();
    let repeated = ctx_symbol::extract_symbols(&parsed)
        .into_iter()
        .find(|s| s.name == "repeatedSpaces")
        .unwrap();
    let compact = ctx_symbol::compact_symbol(&parsed, &repeated);
    assert!(
        compact.contains("strings.Repeat"),
        "continuations must not be folded: {compact}"
    );
}

#[test]
fn compact_ruby_predicate_and_case_fold() {
    let src = r#"
def loopback?
    case @family
    when Socket::AF_INET
      @addr[0] == 127
    else
      false
    end
  end

def simple(a)
  a + 1
end
"#;
    assert_compacts_reparse(src, ruby_path());
    let parsed = ctx_symbol::parse(ruby_path(), src).unwrap();
    let loopback = ctx_symbol::extract_symbols(&parsed)
        .into_iter()
        .find(|s| s.name == "loopback?")
        .unwrap();
    let compact = ctx_symbol::compact_symbol(&parsed, &loopback);
    assert!(compact.starts_with("def loopback?"), "{compact}");
    assert!(compact.contains("# ... ["), "{compact}");
    assert!(compact.trim_end().ends_with("end"), "{compact}");
}

#[test]
fn compact_java_annotation_prefix_folds_class_body() {
    let src = r#"
@ContextConfiguration(
  classes = {
    WireMockConfig.class,
  }
)
class Foo {
  int x;
  int bar() { return x; }
}
"#;
    let parsed = ctx_symbol::parse(java_path(), src).unwrap();
    let foo = ctx_symbol::extract_symbols(&parsed)
        .into_iter()
        .find(|s| s.name == "Foo")
        .unwrap();
    let compact = ctx_symbol::compact_symbol(&parsed, &foo);
    assert!(compact.starts_with("@ContextConfiguration("), "{compact}");
    assert!(compact.contains("class Foo {"), "{compact}");
    let re = ctx_symbol::parse(java_path(), &compact).unwrap();
    assert!(!re.tree.root_node().has_error(), "{compact}");
}

#[test]
fn compact_js_template_literal_passes_through() {
    let src = r#"
const help = () => console.log(
`line one
line two`);
"#;
    let parsed = ctx_symbol::parse(js_path(), src).unwrap();
    let sym = &ctx_symbol::extract_symbols(&parsed)[0];
    let compact = ctx_symbol::compact_symbol(&parsed, sym);
    let re = ctx_symbol::parse(js_path(), &compact).unwrap();
    assert!(!re.tree.root_node().has_error(), "{compact}");
    assert!(
        compact.contains("line two"),
        "template must not fold: {compact}"
    );
}

#[test]
fn compact_passes_single_line_symbols_through() {
    let parsed = ctx_symbol::parse(rust_path(), RUST_SAMPLE).unwrap();
    let symbols = ctx_symbol::extract_symbols(&parsed);
    let max_retries = symbols.iter().find(|s| s.name == "MAX_RETRIES").unwrap();
    let compact = ctx_symbol::compact_symbol(&parsed, max_retries);
    let raw = std::str::from_utf8(&RUST_SAMPLE.as_bytes()[max_retries.byte_range.clone()]).unwrap();
    assert_eq!(compact, raw);
}

#[test]
fn javascript_extracts_classes_functions_and_variables() {
    let symbols = ctx_symbol::outline(JS_SAMPLE, js_path()).unwrap();
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    // `const os = require(...)` is a top-level binding -> also a variable.
    assert_eq!(
        names,
        vec!["os", "User", "greet", "formatName", "MAX_RETRIES", "legacy"]
    );
    assert_eq!(symbols[1].kind, ctx_symbol::SymbolKind::Class);
    assert_eq!(symbols[2].kind, ctx_symbol::SymbolKind::Method);
    assert_eq!(symbols[4].kind, ctx_symbol::SymbolKind::Variable);
    // doc comments via /** */ are attached
    assert_eq!(symbols[1].doc_comment.as_deref(), Some("A user entity."));
    // byte slicing works
    let greet = symbols.iter().find(|s| s.name == "greet").unwrap();
    let slice = &JS_SAMPLE.as_bytes()[greet.byte_range.clone()];
    let text = std::str::from_utf8(slice).unwrap();
    assert!(text.contains("greet()"));
    assert!(!text.contains("class User"));
}

#[test]
fn java_extracts_types_methods_constructors_and_fields() {
    let symbols = ctx_symbol::outline(JAVA_SAMPLE, java_path()).unwrap();
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "Point", "x", "y", "norm", "Repo", "all", "Pair", "Status", "App", "App", "main",
        ]
    );
    assert_eq!(symbols[0].kind, ctx_symbol::SymbolKind::Class);
    assert_eq!(symbols[1].kind, ctx_symbol::SymbolKind::Variable);
    assert_eq!(symbols[4].kind, ctx_symbol::SymbolKind::Interface);
    assert_eq!(symbols[6].kind, ctx_symbol::SymbolKind::Class); // record
    assert_eq!(symbols[7].kind, ctx_symbol::SymbolKind::Enum);
    assert_eq!(symbols[9].kind, ctx_symbol::SymbolKind::Method); // constructor
    assert_eq!(symbols[0].doc_comment.as_deref(), Some("A 2D point."));
    // slicing works
    let norm = symbols.iter().find(|s| s.name == "norm").unwrap();
    let slice = &JAVA_SAMPLE.as_bytes()[norm.byte_range.clone()];
    let text = std::str::from_utf8(slice).unwrap();
    assert!(text.contains("Math.sqrt"));
    assert!(!text.contains("class Point"));
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
fn python_decorated_definitions_include_decorators() {
    let symbols = ctx_symbol::outline(PY_DECORATED_SAMPLE, py_path()).unwrap();
    let decorated = symbols.iter().find(|s| s.name == "decorated").unwrap();
    let slice = &PY_DECORATED_SAMPLE.as_bytes()[decorated.byte_range.clone()];
    let text = std::str::from_utf8(slice).unwrap();
    assert!(
        text.starts_with("@deco"),
        "decorator must be in the slice: {text}"
    );
    assert!(text.contains("def decorated(x)"));
    assert!(!text.contains("def deco(f)"), "slice too wide: {text}");
    // The docstring above a decorated definition is attached.
    let processed = symbols.iter().find(|s| s.name == "processed").unwrap();
    assert_eq!(processed.doc_comment.as_deref(), Some("Processes things."));
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

#[test]
fn ast_anchored_fold_c_functions_and_typedefs() {
    let src = "int add(int a, int b) {\n    int r = a + b;\n    return r;\n}\n\ntypedef struct Point {\n    int x;\n    int y;\n} Point;\n";
    let parsed = ctx_symbol::parse(c_path(), src).unwrap();
    let symbols = ctx_symbol::extract_symbols(&parsed);

    let add = symbols.iter().find(|s| s.name == "add").unwrap();
    let compact = ctx_symbol::compact_symbol(&parsed, add);
    assert!(
        compact.contains("int add(int a, int b) {"),
        "header kept: {compact}"
    );
    assert!(
        compact.contains("... [2 lines omitted]"),
        "body folded: {compact}"
    );
    assert!(compact.contains('}'), "closer kept: {compact}");
    let reparsed = ctx_symbol::parse(c_path(), &compact).unwrap();
    assert!(
        !reparsed.tree.root_node().has_error(),
        "re-parse: {compact}"
    );

    let point = symbols.iter().find(|s| s.name == "Point").unwrap();
    let compact = ctx_symbol::compact_symbol(&parsed, point);
    assert!(
        compact.contains("typedef struct Point {"),
        "typedef header kept: {compact}"
    );
    assert!(
        compact.contains("} Point;"),
        "typedef closer kept: {compact}"
    );
    let reparsed = ctx_symbol::parse(c_path(), &compact).unwrap();
    assert!(
        !reparsed.tree.root_node().has_error(),
        "re-parse: {compact}"
    );
}
