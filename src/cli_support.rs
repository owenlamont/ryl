use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::hash::BuildHasher;
use std::path::{Path, PathBuf};

use crate::config::{ConfigContext, YamlLintConfig, discover_per_file};

/// Replace control characters with a visible `\u{..}` escape, so a crafted key, anchor,
/// or filename cannot inject terminal escape sequences or, via a newline, a GitHub
/// Actions workflow command, nor split a single-line message. Printable text (incl.
/// multibyte Unicode) is untouched and the control-free case borrows without allocating.
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

/// Absolute, lexically-normalized form of `path` (`a/../b` -> `b`). Purely lexical:
/// symlinks are **not** resolved, so a symlink stays distinct from its target (matching
/// ruff, preserving the `--fix`/`--diff` symlink skip).
///
/// # Panics
///
/// Panics if `path` is empty (`std::path::absolute` rejects it); callers pass non-empty
/// file paths or the stdin label.
#[must_use]
pub fn lexical_abspath(path: &Path) -> PathBuf {
    let absolute =
        std::path::absolute(path).expect("a non-empty input path is absolutizable");
    let mut out = PathBuf::new();
    for component in absolute.components() {
        if component == std::path::Component::ParentDir {
            // `absolute` rooted the path, so `pop` is a no-op at the root (`/..` == `/`).
            out.pop();
        } else {
            out.push(component.as_os_str());
        }
    }
    out
}

// In GitHub Actions workflow-command output a raw newline would start a new
// `::command::`: a CI injection vector. Escape as `@actions/core` does (data escapes
// `%`/CR/LF; a `property` like `file=` also escapes `:`/`,`), but render any other
// control char as a literal `\u{..}`, never a `%XX` the runner would decode back into a
// raw control char that could drive ANSI sequences, so the result holds no control char.
#[must_use]
pub fn github_escape(value: &str, property: bool) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '%' => out.push_str("%25"),
            '\r' => out.push_str("%0D"),
            '\n' => out.push_str("%0A"),
            ':' if property => out.push_str("%3A"),
            ',' if property => out.push_str("%2C"),
            c if c.is_control() => {
                write!(out, "\\u{{{:x}}}", c as u32)
                    .expect("writing to a String is infallible");
            }
            c => out.push(c),
        }
    }
    out
}

/// `display` made relative to `project_root`, forward-slashed, no `./` prefix (GitLab's
/// requirement), `..` segments for a path outside the root. Control chars are stripped
/// so a crafted filename cannot inject.
#[must_use]
pub fn report_display_path(display: &Path, project_root: &Path) -> String {
    let absolute = lexical_abspath(display);
    let root = lexical_abspath(project_root);
    let relative = relativize(&absolute, &root);
    let text = relative.to_string_lossy().replace('\\', "/");
    sanitize_control(&text).into_owned()
}

/// `target` relative to `base` (both absolute, lexically normalized), with `..` for the
/// unshared part of `base`. Two different Windows drive prefixes share nothing and fall
/// back to a `..`-prefixed path, since no cross-drive relative path exists.
fn relativize(target: &Path, base: &Path) -> PathBuf {
    let mut target_parts = target.components().peekable();
    let mut base_parts = base.components().peekable();
    while target_parts.peek().is_some() && target_parts.peek() == base_parts.peek() {
        target_parts.next();
        base_parts.next();
    }
    let mut relative = PathBuf::new();
    for _ in base_parts {
        relative.push("..");
    }
    relative.extend(target_parts);
    relative
}

/// Resolve the configuration context for `path`, reusing `global_cfg` when present.
///
/// # Errors
/// Returns an error when configuration discovery fails for `path`.
pub fn resolve_ctx<S: BuildHasher>(
    path: &Path,
    global_cfg: Option<&ConfigContext>,
    markdown: bool,
    cache: &mut HashMap<PathBuf, (PathBuf, YamlLintConfig, bool), S>,
) -> Result<(PathBuf, YamlLintConfig, Vec<String>, bool), String> {
    // The global config is markdown-enabled once by the caller; only a freshly-discovered
    // config needs enabling, done before caching so the matcher is built once per directory.
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
