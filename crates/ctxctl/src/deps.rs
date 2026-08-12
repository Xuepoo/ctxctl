//! Dependency resolution for `ctxctl deps`.
//!
//! Classification is a deterministic function of the extracted imports, the
//! `[paths] ignore` globs (cli-contract.md §7), the file's directory, and the
//! current working directory (existence probes for bare python/go targets).
//! No network, no index — stateless per invocation.

use ctx_symbol::Import;
use std::collections::HashSet;
use std::path::Path;

/// How an import target relates to the workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DepKind {
    /// In-crate / relative import, or a bare target that resolves to a file
    /// or directory under the cwd.
    Local,
    /// Anything else: crates, stdlib, npm packages, remote modules.
    External,
    /// Target matches a `[paths] ignore` glob.
    Ignored,
}

/// An import with its resolved kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedImport {
    pub target: String,
    pub kind: DepKind,
    pub line: usize,
    /// Byte length of the import statement in the original source.
    pub bytes: usize,
}

/// Resolve every import of a file to a kind.
pub fn resolve(
    imports: &[Import],
    language: &str,
    file_dir: &Path,
    ignore: &[String],
) -> Vec<ResolvedImport> {
    // Rust `mod x;` declarations are local modules; a `use x::y` whose first
    // segment matches one of them resolves in-crate.
    let local_mods: HashSet<String> = imports
        .iter()
        .filter(|i| language == "rust" && i.relative && !i.target.contains("::"))
        .map(|i| i.target.clone())
        .collect();
    imports
        .iter()
        .map(|imp| ResolvedImport {
            target: imp.target.clone(),
            kind: classify(imp, language, file_dir, ignore, &local_mods),
            line: imp.line,
            bytes: imp.byte_range.len(),
        })
        .collect()
}

fn classify(
    imp: &Import,
    language: &str,
    file_dir: &Path,
    ignore: &[String],
    local_mods: &HashSet<String>,
) -> DepKind {
    if imp.relative {
        // Rust in-crate paths are always local (no dir-walking involved).
        if language == "rust" {
            return DepKind::Local;
        }
        // Relative imports resolve against the file's directory; python
        // leading-dot modules (`from .x import y`) resolve against the
        // package directory. Keep `./`/`../` intact for Path::join; only
        // python's bare leading dots are stripped.
        let rel = if imp.target.starts_with("./") || imp.target.starts_with("../") {
            &imp.target[..]
        } else {
            imp.target.trim_start_matches('.')
        };
        let joined = file_dir.join(rel);
        // Slash-containing ignore patterns match relative paths, so strip a
        // cwd prefix when present (deterministic per invocation).
        let probe = relative_to_cwd(&joined);
        return if is_ignored(&probe, ignore) {
            DepKind::Ignored
        } else {
            DepKind::Local
        };
    }

    // Bare targets: ignore globs first, then rust in-crate modules, then
    // existence probes for python/go.
    if is_ignored(&imp.target, ignore) {
        return DepKind::Ignored;
    }
    if language == "rust" {
        if let Some(first) = imp.target.split("::").next() {
            if local_mods.contains(first) {
                return DepKind::Local;
            }
        }
    }
    if matches!(language, "python" | "go" | "java" | "csharp") {
        for candidate in existence_candidates(language, &imp.target) {
            if candidate.exists() {
                return DepKind::Local;
            }
        }
    }
    DepKind::External
}

/// File/dir candidates for a bare target, relative to the cwd.
fn existence_candidates(language: &str, target: &str) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    match language {
        "python" => {
            let rel: std::path::PathBuf = target.split('.').collect();
            out.push(rel.clone());
            out.push(rel.with_extension("py"));
            out.push(rel.join("__init__.py"));
            out.push(rel.with_extension("pyi"));
        }
        "go" => {
            let rel: std::path::PathBuf = target.split('/').collect();
            out.push(rel.clone());
            out.push(rel.with_extension("go"));
        }
        "java" => {
            let rel: std::path::PathBuf = target.split('.').collect();
            out.push(rel.clone());
            out.push(rel.with_extension("java"));
        }
        "csharp" => {
            let rel: std::path::PathBuf = target.split('.').collect();
            out.push(rel.clone());
            out.push(rel.with_extension("cs"));
        }
        _ => {}
    }
    out
}

/// True if the path (absolute or relative) contains a segment matching one of
/// the ignore globs, or the whole path matches a slash-containing pattern.
fn is_ignored(path: &str, ignore: &[String]) -> bool {
    let normalized = normalize(path);
    ignore.iter().any(|pattern| {
        if pattern.contains('/') {
            glob_match(pattern, &normalized)
        } else {
            normalized
                .split('/')
                .any(|segment| glob_match(pattern, segment))
        }
    })
}

/// Strip a cwd prefix from an absolute path so slash-containing ignore
/// patterns can match; falls back to the path as-is.
fn relative_to_cwd(path: &Path) -> String {
    let cwd = std::env::current_dir().unwrap_or_default();
    path.strip_prefix(&cwd)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string_lossy().into_owned())
}

/// Resolve `.` / `..` segments and drop empty ones.
fn normalize(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    out.join("/")
}

/// Minimal glob match supporting `*` (any run, crosses segments) and `?`
/// (single char). Deterministic; no regex engine involved.
fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    fn go(p: &[char], t: &[char]) -> bool {
        match (p.first(), t.first()) {
            (None, None) => true,
            (Some('*'), _) => go(&p[1..], t) || (!t.is_empty() && go(p, &t[1..])),
            (Some('?'), Some(_)) => go(&p[1..], &t[1..]),
            (Some(a), Some(b)) if a == b => go(&p[1..], &t[1..]),
            _ => false,
        }
    }
    go(&p, &t)
}
