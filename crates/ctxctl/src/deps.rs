//! Dependency resolution for `ctxctl deps`.
//!
//! Classification is a deterministic function of the extracted imports, the
//! `[paths] ignore` globs (cli-contract.md §7), and anchors derived from the
//! analyzed file's own location (its directory and its project root). The
//! process working directory never participates, so the same file in the
//! same tree produces byte-identical output from any cwd.
//! No network, no index — stateless per invocation.

use ctx_symbol::Import;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// How an import target relates to the workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DepKind {
    /// In-crate / relative import, or a bare target that resolves under the
    /// analyzed file's project root.
    Local,
    /// Anything else: crates, stdlib, npm packages, remote modules.
    External,
    /// Target matches a `[paths] ignore` glob.
    Ignored,
    /// Bare target with conflicting local evidence: files matching it sit
    /// beside the analyzed file while nothing resolves at the project root
    /// (e.g. an `os.py` shadowing the stdlib module). Emitted instead of
    /// guessing a side.
    Unresolved,
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
    let file_dir = canonical_dir(file_dir);
    let root = project_root(&file_dir);
    if imp.relative {
        // Rust in-crate paths are always local (no dir-walking involved).
        if language == "rust" {
            return DepKind::Local;
        }
        // Relative imports resolve against the file's directory; python
        // leading-dot modules (`from .x import y`) resolve against the
        // package directory — n leading dots means n-1 levels up. Keep
        // `./`/`../` intact for Path::join.
        //
        // Ignore globs match ONLY the project-relative portion of the
        // resolved path, so ancestor directories named like an ignore glob
        // (`target`, `dist`, ...) above the project cannot taint imports.
        let base = file_dir.strip_prefix(&root).unwrap_or(Path::new(""));
        let joined = base.join(relative_target(imp));
        let probe = normalize(&joined.to_string_lossy());
        return if is_ignored(&probe, ignore) {
            DepKind::Ignored
        } else {
            DepKind::Local
        };
    }

    // Bare targets: ignore globs first, then rust in-crate modules, then
    // anchored existence probes for python/go/java/csharp.
    if is_ignored(&imp.target, ignore) {
        return DepKind::Ignored;
    }
    if language == "rust"
        && let Some(first) = imp.target.split("::").next()
        && local_mods.contains(first)
    {
        return DepKind::Local;
    }
    if matches!(language, "python" | "go" | "java" | "csharp") {
        let candidates = existence_candidates(language, &imp.target);
        // A bare target is local only when it resolves deterministically at
        // the project root (module layouts root there). Files merely sitting
        // next to the analyzed file are ambiguous — they may shadow a
        // well-known module — so they yield Unresolved, never a guess.
        if candidates.iter().any(|c| root.join(c).exists()) {
            return DepKind::Local;
        }
        if candidates.iter().any(|c| file_dir.join(c).exists()) {
            return DepKind::Unresolved;
        }
    }
    DepKind::External
}

/// File/dir candidates for a bare target, relative to an anchor root.
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

/// The import target as a relative path fragment: `./`/`../` kept verbatim,
/// python leading-dot forms expanded to `..` jumps.
fn relative_target(imp: &Import) -> PathBuf {
    if imp.target.starts_with('.')
        && !imp.target.starts_with("./")
        && !imp.target.starts_with("../")
    {
        let dots = imp.target.bytes().take_while(|b| *b == b'.').count();
        let mut up = PathBuf::new();
        for _ in 1..dots {
            up.push("..");
        }
        up.push(&imp.target[dots..]);
        up
    } else {
        PathBuf::from(&imp.target[..])
    }
}

/// Absolute, symlink-resolved directory of the analyzed file. The fallback
/// keeps probes deterministic when canonicalization fails mid-run; by then
/// `read_source` has already read the file successfully, so this is rare.
fn canonical_dir(dir: &Path) -> PathBuf {
    dir.canonicalize()
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default().join(dir))
}

/// Anchoring root for probes and ignore scoping: the nearest ancestor of
/// `start` (inclusive) containing a `.git` entry; `start` itself when not
/// inside a repository. Derived purely from the file location.
fn project_root(start: &Path) -> PathBuf {
    let mut current = start;
    loop {
        if current.join(".git").exists() {
            return current.to_path_buf();
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => return start.to_path_buf(),
        }
    }
}

/// True if the project-relative path contains a segment matching one of the
/// ignore globs, or the whole path matches a slash-containing pattern.
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
/// (single char). Deterministic; no regex engine involved. Memoized on
/// (pattern_len, text_len) so adversarial patterns (`*a*a*…*b`) stay
/// polynomial instead of exponential.
fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let mut memo = vec![vec![None; t.len() + 1]; p.len() + 1];
    fn go(p: &[char], t: &[char], memo: &mut [Vec<Option<bool>>]) -> bool {
        if let Some(v) = memo[p.len()][t.len()] {
            return v;
        }
        let res = match (p.first(), t.first()) {
            (None, None) => true,
            (Some('*'), _) => go(&p[1..], t, memo) || (!t.is_empty() && go(p, &t[1..], memo)),
            (Some('?'), Some(_)) => go(&p[1..], &t[1..], memo),
            (Some(a), Some(b)) if a == b => go(&p[1..], &t[1..], memo),
            _ => false,
        };
        memo[p.len()][t.len()] = Some(res);
        res
    }
    go(&p, &t, &mut memo)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_resolves_dot_segments() {
        assert_eq!(normalize("a/./b"), "a/b");
        assert_eq!(normalize("a/b/../c"), "a/c");
        assert_eq!(normalize("//x//"), "x");
        assert_eq!(normalize("../a"), "a");
    }

    #[test]
    fn is_ignored_matches_segments_and_slash_patterns() {
        let ignore = vec![
            "node_modules".to_string(),
            "src/vendor/*".to_string(),
            "gen?.ts".to_string(),
        ];
        assert!(is_ignored("a/node_modules/b", &ignore));
        assert!(is_ignored("src/vendor/helper", &ignore));
        assert!(is_ignored("pkgs/genX.ts", &ignore));
        assert!(!is_ignored("a/targetless/b", &ignore));
        assert!(!is_ignored("src/vendorous/helper", &ignore));
    }

    #[test]
    fn relative_target_expands_python_dots() {
        let mk = |target: &str| Import {
            target: target.to_string(),
            relative: true,
            line: 1,
            byte_range: 0..1,
        };
        assert_eq!(relative_target(&mk("./x")), PathBuf::from("./x"));
        assert_eq!(relative_target(&mk("../y")), PathBuf::from("../y"));
        assert_eq!(relative_target(&mk(".")), PathBuf::new());
        // Leading dots become `..` jumps; the remainder stays one component
        // (pre-existing join semantics, unchanged here).
        assert_eq!(
            relative_target(&mk("..pkg.mod")),
            PathBuf::from("../pkg.mod")
        );
        assert_eq!(relative_target(&mk("plain")), PathBuf::from("plain"));
    }

    #[test]
    fn existence_candidates_shapes_per_language() {
        assert_eq!(
            existence_candidates("python", "os"),
            vec![
                PathBuf::from("os"),
                PathBuf::from("os.py"),
                PathBuf::from("os/__init__.py"),
                PathBuf::from("os.pyi"),
            ]
        );
        assert_eq!(
            existence_candidates("go", "a/b"),
            vec![PathBuf::from("a/b"), PathBuf::from("a/b.go")]
        );
        assert!(existence_candidates("rust", "x").is_empty());
    }
}
