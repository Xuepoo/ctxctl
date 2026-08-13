//! Unit tests for import extraction across all language backends.

use std::path::Path;

const RUST_IMPORTS: &str = r#"
use serde::Deserialize;
use crate::lib::helper;
use std::collections::{HashMap, HashSet};
use super::util as u;

mod frontend;
mod inline {
    pub fn x() {}
}

extern crate log;
"#;

const PY_IMPORTS: &str = r#"
import os, sys
import numpy as np
from typing import Optional
from . import vendor_helpers
from .models import User
import myproject.models
"#;

const GO_IMPORTS: &str = r#"
package main

import (
	"fmt"
	_ "embed"
	"github.com/x/y"
	"localpkg/helper"
)

func main() {}
"#;

const TS_IMPORTS: &str = r#"
import express from "express";
import { helper } from "./helpers";
import "../lib/util";
import type { User } from "./types";
import path = require("path");
const os = require("os");
export { helper } from "./helpers2";
export const x = 1;
"#;

const JS_IMPORTS: &str = r#"
import express from "express";
import { helper } from "./helpers";
const os = require("os");
export { helper } from "./helpers2";
export function f() {}
"#;

const JAVA_IMPORTS: &str = r#"
package com.example.app;

import java.util.List;
import static java.lang.Math.PI;
import com.example.util.Helper;
import java.util.*;
"#;

const C_IMPORTS: &str = r#"
#include <stdio.h>
#include <stdlib.h>
#include "util.h"
#include "../common/defs.h"
"#;

const CPP_IMPORTS: &str = r#"
#include <vector>
#include "local.hpp"
using namespace std;
using std::vector;
using Alias = int;
"#;

const CSHARP_IMPORTS: &str = r#"
using System;
using System.Collections.Generic;
using static System.Math;
using MyApp.Utils;
"#;

const RUBY_IMPORTS: &str = r#"
require 'json'
require "sinatra/base"
require_relative 'helpers'
require './local'
require '../shared/mixins'
"#;

const LUA_IMPORTS: &str = r#"
local a = require "json"
local b = require("socket")
local c = require './local'
"#;

fn extract(source: &str, path: &Path) -> Vec<(String, bool, usize)> {
    ctx_symbol::extract_imports(&ctx_symbol::parse(path, source).unwrap())
        .into_iter()
        .map(|i| (i.target, i.relative, i.line))
        .collect()
}

#[test]
fn rust_extracts_use_mod_and_extern_crate() {
    let imports = extract(RUST_IMPORTS, Path::new("sample.rs"));
    assert_eq!(
        imports,
        vec![
            ("serde::Deserialize".to_string(), false, 2),
            ("crate::lib::helper".to_string(), true, 3),
            ("std::collections::HashMap".to_string(), false, 4),
            ("std::collections::HashSet".to_string(), false, 4),
            ("super::util".to_string(), true, 5),
            ("frontend".to_string(), true, 7),
            ("log".to_string(), false, 12),
        ]
    );
}

#[test]
fn python_extracts_imports_and_relative_from() {
    let imports = extract(PY_IMPORTS, Path::new("sample.py"));
    assert_eq!(
        imports,
        vec![
            ("os".to_string(), false, 2),
            ("sys".to_string(), false, 2),
            ("numpy".to_string(), false, 3),
            ("typing".to_string(), false, 4),
            (".".to_string(), true, 5),
            (".models".to_string(), true, 6),
            ("myproject.models".to_string(), false, 7),
        ]
    );
}

#[test]
fn go_extracts_import_specs() {
    let imports = extract(GO_IMPORTS, Path::new("sample.go"));
    assert_eq!(
        imports,
        vec![
            ("fmt".to_string(), false, 5),
            ("embed".to_string(), false, 6),
            ("github.com/x/y".to_string(), false, 7),
            ("localpkg/helper".to_string(), false, 8),
        ]
    );
}

#[test]
fn typescript_extracts_statements_require_and_reexports() {
    let imports = extract(TS_IMPORTS, Path::new("sample.ts"));
    assert_eq!(
        imports,
        vec![
            ("express".to_string(), false, 2),
            ("./helpers".to_string(), true, 3),
            ("../lib/util".to_string(), true, 4),
            ("./types".to_string(), true, 5),
            ("path".to_string(), false, 6),
            ("os".to_string(), false, 7),
            ("./helpers2".to_string(), true, 8),
        ]
    );
}

#[test]
fn javascript_extracts_imports_require_and_reexports() {
    let imports = extract(JS_IMPORTS, Path::new("sample.js"));
    assert_eq!(
        imports,
        vec![
            ("express".to_string(), false, 2),
            ("./helpers".to_string(), true, 3),
            ("os".to_string(), false, 4),
            ("./helpers2".to_string(), true, 5),
        ]
    );
}

#[test]
fn java_extracts_imports_including_static_and_wildcard() {
    let imports = extract(JAVA_IMPORTS, Path::new("Sample.java"));
    assert_eq!(
        imports,
        vec![
            ("java.util.List".to_string(), false, 4),
            ("java.lang.Math".to_string(), false, 5),
            ("com.example.util.Helper".to_string(), false, 6),
            ("java.util".to_string(), false, 7),
        ]
    );
}

#[test]
fn csharp_extracts_using_directives() {
    let imports = extract(CSHARP_IMPORTS, Path::new("Sample.cs"));
    assert_eq!(
        imports,
        vec![
            ("System".to_string(), false, 2),
            ("System.Collections.Generic".to_string(), false, 3),
            ("System.Math".to_string(), false, 4),
            ("MyApp.Utils".to_string(), false, 5),
        ]
    );
}

#[test]
fn ruby_require_relative_is_local_require_is_external() {
    let imports = extract(RUBY_IMPORTS, Path::new("sample.rb"));
    assert_eq!(
        imports,
        vec![
            ("json".to_string(), false, 2),
            ("sinatra/base".to_string(), false, 3),
            ("helpers".to_string(), true, 4),
            ("./local".to_string(), true, 5),
            ("../shared/mixins".to_string(), true, 6),
        ]
    );
}

#[test]
fn lua_require_targets_with_path_prefix_are_relative() {
    let imports = extract(LUA_IMPORTS, Path::new("sample.lua"));
    assert_eq!(
        imports,
        vec![
            ("json".to_string(), false, 2),
            ("socket".to_string(), false, 3),
            ("./local".to_string(), true, 4),
        ]
    );
}

#[test]
fn c_quoted_includes_are_relative_angle_are_external() {
    let imports = extract(C_IMPORTS, Path::new("sample.c"));
    assert_eq!(
        imports,
        vec![
            ("stdio.h".to_string(), false, 2),
            ("stdlib.h".to_string(), false, 3),
            ("util.h".to_string(), true, 4),
            ("../common/defs.h".to_string(), true, 5),
        ]
    );
}

#[test]
fn cpp_extracts_includes_and_using_declarations() {
    let imports = extract(CPP_IMPORTS, Path::new("sample.cpp"));
    assert_eq!(
        imports,
        vec![
            ("vector".to_string(), false, 2),
            ("local.hpp".to_string(), true, 3),
            ("std".to_string(), false, 4),
            ("std::vector".to_string(), false, 5),
        ]
    );
}
