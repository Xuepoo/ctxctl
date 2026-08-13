//! Config resolution per cli-contract.md §6.
//!
//! Precedence (high -> low): `--config <path>` > project `.ctxctl/config.toml`
//! (discovered by walking up from the cwd) > XDG global
//! (`$XDG_CONFIG_HOME/ctxctl/config.toml`) > built-in defaults.
//!
//! Stateless by design: parsed per command, never cached. Project-level keys
//! override global keys; undeclared keys fall back to global -> default.
//! No array-concatenation semantics.

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Default `[outline]` fold threshold (cli-contract.md §7).
pub const DEFAULT_FOLD_THRESHOLD: usize = 50;

/// Default `[paths]` ignore globs (cli-contract.md §7).
pub const DEFAULT_IGNORE_GLOBS: &[&str] = &["node_modules", "target", "dist", ".git"];

/// Fully resolved configuration, key by key.
#[derive(Debug, Clone)]
pub struct Config {
    pub exec: ExecConfig,
    pub outline: OutlineConfig,
    pub paths: PathsConfig,
    pub general: GeneralConfig,
}

#[derive(Debug, Clone)]
pub struct ExecConfig {
    /// Keep patterns; replaces the built-in defaults when configured.
    pub keep: Vec<String>,
    pub head_lines: usize,
    pub tail_lines: usize,
    pub collapse_threshold: usize,
}

#[derive(Debug, Clone)]
pub struct OutlineConfig {
    /// Fold the symbol list (text mode) when it exceeds this many symbols.
    pub fold_threshold: usize,
    pub show_doc: bool,
}

#[derive(Debug, Clone)]
pub struct PathsConfig {
    /// Default-ignored directories (rg-style glob), for commands that walk
    /// directories.
    pub ignore: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct GeneralConfig {
    pub show_saved: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            exec: ExecConfig {
                keep: ctx_exec::DEFAULT_KEEP_PATTERNS
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
                head_lines: ctx_exec::DEFAULT_HEAD_LINES,
                tail_lines: ctx_exec::DEFAULT_TAIL_LINES,
                collapse_threshold: ctx_exec::DEFAULT_COLLAPSE_THRESHOLD,
            },
            outline: OutlineConfig {
                fold_threshold: DEFAULT_FOLD_THRESHOLD,
                show_doc: true,
            },
            paths: PathsConfig {
                ignore: DEFAULT_IGNORE_GLOBS.iter().map(|s| s.to_string()).collect(),
            },
            general: GeneralConfig { show_saved: true },
        }
    }
}

/// Partial view of a config file: only fields explicitly declared are set.
/// Used to implement key-wise merge semantics. Unknown keys are errors —
/// a typo'd key must not be silently dropped.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Partial {
    exec: PartialExec,
    outline: PartialOutline,
    paths: PartialPaths,
    general: PartialGeneral,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct PartialExec {
    keep: Option<Vec<String>>,
    head_lines: Option<usize>,
    tail_lines: Option<usize>,
    collapse_threshold: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct PartialOutline {
    fold_threshold: Option<usize>,
    show_doc: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct PartialPaths {
    ignore: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct PartialGeneral {
    show_saved: Option<bool>,
}

impl Partial {
    fn merge_into(self, config: &mut Config) {
        if let Some(keep) = self.exec.keep {
            config.exec.keep = keep;
        }
        if let Some(v) = self.exec.head_lines {
            config.exec.head_lines = v;
        }
        if let Some(v) = self.exec.tail_lines {
            config.exec.tail_lines = v;
        }
        if let Some(v) = self.exec.collapse_threshold {
            config.exec.collapse_threshold = v;
        }
        if let Some(v) = self.outline.fold_threshold {
            config.outline.fold_threshold = v;
        }
        if let Some(v) = self.outline.show_doc {
            config.outline.show_doc = v;
        }
        if let Some(ignore) = self.paths.ignore {
            config.paths.ignore = ignore;
        }
        if let Some(v) = self.general.show_saved {
            config.general.show_saved = v;
        }
    }
}

/// Load the resolved configuration. `explicit` is the `--config` path, if any.
pub fn load(explicit: Option<&Path>) -> Result<Config, String> {
    let mut config = Config::default();

    if let Some(global) = xdg_global_path()
        && global.is_file()
    {
        merge_file(&mut config, &global)?;
    }
    if let Some(project) =
        discover_project_config(&std::env::current_dir().map_err(|e| e.to_string())?)
    {
        merge_file(&mut config, &project)?;
    }
    if let Some(explicit) = explicit {
        merge_file(&mut config, explicit)?;
    }

    Ok(config)
}

fn merge_file(config: &mut Config, path: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read config {}: {e}", path.display()))?;
    let partial: Partial =
        toml::from_str(&text).map_err(|e| format!("invalid config {}: {e}", path.display()))?;
    partial.merge_into(config);
    Ok(())
}

fn xdg_global_path() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        // An explicitly set XDG_CONFIG_HOME is authoritative; do not fall
        // back to ~/.config (XDG spec).
        return Some(PathBuf::from(xdg).join("ctxctl/config.toml"));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config/ctxctl/config.toml"))
}

/// Walk up from `start` looking for `.ctxctl/config.toml` (like `.git`
/// discovery); stop at the first hit.
fn discover_project_config(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(current) = dir {
        let candidate = current.join(".ctxctl/config.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        dir = current.parent();
    }
    None
}
