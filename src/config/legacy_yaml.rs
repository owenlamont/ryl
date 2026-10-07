//! yamllint-compatible YAML config: parsing, `extends` resolution and the built-in
//! presets. The migrator loads its sources only through [`load`].

use std::path::{Path, PathBuf};

use super::{Env, YamlLintConfig, is_toml_path};
use crate::config_schema::parse_yaml_config;
use crate::yaml_dom::YamlOwned;

/// Bounds `extends` recursion: a cyclic `extends` would otherwise overflow the stack.
const MAX_EXTENDS_DEPTH: usize = 32;

impl YamlLintConfig {
    /// Parse configuration data without filesystem access.
    ///
    /// # Errors
    /// Returns an error when `extends` is used and the config requires filesystem access.
    pub fn from_yaml_str(s: &str) -> Result<Self, String> {
        parse(s, None, None)
    }
}

/// Read and finalize the YAML config at `path`, resolving relative paths against
/// `base_dir` exactly as runtime discovery would.
pub(crate) fn load(
    envx: &dyn Env,
    path: &Path,
    base_dir: &Path,
) -> Result<YamlLintConfig, String> {
    let data = envx.read_to_string(path)?;
    let mut cfg = parse(&data, Some(envx), Some(base_dir))?;
    cfg.finalize(envx, base_dir)?;
    Ok(cfg)
}

pub(super) fn parse(
    s: &str,
    envx: Option<&dyn Env>,
    base_dir: Option<&Path>,
) -> Result<YamlLintConfig, String> {
    parse_at_depth(s, envx, base_dir, 0)
}

fn parse_at_depth(
    s: &str,
    envx: Option<&dyn Env>,
    base_dir: Option<&Path>,
    depth: usize,
) -> Result<YamlLintConfig, String> {
    if depth > MAX_EXTENDS_DEPTH {
        return Err(
            "invalid config: extends nested too deeply (possible cyclic extends)"
                .to_string(),
        );
    }
    let docs = YamlOwned::load_from_str(s)
        .map_err(|e| format!("failed to parse config data: {e}"))?;
    // An empty document stream yields no docs; treat it as a non-mapping so it reports
    // "invalid config: not a mapping" (matching yamllint) instead of panicking on
    // `docs[0]`.
    let parsed = parse_yaml_config(docs.first().unwrap_or(&YamlOwned::BadValue))?;
    let mut cfg = YamlLintConfig::default();
    let base_path = base_dir.unwrap_or_else(|| Path::new(""));
    for entry in &parsed.extends {
        extend_from_entry(&mut cfg, entry, envx, base_path, depth)?;
    }
    cfg.apply_normalized_config(parsed.normalized);
    Ok(cfg)
}

fn extend_from_entry(
    cfg: &mut YamlLintConfig,
    entry: &str,
    envx: Option<&dyn Env>,
    base_dir: &Path,
    depth: usize,
) -> Result<(), String> {
    if let Some(builtin) = builtin(entry) {
        let base = parse(builtin, None, None).expect("builtin preset must parse");
        cfg.merge_from(base);
        return Ok(());
    }

    let Some(envx) = envx else {
        return Err(format!(
            "invalid config: extends '{entry}' requires filesystem access for resolution"
        ));
    };

    let resolved = resolve_extend_path(entry, envx, base_dir);
    if is_toml_path(&resolved) {
        return Err(format!(
            "invalid config: extends cannot reference TOML configuration {}",
            resolved.display()
        ));
    }
    let data = envx.read_to_string(&resolved).map_err(|err| {
        format!(
            "failed to read extended config {}: {err}",
            resolved.display()
        )
    })?;
    let parent_dir = resolved
        .parent()
        .map_or_else(|| base_dir.to_path_buf(), Path::to_path_buf);
    let base = parse_at_depth(&data, Some(envx), Some(&parent_dir), depth + 1)?;
    cfg.merge_from(base);
    Ok(())
}

fn resolve_extend_path(entry: &str, envx: &dyn Env, base_dir: &Path) -> PathBuf {
    let candidate = PathBuf::from(entry);
    if candidate.is_absolute() {
        return candidate;
    }
    let joined = base_dir.join(&candidate);
    if envx.path_exists(&joined) {
        return joined;
    }
    let fallback = envx.current_dir().join(&candidate);
    if envx.path_exists(&fallback) {
        fallback
    } else {
        candidate
    }
}

fn builtin(name: &str) -> Option<&'static str> {
    match name {
        "default" => Some(DEFAULT),
        "relaxed" => Some(RELAXED),
        "empty" => Some(EMPTY),
        _ => None,
    }
}

const DEFAULT: &str = r"---

yaml-files:
  - '*.yaml'
  - '*.yml'
  - '.yamllint'

rules:
  anchors: enable
  braces: enable
  brackets: enable
  colons: enable
  commas: enable
  comments:
    level: warning
  comments-indentation:
    level: warning
  document-end: disable
  document-start:
    level: warning
  empty-lines: enable
  empty-values: disable
  float-values: disable
  hyphens: enable
  indentation: enable
  key-duplicates: enable
  key-ordering: disable
  line-length: enable
  new-line-at-end-of-file: enable
  new-lines: enable
  octal-values: disable
  quoted-strings: disable
  trailing-spaces: enable
  truthy:
    level: warning
";

const RELAXED: &str = r"---

extends: default

rules:
  braces:
    level: warning
    max-spaces-inside: 1
  brackets:
    level: warning
    max-spaces-inside: 1
  colons:
    level: warning
  commas:
    level: warning
  comments: disable
  comments-indentation: disable
  document-start: disable
  empty-lines:
    level: warning
  hyphens:
    level: warning
  indentation:
    level: warning
    indent-sequences: consistent
  line-length:
    level: warning
    allow-non-breakable-inline-mappings: true
  truthy: disable
";

const EMPTY: &str = r"
rules: {}
";
