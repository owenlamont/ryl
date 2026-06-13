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

/// Absolute, lexically-normalized form of `path`: `std::path::absolute` makes it
/// absolute and drops `.`, then `..` components are collapsed (`a/../b` -> `b`). Purely
/// lexical — symlinks are **not** resolved, so a symlink stays distinct from its target
/// (matching ruff, and preserving ryl's `--fix`/`--diff` symlink skip). Used both as the
/// input de-dup identity (`gather_lint_files`) and as the basis for a `git apply -p0`-able
/// `--diff` header path.
///
/// # Panics
///
/// Panics if `path` is empty (`std::path::absolute` rejects it). Callers pass
/// source-kind-matched file paths or the stdin label, all non-empty, so this does not
/// arise in practice.
#[must_use]
pub fn lexical_abspath(path: &Path) -> PathBuf {
    let absolute =
        std::path::absolute(path).expect("a non-empty input path is absolutizable");
    let mut out = PathBuf::new();
    for component in absolute.components() {
        if component == std::path::Component::ParentDir {
            // `absolute` rooted the path, so `pop` removes the previous component or is
            // a harmless no-op at the root (`/..` == `/`).
            out.pop();
        } else {
            out.push(component.as_os_str());
        }
    }
    out
}

// User-controlled text (a quoted key, an anchor name, a filename) reaches GitHub
// Actions workflow-command output, where a raw newline would start a new
// `::command::` — a command-injection vector in CI. Encode it the way GitHub's
// `@actions/core` does (data escapes `%`/CR/LF; a `property` such as `file=` also
// escapes `:`/`,`), and additionally render any other control character as a
// literal `\u{..}` — never a `%XX`, which the runner would decode back into the raw
// control char and let it drive ANSI sequences in the log viewer. The result never
// contains a control character, so it cannot inject or split a workflow-command line.
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

/// The display path for a report (`location.path` in GitLab, `name`/`classname` in JUnit):
/// `display` made relative to `project_root` with forward slashes and no `./` prefix, as
/// GitLab requires. The caller relativizes against the project root (`CI_PROJECT_DIR` or
/// the working directory, like ruff), so a `--stdin-filename` or a path under the repo
/// stays relative even when the config lives elsewhere. A path outside the project root
/// is expressed with `..` segments (like ruff's `pathdiff`) rather than kept absolute.
/// Control characters are stripped so a crafted filename cannot inject into the report.
#[must_use]
pub fn report_display_path(display: &Path, project_root: &Path) -> String {
    let absolute = lexical_abspath(display);
    let root = lexical_abspath(project_root);
    let relative = relativize(&absolute, &root);
    let text = relative.to_string_lossy().replace('\\', "/");
    sanitize_control(&text).into_owned()
}

/// `target` expressed relative to `base`, with `..` segments for the part of `base` that
/// `target` does not share. Both are absolute and lexically normalized (no `.`/`..`
/// components), so this is a pure component walk: drop the common prefix, emit one `..`
/// per remaining `base` component, then append the rest of `target`. On a shared root this
/// yields a clean relative path; two different Windows drive prefixes share nothing and
/// fall back to a best-effort `..`-prefixed path (no relative path exists across drives).
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
