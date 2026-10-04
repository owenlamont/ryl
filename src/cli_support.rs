use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::{
    ConfigContext, PerFileConfig, SourceKind, SystemEnv, YamlLintConfig,
    load_project_config, locate_per_file,
};

/// Base dir, shared config and whether one was found. `Arc`, not a clone per file: a
/// cloned glob matcher re-allocates its regex cache on first use.
pub type ResolvedConfig = (PathBuf, Arc<YamlLintConfig>, bool);

pub type LintFile = (PathBuf, PathBuf, Arc<YamlLintConfig>, SourceKind);

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
pub fn resolve_ctx(
    path: &Path,
    global_cfg: Option<&ResolvedConfig>,
    flags: &CliConfigFlags,
    cache: &mut ConfigCache,
) -> Result<(PathBuf, Arc<YamlLintConfig>, Vec<String>, bool), String> {
    if let Some((base_dir, cfg, found)) = global_cfg {
        return Ok((base_dir.clone(), Arc::clone(cfg), Vec::new(), *found));
    }
    let start = path
        .parent()
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    if let Some(entry) = cache.by_dir.get(&start).cloned() {
        return Ok((entry.0, entry.1, Vec::new(), entry.2));
    }
    let (entry, notices) = match locate_per_file(path, &SystemEnv)? {
        PerFileConfig::Project { cfg_path, notices } => {
            if let Some(entry) = cache.by_config.get(&cfg_path) {
                (entry.clone(), notices)
            } else {
                let entry = resolved(load_project_config(&cfg_path)?, flags);
                cache.by_config.insert(cfg_path, entry.clone());
                (entry, notices)
            }
        }
        PerFileConfig::Fallback(ctx) => {
            let notices = ctx.notices.clone();
            (resolved(*ctx, flags), notices)
        }
    };
    cache.by_dir.insert(start, entry.clone());
    Ok((entry.0, entry.1, notices, entry.2))
}

/// Resolved configs keyed by input directory, and project configs by their file, so
/// directories sharing one config file share one loaded config.
#[derive(Default)]
pub struct ConfigCache {
    by_dir: HashMap<PathBuf, ResolvedConfig>,
    by_config: HashMap<PathBuf, ResolvedConfig>,
}

/// The `--markdown` and `--enable` overrides, applied to every resolved config.
#[derive(Debug, Default)]
pub struct CliConfigFlags {
    pub markdown: bool,
    pub enable: Option<Vec<&'static str>>,
}

impl CliConfigFlags {
    pub fn apply(&self, cfg: &mut YamlLintConfig, base_dir: &Path) {
        if self.markdown {
            cfg.enable_default_markdown(base_dir);
        }
        if let Some(rules) = &self.enable {
            cfg.restrict_rules(rules);
        }
    }
}

// The global config is adjusted once by the caller; a discovered one here, before
// caching, so its matcher is built once.
fn resolved(ctx: ConfigContext, flags: &CliConfigFlags) -> ResolvedConfig {
    let mut cfg = ctx.config;
    flags.apply(&mut cfg, &ctx.base_dir);
    (ctx.base_dir, Arc::new(cfg), ctx.config_found)
}
