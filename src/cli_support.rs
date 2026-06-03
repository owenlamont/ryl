use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::hash::BuildHasher;
use std::path::{Path, PathBuf};

use crate::config::{ConfigContext, YamlLintConfig, discover_per_file};

/// Replace control characters — which a crafted key, value, anchor name, or
/// filename can carry into a diagnostic, warning, or path — with a visible `\u{..}`
/// escape, so they cannot inject terminal escape sequences or (via a newline) a
/// GitHub Actions workflow command, and cannot split a single-line message.
/// Printable text (including multibyte Unicode) is untouched, and the common
/// control-free case borrows without allocating. Shared by the CLI output layer and
/// the `--fix` symlink warning so every user-controlled string is sanitized the same
/// way.
#[must_use]
pub fn sanitize_control(text: &str) -> Cow<'_, str> {
    if !text.contains(char::is_control) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_control() {
            write!(out, "\\u{{{:x}}}", ch as u32)
                .expect("writing to a String is infallible");
        } else {
            out.push(ch);
        }
    }
    Cow::Owned(out)
}

/// Resolve the configuration context for a given file path, optionally using a cached
/// global configuration.
///
/// This mirrors the logic used by the CLI when filtering candidate files.
///
/// # Errors
/// Returns an error when configuration discovery fails for the provided path.
pub fn resolve_ctx<S: BuildHasher>(
    path: &Path,
    global_cfg: Option<&ConfigContext>,
    markdown: bool,
    cache: &mut HashMap<PathBuf, (PathBuf, YamlLintConfig, bool), S>,
) -> Result<(PathBuf, YamlLintConfig, Vec<String>, bool), String> {
    // The global config is markdown-enabled once by the caller, so per-file clones
    // here inherit the built matcher; only freshly-discovered configs need enabling
    // (done before caching, so the matcher is built once per directory).
    if let Some(gc) = global_cfg {
        return Ok((
            gc.base_dir.clone(),
            gc.config.clone(),
            Vec::new(),
            gc.config_found,
        ));
    }
    let start = path
        .parent()
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    if let Some(entry) = cache.get(&start).cloned() {
        return Ok((entry.0, entry.1, Vec::new(), entry.2));
    }
    let ctx = discover_per_file(path)?;
    let mut cfg = ctx.config;
    if markdown {
        cfg.enable_default_markdown(&ctx.base_dir);
    }
    let entry = (ctx.base_dir.clone(), cfg, ctx.config_found);
    let notices = ctx.notices;
    cache.insert(start, entry.clone());
    Ok((entry.0, entry.1, notices, entry.2))
}
