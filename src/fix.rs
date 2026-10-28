use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use ryl_yaml_parser::Event;
#[cfg(not(unix))]
use same_file::Handle;
use similar::TextDiff;

use crate::cli_support::LintFile;
use crate::config::{SourceKind, YamlLintConfig};
use crate::decoder;
use crate::directives::{Directives, PerLineRuleApply};
use crate::format::FormatCache;
use crate::markdown_embed::{
    EmbeddedRegion, MarkdownSources, RegionKind, extract_regions,
    markdown_region_problems,
};
use crate::rules::support::collection_style;
use crate::rules::support::line_syntax::{buffer_newline, first_line_break};
use crate::rules::{
    braces, brackets, colons, commas, comments, comments_indentation, document_end,
    document_start, empty_lines, hyphens, indentation, key_ordering, line_length,
    new_line_at_end_of_file, new_lines, quoted_strings, trailing_spaces, truthy,
};
use crate::text_version::{Analysis, ParsedFacts, TextVersion};

pub const RULE_FIX_MAX_ITERATIONS: usize = 8;
/// Bound on whole-pipeline passes: a later fixer can expose a diagnostic an earlier one
/// fixes (quoted-strings joining a plain scalar's continuation line strands a comment
/// indented to it). Matches ruff's `MAX_ITERATIONS`.
pub const FIX_PIPELINE_MAX_PASSES: usize = 100;

/// File-shape rules suppressed in embedded markdown regions (not standalone files, so `--fix`
/// never injects `---`/`...` or a trailing newline); shared by the check and fix paths.
const SUPPRESSED: [&str; 4] = [
    document_start::ID,
    document_end::ID,
    new_line_at_end_of_file::ID,
    new_lines::ID,
];

#[must_use]
pub fn suppressed_rules() -> &'static [&'static str] {
    &SUPPRESSED
}

/// Every rule with a safe `--fix`, in application order; extend together with `Passes::lint`
/// and the layout planner when adding a safe fixer. The LSP drives per-rule
/// "Fix all `<rule>`" actions off this list.
pub const SAFE_FIX_RULE_IDS: [&str; 16] = [
    new_lines::ID,
    comments::ID,
    comments_indentation::ID,
    commas::ID,
    braces::ID,
    brackets::ID,
    colons::ID,
    hyphens::ID,
    new_line_at_end_of_file::ID,
    quoted_strings::ID,
    trailing_spaces::ID,
    document_start::ID,
    document_end::ID,
    empty_lines::ID,
    truthy::ID,
    key_ordering::ID,
];

#[derive(Debug, Clone, Default)]
pub struct FixStats {
    pub changed_files: usize,
    /// Per-file skip notices and format errors, in input order.
    pub notices: Vec<u8>,
    pub file_notices: Vec<Vec<u8>>,
    pub skipped: Vec<usize>,
    /// Input index and error for files that could not be read or written.
    pub errors: Vec<(usize, String)>,
}

/// One file's in-place fix result. Markdown can fix some regions while skipping others;
/// YAML can apply key ordering while refusing unsafe block scalar edits.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FixOutcome {
    pub changed: bool,
    pub skipped: Vec<crate::lint::LintProblem>,
}

/// The text transform a write-back or preview applies: the safe lint fixes or the formatter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rewrite {
    Fix,
    Format,
}

impl Rewrite {
    /// How the CLI names this rewrite's in-place mode in notices.
    #[must_use]
    pub fn flag(self) -> &'static str {
        match self {
            Self::Fix => "--fix",
            Self::Format => "ryl format",
        }
    }

    fn apply(
        self,
        input: &str,
        cfg: &YamlLintConfig,
        (path, base_dir): (&Path, &Path),
        skip: &[&str],
        cache: &mut FormatCache,
        parsed: Option<&mut ParsedInput>,
    ) -> String {
        match self {
            Self::Fix => apply_safe_fixes_filtered(input, cfg, path, base_dir, skip),
            Self::Format => cache
                .format_with_facts(input, cfg, path, skip, parsed)
                .0
                .clone(),
        }
    }
}

pub(crate) struct ParsedInput {
    pub(crate) facts: Option<ParsedFacts>,
    pub(crate) tokens: Option<crate::text_version::lexical::Tokens>,
    pub(crate) prefetched: bool,
    pub(crate) strict_error: Option<crate::lint::LintProblem>,
}

pub(crate) fn rewrite_parse_gate(
    content: &str,
    rewrite: Rewrite,
) -> Result<ParsedInput, crate::lint::LintProblem> {
    if rewrite == Rewrite::Fix {
        let (relaxed_error, strict_error) = crate::lint::parse_errors(content);
        if let Some(problem) = relaxed_error {
            return Err(problem);
        }
        return Ok(ParsedInput {
            facts: None,
            tokens: None,
            prefetched: false,
            strict_error,
        });
    }
    let (facts, tokens, prefetched) = ParsedFacts::parse_for_format(content);
    if let Some(problem) =
        crate::lint::parse_error_from_result(content, facts.error.as_ref())
    {
        return Err(problem);
    }
    let strict_error =
        crate::lint::parse_error_from_result(content, facts.strict_error());
    Ok(ParsedInput {
        facts: Some(facts),
        tokens,
        prefetched,
        strict_error,
    })
}

/// `std::fs::write` follows symlinks, so `--fix` (and `--diff`, its preview) skips a
/// symlinked input with a warning rather than let an untrusted tree redirect the write
/// (`innocent.yaml -> ~/.bashrc`); read-only linting is unaffected. Best-effort: only the
/// final component is checked, not atomically (a full defense needs `O_NOFOLLOW`).
fn symlink_notice(path: &Path, flag: &str) -> Option<String> {
    is_symlink(path).then(|| {
        format!(
            "skipping {}: refusing to follow a symlink for {flag}\n",
            crate::cli_support::sanitize_control(&path.display().to_string())
        )
    })
}

/// Render the stderr notice for a skipped input or unfixed rule.
#[must_use]
pub fn skip_notice(
    path: &Path,
    problem: &crate::lint::LintProblem,
    flag: &str,
) -> String {
    if problem.level == crate::lint::Severity::Warning && problem.rule.is_none() {
        return format!(
            "{}:{}:{} {}\n",
            crate::cli_support::sanitize_control(&path.display().to_string()),
            problem.line,
            problem.column,
            crate::cli_support::sanitize_control(&problem.message),
        );
    }
    let what = problem.rule.map_or_else(
        || format!("skipped by {flag}"),
        |rule| format!("{rule} not fixed"),
    );
    format!(
        "{}:{}:{} {what}: {}\n",
        crate::cli_support::sanitize_control(&path.display().to_string()),
        problem.line,
        problem.column,
        crate::cli_support::sanitize_control(&problem.message),
    )
}

fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

#[cfg(unix)]
fn file_identity(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::symlink_metadata(path).ok()?;
    Some((metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
fn file_identity(path: &Path) -> Option<Handle> {
    Handle::from_path(path).ok()
}

/// `content` after `rewrite` (`None` when unchanged), plus the skips to report against the
/// rewritten text: unparsable YAML is left untouched with one skip; for Markdown, each
/// region [`fix_markdown_str`] left alone.
#[must_use]
pub fn rewrite_str(
    content: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
    kind: SourceKind,
    rewrite: Rewrite,
) -> (Option<String>, Vec<crate::lint::LintProblem>) {
    match kind {
        SourceKind::Yaml => {
            let mut events = match rewrite_parse_gate(content, rewrite) {
                Ok(events) => events,
                Err(problem) => return (None, vec![problem]),
            };
            let mut cache = FormatCache::default();
            let fixed = rewrite.apply(
                content,
                cfg,
                (path, base_dir),
                &[],
                &mut cache,
                Some(&mut events),
            );
            let mut skipped =
                unfixed_notices(&fixed, cfg, path, base_dir, rewrite, &mut cache);
            if let Some(problem) = rewrite_parse_notice(content, &fixed, &events) {
                if fixed == content {
                    skipped.clear();
                }
                skipped.push(problem);
            }
            ((fixed != content).then_some(fixed), skipped)
        }
        SourceKind::Markdown => {
            let mut cache = FormatCache::default();
            let (fixed, refusals) = fix_markdown_cached(
                content, path, cfg, base_dir, rewrite, true, &mut cache,
            );
            (
                fixed,
                refusals
                    .into_iter()
                    .map(|refusal| refusal.problem)
                    .collect(),
            )
        }
    }
}

/// Apply `rewrite` to each file in place, writing each file as soon as it is rewritten.
#[must_use]
pub fn rewrite_files(files: &[LintFile], rewrite: Rewrite) -> FixStats {
    let mut stats = FixStats::default();
    for batch in hard_link_batches(files) {
        let results = read_in_parallel(
            &files[batch.clone()],
            |decoded, (path, base_dir, cfg, kind)| {
                let (rewritten, skipped) =
                    rewrite_str(decoded.content(), cfg, path, base_dir, *kind, rewrite);
                if let Some(rewritten) = &rewritten {
                    decoded.write(path, rewritten)?;
                }
                Ok((rewritten.is_some(), skipped))
            },
        );
        for (index, (result, stderr)) in batch.zip(results) {
            let notice_start = stats.notices.len();
            let path = &files[index].0;
            if let Some(notice) = symlink_notice(path, rewrite.flag()) {
                stats.notices.extend_from_slice(notice.as_bytes());
                stats
                    .file_notices
                    .push(stats.notices[notice_start..].to_vec());
                stats.skipped.push(index);
                continue;
            }
            stats.notices.extend(stderr);
            match result {
                Ok((changed, skipped)) => {
                    stats.changed_files += usize::from(changed);
                    if !skipped.is_empty() {
                        stats.skipped.push(index);
                    }
                    for problem in &skipped {
                        stats.notices.extend_from_slice(
                            skip_notice(path, problem, rewrite.flag()).as_bytes(),
                        );
                    }
                }
                Err(error) => {
                    if rewrite == Rewrite::Format {
                        let notice = crate::cli_support::sanitize_control(&error);
                        stats
                            .notices
                            .extend_from_slice(format!("error: {notice}\n").as_bytes());
                    }
                    stats.errors.push((index, error));
                }
            }
            stats
                .file_notices
                .push(stats.notices[notice_start..].to_vec());
        }
    }
    stats
}

/// Input ranges in which no two paths name the same file, so the later path of a hard link
/// is read only after the earlier one is written.
fn hard_link_batches(files: &[LintFile]) -> Vec<std::ops::Range<usize>> {
    let mut batches = Vec::new();
    let mut start = 0;
    let mut seen = HashSet::new();
    for (index, (path, ..)) in files.iter().enumerate() {
        let Some(identity) = file_identity(path) else {
            continue;
        };
        if seen.contains(&identity) {
            batches.push(start..index);
            seen.clear();
            start = index;
        }
        seen.insert(identity);
    }
    batches.push(start..files.len());
    batches
}

/// Never reads a symlink; pairs each result with stderr for ordered replay.
fn read_in_parallel<T: Send>(
    files: &[LintFile],
    compute: impl Fn(decoder::DecodedFile, &LintFile) -> Result<T, String> + Sync,
) -> Vec<(Result<T, String>, Vec<u8>)> {
    let compute_file = |file: &LintFile| {
        crate::cli_support::capture_file_stderr(|| {
            if is_symlink(&file.0) {
                Err(format!("skipped symlink {}", file.0.display()))
            } else {
                decoder::read_file_lossless(&file.0)
                    .and_then(|decoded| compute(decoded, file))
            }
        })
    };
    if crate::cli_support::small_file_batch(files) {
        files.iter().map(compute_file).collect()
    } else {
        files.par_iter().map(compute_file).collect()
    }
}

/// A rewrite preview; formatting changes can have no applicable text diff.
#[derive(Debug, Default)]
pub struct DiffOutcome {
    pub changed: bool,
    pub diff: Option<String>,
    pub skipped: Vec<crate::lint::LintProblem>,
    pub problems: Vec<crate::lint::LintProblem>,
}

/// Aggregated `--diff` results across all linted files.
#[derive(Debug, Default)]
pub struct DiffStats {
    pub changed_files: usize,
    /// Per-file stderr, including skip notices, in input order.
    pub notices: Vec<u8>,
    pub file_notices: Vec<Vec<u8>>,
    pub skipped: Vec<PathBuf>,
    /// Unified diffs for files that would change, in input order.
    pub diffs: Vec<String>,
    /// Each changed file's findings and diff.
    pub problems: Vec<(PathBuf, Vec<crate::lint::LintProblem>, Option<String>)>,
    /// Files read that would not change and drew no skip.
    pub unchanged: usize,
}

impl DiffStats {
    pub fn record(&mut self, path: &Path, outcome: DiffOutcome, flag: &str) {
        self.changed_files += usize::from(outcome.changed);
        self.unchanged += usize::from(!outcome.changed && outcome.skipped.is_empty());
        if !outcome.skipped.is_empty() {
            self.skipped.push(path.to_path_buf());
        }
        if let Some(diff) = &outcome.diff {
            self.diffs.push(diff.clone());
        }
        for problem in &outcome.skipped {
            self.notices
                .extend_from_slice(skip_notice(path, problem, flag).as_bytes());
        }
        if !outcome.problems.is_empty() {
            self.problems
                .push((path.to_path_buf(), outcome.problems, outcome.diff));
        }
    }
}

fn cwd_relative_label(path: &Path) -> String {
    let cwd = std::env::current_dir().unwrap_or_default();
    #[cfg(windows)]
    let label = {
        let rooted = match path.components().next() {
            Some(std::path::Component::Prefix(prefix))
                if matches!(prefix.kind(), std::path::Prefix::Disk(_))
                    && !path.has_root() =>
            {
                let std::path::Prefix::Disk(drive) = prefix.kind() else {
                    unreachable!()
                };
                std::path::absolute(format!("{}:", char::from(drive)))
                    .expect("an input drive is absolutizable")
                    .join(
                        path.strip_prefix(prefix.as_os_str())
                            .expect("the prefix came from this path"),
                    )
            }
            _ => cwd.join(path),
        };
        let mut absolute = PathBuf::new();
        for component in rooted.components() {
            if component == std::path::Component::ParentDir {
                absolute.pop();
            } else {
                absolute.push(component.as_os_str());
            }
        }
        windows_diff_label(&absolute.to_string_lossy(), &cwd.to_string_lossy())
    };
    #[cfg(not(windows))]
    let label = {
        let abspath = crate::cli_support::lexical_abspath(path);
        abspath
            .strip_prefix(&cwd)
            .unwrap_or(&abspath)
            .display()
            .to_string()
    };
    crate::cli_support::sanitize_control(&label).into_owned()
}

#[must_use]
pub fn windows_diff_label(path: &str, cwd: &str) -> String {
    let path = diff_display_path(path);
    let cwd = diff_display_path(cwd);
    if path == cwd {
        String::new()
    } else {
        path.strip_prefix(&format!("{}/", cwd.trim_end_matches('/')))
            .unwrap_or(&path)
            .to_owned()
    }
}

fn diff_display_path(path: &str) -> String {
    let label = path.replace('\\', "/");
    if let Some(unc) = label.strip_prefix("//?/UNC/") {
        format!("//{unc}")
    } else {
        label.strip_prefix("//?/").unwrap_or(&label).to_owned()
    }
}

/// A unified diff, or `None` when identical, like `ruff check --diff`: 3 context lines
/// (pinned against a `similar` default change) and a plain `--- path`/`+++ path` header,
/// CWD-relative (so `git apply -p0` works) and sanitized
/// against injected escapes or forged hunks. The body is verbatim.
fn render_unified_diff(original: &str, fixed: &str, path: &Path) -> Option<String> {
    if original == fixed {
        return None;
    }
    let label = cwd_relative_label(path);
    // git/patch headers use forward slashes; normalize the Windows `\` (Unix leaves `\`
    // alone, where it is a filename character, not a separator).
    #[cfg(windows)]
    let label = label.replace('\\', "/");
    // Split on `\n` only (not `similar`'s CR-aware `from_lines`) so a bare `\r` is diff
    // *content* `git apply` matches byte-for-byte. Identical to `from_lines` on LF/CRLF; a
    // side ending in a bare `\r` is unrenderable and `diff_outcome` skips it before here.
    let original_lines: Vec<&str> = original.split_inclusive('\n').collect();
    let fixed_lines: Vec<&str> = fixed.split_inclusive('\n').collect();
    Some(
        TextDiff::configure()
            .newline_terminated(true)
            .diff_slices(&original_lines, &fixed_lines)
            .unified_diff()
            .context_radius(3)
            .header(&label, &label)
            .to_string(),
    )
}

/// The `--diff` outcome for `content` (file and stdin paths), gated like `--fix`: unparsable
/// YAML yields one skip; Markdown diffs at host level via [`fix_markdown_str`], skipping each
/// unparsable region. A path unrepresentable in a diff header is skipped here.
#[must_use]
pub fn diff_outcome(
    content: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
    kind: SourceKind,
    rewrite: Rewrite,
) -> DiffOutcome {
    let mut cache = FormatCache::default();
    let mut outcome =
        diff_and_skips(content, cfg, path, base_dir, kind, rewrite, &mut cache);
    if rewrite == Rewrite::Format
        && (outcome.diff.is_some() || outcome.changed && outcome.problems.is_empty())
    {
        outcome
            .problems
            .push(would_reformat(outcome.diff.as_deref()));
    }
    if path_unrepresentable_in_diff(path) {
        outcome.diff = None;
        outcome.changed &= rewrite == Rewrite::Format;
        outcome.skipped.push(diff_skip(
            "filename has non-UTF-8 bytes or control characters; no applicable diff path",
        ));
    }
    outcome
}

/// The one finding a file `ryl format` would change gets, at the first change in `diff`
/// (the start, without one), as ruff reports an unformatted file.
fn would_reformat(diff: Option<&str>) -> crate::lint::LintProblem {
    let (line, column) = diff.and_then(first_change).unwrap_or((1, 1));
    crate::lint::LintProblem {
        line,
        column,
        level: crate::lint::Severity::Error,
        message: "File would be reformatted".to_string(),
        rule: Some("unformatted"),
    }
}

/// The 1-based line and character column of the first change a unified `diff` makes to its
/// old text.
fn first_change(diff: &str) -> Option<(usize, usize)> {
    let mut lines = diff.lines().skip_while(|line| !line.starts_with("@@"));
    let header = lines.next()?.strip_prefix("@@ -")?;
    let mut line = header
        .split([',', ' '])
        .next()?
        .parse::<usize>()
        .ok()?
        .max(1);
    let mut removed = None;
    for text in lines {
        match text.split_at_checked(1) {
            Some((" ", _)) if removed.is_none() => line += 1,
            Some(("-", old)) => {
                removed.get_or_insert(old);
            }
            Some(("+", new)) => {
                let same = removed.map_or(0, |old: &str| {
                    old.chars()
                        .zip(new.chars())
                        .take_while(|(a, b)| a == b)
                        .count()
                });
                return Some((line, same + 1));
            }
            Some(("\\", _)) => {}
            _ => break,
        }
    }
    Some((line, 1))
}

fn diff_and_skips(
    content: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
    kind: SourceKind,
    rewrite: Rewrite,
    cache: &mut FormatCache,
) -> DiffOutcome {
    match kind {
        SourceKind::Yaml => {
            let mut events = match rewrite_parse_gate(content, rewrite) {
                Ok(events) => events,
                Err(problem) => {
                    return DiffOutcome {
                        diff: None,
                        skipped: vec![problem],
                        ..DiffOutcome::default()
                    };
                }
            };
            let fixed = rewrite.apply(
                content,
                cfg,
                (path, base_dir),
                &[],
                cache,
                Some(&mut events),
            );
            if content != fixed && ends_in_bare_cr(content, &fixed) {
                return DiffOutcome {
                    changed: rewrite == Rewrite::Format,
                    diff: None,
                    skipped: vec![bare_cr_diff_skip(rewrite)],
                    ..DiffOutcome::default()
                };
            }
            let mut skipped = unfixed_notices(
                if rewrite == Rewrite::Format {
                    content
                } else {
                    &fixed
                },
                cfg,
                path,
                base_dir,
                rewrite,
                cache,
            );
            if let Some(problem) = rewrite_parse_notice(content, &fixed, &events) {
                if fixed == content {
                    skipped.clear();
                }
                skipped.push(problem);
            }
            DiffOutcome {
                changed: content != fixed,
                diff: render_unified_diff(content, &fixed, path),
                skipped,
                ..DiffOutcome::default()
            }
        }
        SourceKind::Markdown => {
            markdown_diff(content, cfg, (path, base_dir), rewrite, cache)
        }
    }
}

/// [`diff_and_skips`] for a Markdown host.
fn markdown_diff(
    content: &str,
    cfg: &YamlLintConfig,
    (path, base_dir): (&Path, &Path),
    rewrite: Rewrite,
    cache: &mut FormatCache,
) -> DiffOutcome {
    // A bare-`\r` markdown host is skipped upstream (`fix_markdown_str` returns
    // `None`), so content reaching `render_unified_diff` never carries a bare `\r`.
    let (fixed, refusals) =
        fix_markdown_cached(content, path, cfg, base_dir, rewrite, false, cache);
    // `--diff` never writes, so skips point at the original content's lines.
    let mut skips = |markdown| {
        markdown_region_problems(markdown, cfg, |region| {
            let strict = crate::lint::strict_parse_error(&region.content);
            region_skips(
                &region.content,
                strict,
                (cfg, path, base_dir),
                rewrite,
                cache,
            )
        })
    };
    // A format preview gets its region notices from `fix_markdown_cached`.
    let mut skipped = if rewrite == Rewrite::Fix {
        skips(content)
    } else {
        Vec::new()
    };
    skipped.retain(|problem| {
        !refusals.iter().any(|refusal| {
            refusal.problem.level == crate::lint::Severity::Warning
                && refusal.problem.line == problem.line
                && refusal.problem.column == problem.column
                && problem.rule.is_none()
        })
    });
    skipped.extend(refusals.iter().map(|refusal| refusal.problem.clone()));
    // `key-ordering` notices describe the fixed text, so they come from it.
    if let Some(fixed) = fixed.as_ref().filter(|_| rewrite == Rewrite::Fix) {
        skipped.retain(|problem| problem.rule.is_none());
        skipped.extend(skips(fixed).into_iter().filter(|p| p.rule.is_some()));
    }
    let changed =
        fixed.is_some() || refusals.iter().any(|refusal| refusal.blocks_preview);
    let diff = fixed.and_then(|fixed| render_unified_diff(content, &fixed, path));
    DiffOutcome {
        changed,
        diff,
        skipped,
        problems: refusals
            .into_iter()
            .filter(|refusal| refusal.blocks_preview)
            .map(|refusal| refusal.problem)
            .collect(),
    }
}

/// Each refusal or unfixed key-ordering finding left in fixed `content`.
fn unfixed_notices(
    content: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
    rewrite: Rewrite,
    cache: &mut FormatCache,
) -> Vec<crate::lint::LintProblem> {
    if crate::directives::disables_file(content) {
        return Vec::new();
    }
    if rewrite == Rewrite::Format {
        return crate::format::unfixed(content, cfg, cache);
    }
    let mut notices = Vec::new();
    if let Some(line) =
        crate::rules::block_scalar_chomping::ambiguous_blank_body_line(content)
    {
        notices.push(crate::lint::LintProblem {
            line,
            column: 1,
            level: crate::lint::Severity::Error,
            message: "cannot fix this file safely: block scalar value may change"
                .to_string(),
            rule: None,
        });
    }
    if rule_enabled(key_ordering::ID, cfg, path, base_dir) {
        let rule = key_ordering::Config::resolve(cfg, path);
        notices.extend(
            key_ordering::unfixed(content, &rule, &cfg.per_line_applies(path))
                .into_iter()
                .map(|violation| crate::lint::LintProblem {
                    line: violation.line,
                    column: violation.column,
                    level: crate::lint::Severity::Error,
                    message: violation.message,
                    rule: Some(key_ordering::ID),
                }),
        );
    }
    notices
}

/// A Markdown region's skips: its `strict` parse error, else unsafe scalar or unsorted
/// mappings.
fn region_skips(
    region: &str,
    strict: Option<crate::lint::LintProblem>,
    (cfg, path, base_dir): (&YamlLintConfig, &Path, &Path),
    rewrite: Rewrite,
    cache: &mut FormatCache,
) -> Vec<crate::lint::LintProblem> {
    strict.map_or_else(
        || unfixed_notices(region, cfg, path, base_dir, rewrite, cache),
        |problem| vec![problem],
    )
}

fn rewrite_parse_notice(
    original: &str,
    fixed: &str,
    parsed: &ParsedInput,
) -> Option<crate::lint::LintProblem> {
    if crate::directives::disables_file(original) {
        return None;
    }
    parsed.strict_error.clone().map(|mut problem| {
        if original != fixed {
            problem.level = crate::lint::Severity::Warning;
            problem.message =
                "input was YAML the spec rejects; repaired it".to_string();
        }
        problem
    })
}

pub(crate) fn repair_events_match(original: &str, repaired: &str) -> bool {
    let before = TextVersion::new(original);
    let after = TextVersion::new(repaired);
    representation_matches(before.analysis(), after.analysis())
}

#[doc(hidden)]
#[must_use]
/// # Panics
/// Panics if either input lacks a complete parse.
pub fn format_events_match(original: &str, formatted: &str) -> bool {
    let before = TextVersion::new(original);
    let after = TextVersion::new(formatted);
    if format_events_match_with_analysis(before.analysis(), after.analysis()) {
        return true;
    }
    let normalize = |text: &str| {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        trailing_spaces::fix(&text).unwrap_or(text)
    };
    !non_ascii_marker(formatted)
        && repair_events_match(&normalize(original), &normalize(formatted))
}

pub(crate) fn format_events_match_with_analysis(
    original: Analysis<'_>,
    formatted: Analysis<'_>,
) -> bool {
    !non_ascii_marker(formatted.text()) && representation_matches(original, formatted)
}

pub(crate) fn non_ascii_marker(formatted: &str) -> bool {
    // The reference parser's marker lookahead includes non-ASCII whitespace.
    crate::rules::support::line_syntax::split_lines_preserve_endings(formatted).any(
        |(_, line, _)| {
            let line = line.strip_prefix('\u{feff}').unwrap_or(line);
            crate::rules::support::line_syntax::starts_with_document_marker(line)
                && line.as_bytes().get(3).is_some_and(|byte| !byte.is_ascii())
        },
    )
}

fn representation_matches(before: Analysis<'_>, after: Analysis<'_>) -> bool {
    assert!(
        before.parsed().valid && after.parsed().valid,
        "repair operands have complete parses"
    );
    let significant = |event: &Event<'_>| {
        !matches!(
            event,
            Event::Comment(..) | Event::StreamStart | Event::StreamEnd
        )
    };
    let mut left = before
        .event_iter()
        .map(|(event, _)| event)
        .filter(significant);
    let mut right = after
        .event_iter()
        .map(|(event, _)| event)
        .filter(significant);
    let mut comparison = EventComparison::default();
    loop {
        match (left.next(), right.next()) {
            (None, None) => return true,
            (Some(a), Some(b)) => {
                if !comparison.matches(&a, &b) {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

#[derive(Default)]
pub(crate) struct EventComparison {
    yaml11: bool,
}

impl EventComparison {
    pub(crate) fn matches(&mut self, a: &Event<'_>, b: &Event<'_>) -> bool {
        if let Event::DocumentStart(_, version) = a {
            self.yaml11 = crate::rules::support::yaml_version::resolves_as_yaml_1_1(
                version.map(crate::rules::support::yaml_version::event_version),
            );
        }
        a == b
            || normalized_event(a.clone(), self.yaml11)
                == normalized_event(b.clone(), self.yaml11)
    }
}

fn normalized_event(mut event: Event<'_>, yaml11: bool) -> Event<'_> {
    use crate::rules::support::yaml_version;
    use ryl_yaml_parser::{ScalarStyle, StructureStyle};
    match &mut event {
        Event::DocumentStart(explicit, _) => *explicit = false,
        Event::SequenceStart(style, ..) | Event::MappingStart(style, ..) => {
            *style = StructureStyle::Block;
        }
        Event::Scalar(value, style, _, tag)
            if (*style != ScalarStyle::Plain
                || tag.is_some()
                || (if yaml11 {
                    !yaml_version::resolves_to_nonstring_in_yaml_1_1(value)
                } else {
                    matches!(
                        crate::yaml_dom::Scalar::resolve_plain_scalar(value.clone()),
                        crate::yaml_dom::Scalar::String(_)
                    ) && !crate::yaml_dom::is_core_schema_int_spelling(value)
                }))
                && (!value.contains('\u{feff}')
                    || matches!(
                        *style,
                        ScalarStyle::SingleQuoted | ScalarStyle::DoubleQuoted
                    )) =>
        {
            *style = ScalarStyle::DoubleQuoted;
        }
        _ => {}
    }
    event
}

/// Whether either side ends in a bare `\r`, which `similar` renders as a hunk line no patch
/// tool accepts (a mid-line `\r` is fine), so `--diff` skips it (use `--fix`).
fn ends_in_bare_cr(original: &str, fixed: &str) -> bool {
    original.ends_with('\r') || fixed.ends_with('\r')
}

#[must_use]
fn bare_cr_diff_skip(rewrite: Rewrite) -> crate::lint::LintProblem {
    diff_skip(&format!(
        "content ends in a bare carriage return, which has no applicable text diff; use {}",
        rewrite.flag(),
    ))
}

/// A 1:1 skip problem for an input that cannot produce an applicable `--diff`.
#[must_use]
fn diff_skip(message: &str) -> crate::lint::LintProblem {
    crate::lint::LintProblem {
        line: 1,
        column: 1,
        level: crate::lint::Severity::Error,
        message: message.to_string(),
        rule: None,
    }
}

/// The `--diff` skip for a non-UTF-8 (or BOM) input, whose decoded-text diff cannot apply
/// back to the original bytes as `--fix`'s re-encode does. Shared by file and stdin paths.
#[must_use]
pub fn non_utf8_diff_skip() -> crate::lint::LintProblem {
    diff_skip("non-UTF-8 or BOM content has no applicable text diff; use --fix")
}

#[must_use]
pub fn decoded_diff_outcome(
    decoded: &decoder::DecodedFile,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
    kind: SourceKind,
    rewrite: Rewrite,
) -> DiffOutcome {
    if decoded.is_plain_utf8() {
        return diff_outcome(decoded.content(), cfg, path, base_dir, kind, rewrite);
    }
    let mut outcome =
        diff_outcome(decoded.content(), cfg, path, base_dir, kind, rewrite);
    outcome.diff = None;
    outcome.skipped.push(if rewrite == Rewrite::Fix {
        non_utf8_diff_skip()
    } else {
        diff_skip(
            "non-UTF-8 or BOM content has no applicable text diff; use ryl format",
        )
    });
    outcome
}

/// Whether the path can't be faithfully written in a diff header (not valid UTF-8, or holding
/// a control char). Either way the header would name a different path than the on-disk file,
/// so no consumer could apply the patch; `--diff` skips these.
fn path_unrepresentable_in_diff(path: &Path) -> bool {
    let name = path.as_os_str();
    name.to_str().is_none() || name.to_string_lossy().contains(char::is_control)
}

/// Unified diffs for each file's `rewrite`, reading from disk and never writing. A symlinked
/// input is skipped with a warning naming `flag` (parity with `--fix`); other un-diffable inputs (non-UTF-8/
/// BOM content, unparsable, or an unrepresentable name) are skipped via [`diff_outcome`].
///
/// # Errors
///
/// Returns an error if any file cannot be read.
pub fn diff_files(
    files: &[LintFile],
    rewrite: Rewrite,
    flag: &str,
) -> Result<DiffStats, String> {
    diff_files_with_notices(files, rewrite, flag, &[])
}

/// As [`diff_files`], with notices for omitted cached files before each input and after
/// the final input.
///
/// # Errors
///
/// Returns an error if any file cannot be read.
pub fn diff_files_with_notices(
    files: &[LintFile],
    rewrite: Rewrite,
    flag: &str,
    preceding_notices: &[Vec<u8>],
) -> Result<DiffStats, String> {
    let mut stats = DiffStats::default();
    let results = read_in_parallel(files, |decoded, (path, base_dir, cfg, kind)| {
        Ok(decoded_diff_outcome(
            &decoded, cfg, path, base_dir, *kind, rewrite,
        ))
    });
    for (index, ((path, ..), (result, stderr))) in files.iter().zip(results).enumerate()
    {
        if let Some(notices) = preceding_notices.get(index) {
            stats.notices.extend_from_slice(notices);
        }
        let notice_start = stats.notices.len();
        if let Some(notice) = symlink_notice(path, flag) {
            stats.notices.extend_from_slice(notice.as_bytes());
            stats
                .file_notices
                .push(stats.notices[notice_start..].to_vec());
            stats.skipped.push(path.clone());
            continue;
        }
        stats.notices.extend(stderr);
        match result {
            Ok(outcome) => stats.record(path, outcome, flag),
            Err(error) => {
                let _ = std::io::stderr().write_all(&stats.notices);
                return Err(error);
            }
        }
        stats
            .file_notices
            .push(stats.notices[notice_start..].to_vec());
    }
    if let Some(notices) = preceding_notices.get(files.len()) {
        stats.notices.extend_from_slice(notices);
    }
    Ok(stats)
}

/// Apply `rewrite` to each embedded YAML region of `markdown` and splice the results back
/// in, or `None` if nothing changed. File-shape rules are excluded per [`suppressed_rules`].
/// Each line regains the prefix the parser stripped (spaces, a blockquote `> `, or a tab), and
/// a region is rewritten only when re-applying that prefix reproduces the original raw bytes
/// exactly, and parsing the replacement recovers the full rewritten region. Ragged regions
/// and rewrites that change Markdown boundaries are left untouched. Regions are spliced
/// back-to-front so earlier edits do not shift later offsets.
#[must_use]
pub fn fix_markdown_str(
    markdown: &str,
    path: &Path,
    cfg: &YamlLintConfig,
    base_dir: &Path,
    rewrite: Rewrite,
) -> Option<String> {
    fix_markdown_cached(
        markdown,
        path,
        cfg,
        base_dir,
        rewrite,
        false,
        &mut FormatCache::default(),
    )
    .0
}

struct MarkdownRefusal {
    problem: crate::lint::LintProblem,
    blocks_preview: bool,
}

fn markdown_refusal(
    region: &EmbeddedRegion,
    reason: &str,
    blocks_preview: bool,
) -> MarkdownRefusal {
    let kind = match region.kind {
        RegionKind::FrontMatter => "front matter",
        RegionKind::FencedBlock => "fenced YAML block",
    };
    MarkdownRefusal {
        problem: crate::lint::LintProblem {
            line: region.line_offset + 1,
            column: region.col_offset + 1,
            level: crate::lint::Severity::Error,
            message: format!("{kind} {reason}"),
            rule: None,
        },
        blocks_preview,
    }
}

fn fix_markdown_cached(
    markdown: &str,
    path: &Path,
    cfg: &YamlLintConfig,
    base_dir: &Path,
    rewrite: Rewrite,
    refusal_lines_in_output: bool,
    cache: &mut FormatCache,
) -> (Option<String>, Vec<MarkdownRefusal>) {
    if crate::markdown_embed::markdown_has_unsupported_cr(markdown) {
        let skip = crate::markdown_embed::unsupported_cr_skip();
        let reported = refusal_lines_in_output || rewrite == Rewrite::Format;
        let refusals = reported.then_some(MarkdownRefusal {
            problem: skip,
            blocks_preview: false,
        });
        return (None, refusals.into_iter().collect());
    }
    let sources = MarkdownSources {
        front_matter: cfg.markdown_front_matter(),
        fenced_blocks: cfg.markdown_fenced_blocks(),
    };
    let mut regions = extract_regions(markdown, sources);
    regions.sort_by_key(|region| std::cmp::Reverse(region.raw_span.start));

    let mut out = markdown.to_string();
    let mut changed = false;
    let mut notices = Vec::new();
    let mut refusals = Vec::new();
    for region in &regions {
        if region.content.trim().is_empty() {
            continue;
        }
        let mut parsed = rewrite_parse_gate(&region.content, rewrite);
        let fixed = parsed.as_mut().ok().map(|parsed| {
            rewrite.apply(
                &region.content,
                cfg,
                (path, base_dir),
                suppressed_rules(),
                cache,
                Some(parsed),
            )
        });
        let written = fixed
            .as_ref()
            .filter(|fixed| **fixed != region.content)
            .and_then(|fixed| {
                write_region(&out, region, fixed, (sources, rewrite), &mut refusals)
            });
        if refusal_lines_in_output || rewrite == Rewrite::Format {
            if let Some((_, _, line_delta)) =
                written.as_ref().filter(|_| refusal_lines_in_output)
            {
                for problem in notices
                    .iter_mut()
                    .chain(refusals.iter_mut().map(|refusal| &mut refusal.problem))
                {
                    problem.line = problem.line.checked_add_signed(*line_delta).expect(
                        "rewritten region shifts a later refusal within the document",
                    );
                }
            }
            let mode = (rewrite, refusal_lines_in_output);
            if let Some((host, shown, strict)) =
                notice_source(markdown, region, &parsed, written.as_ref(), mode)
            {
                let context = (cfg, path, base_dir);
                let skips =
                    region_skips(&shown.content, strict, context, rewrite, cache);
                notices
                    .splice(0..0, crate::markdown_embed::in_host(host, shown, skips));
            }
        }
        let (Some((candidate, ..)), Ok(parsed), Some(fixed)) =
            (written, &parsed, &fixed)
        else {
            continue;
        };
        if let Some(mut problem) = rewrite_parse_notice(&region.content, fixed, parsed)
        {
            problem.line += region.line_offset;
            problem.column += region.col_offset;
            refusals.push(MarkdownRefusal {
                problem,
                blocks_preview: false,
            });
        }
        out = candidate;
        changed = true;
    }
    let mut problems: Vec<_> = notices
        .into_iter()
        .map(|problem| MarkdownRefusal {
            problem,
            blocks_preview: false,
        })
        .collect();
    problems.extend(refusals);
    (changed.then_some(out), problems)
}

/// The host and region `region`'s notices are read from, with the strict parse error that
/// replaces them; `None` where a preview reports a repaired region through its repair
/// notice alone.
fn notice_source<'a>(
    markdown: &'a str,
    region: &'a EmbeddedRegion,
    parsed: &Result<ParsedInput, crate::lint::LintProblem>,
    written: Option<&'a (String, EmbeddedRegion, isize)>,
    (rewrite, in_output): (Rewrite, bool),
) -> Option<(
    &'a str,
    &'a EmbeddedRegion,
    Option<crate::lint::LintProblem>,
)> {
    let input_strict = || {
        parsed.as_ref().map_or_else(
            |_| crate::lint::strict_parse_error(&region.content),
            |parsed| parsed.strict_error.clone(),
        )
    };
    match written {
        // Rewritten YAML parses strictly by construction, so only `--fix` re-checks it.
        Some((candidate, updated, _)) if in_output => Some((
            candidate.as_str(),
            updated,
            (rewrite == Rewrite::Fix)
                .then(|| crate::lint::strict_parse_error(&updated.content))
                .flatten(),
        )),
        Some(_) => input_strict().is_none().then_some((markdown, region, None)),
        None => Some((markdown, region, input_strict())),
    }
}

/// `out` with `region` rewritten to `fixed`, the rewritten region, and the lines it adds;
/// `None`, with any refusal, where writing it back would not reproduce `fixed`.
fn write_region(
    out: &str,
    region: &EmbeddedRegion,
    fixed: &str,
    (sources, rewrite): (MarkdownSources, Rewrite),
    refusals: &mut Vec<MarkdownRefusal>,
) -> Option<(String, EmbeddedRegion, isize)> {
    let Some((prefix, newline)) = region_prefix(out, region) else {
        let raw = &out[region.raw_span.clone()];
        if raw.contains("\r\n")
            && raw.matches('\n').count() > raw.matches("\r\n").count()
        {
            refusals.push(markdown_refusal(
                region,
                "mixes CRLF and LF line endings; cannot write it back safely",
                rewrite == Rewrite::Format,
            ));
        }
        return None;
    };
    let replacement = reindent(fixed, &prefix, newline);
    let mut candidate = out.to_owned();
    candidate.replace_range(region.raw_span.clone(), &replacement);
    let expected_span =
        region.raw_span.start..region.raw_span.start + replacement.len();
    let Some(updated) =
        extract_regions(&candidate, sources)
            .into_iter()
            .find(|updated| {
                updated.kind == region.kind
                    && updated.raw_span == expected_span
                    && updated.content.replace("\r\n", "\n")
                        == fixed.replace("\r\n", "\n")
            })
    else {
        if rewrite == Rewrite::Format {
            refusals.push(markdown_refusal(
                region,
                "would change Markdown fence boundaries; left unchanged",
                false,
            ));
        }
        return None;
    };
    let line_delta = replacement.matches('\n').count().cast_signed()
        - out[region.raw_span.clone()]
            .matches('\n')
            .count()
            .cast_signed();
    Some((candidate, updated, line_delta))
}

/// The prefix the parser stripped from each line of `region` and the host's line ending, or
/// `None` for a ragged region, which re-applying one prefix cannot reproduce.
pub(crate) fn region_prefix(
    markdown: &str,
    region: &EmbeddedRegion,
) -> Option<(String, &'static str)> {
    let raw = &markdown[region.raw_span.clone()];
    let newline = buffer_newline(raw);
    // `raw` starts at the first content line and `col_offset` (its stripped char count)
    // never spans a newline, so the first `col_offset` chars of `raw` are exactly the
    // prefix the parser stripped. The guard below re-checks it against every line, so a
    // ragged prefix still fails and is skipped.
    let prefix: String = raw.chars().take(region.col_offset).collect();
    (reindent(&region.content, &prefix, newline) == raw).then_some((prefix, newline))
}

/// Re-encode dedented region content into its host: each non-empty line regains `prefix` and
/// lines are joined with `newline`. Empty lines stay empty (matching how the parser dedents
/// blanks); the trailing newline is preserved.
fn reindent(content: &str, prefix: &str, newline: &str) -> String {
    let mut out = String::with_capacity(content.len());
    for piece in content.replace("\r\n", "\n").split_inclusive('\n') {
        let (line, terminated) = piece
            .strip_suffix('\n')
            .map_or((piece, false), |line| (line, true));
        if !line.is_empty() {
            out.push_str(prefix);
            out.push_str(line);
        }
        if terminated {
            out.push_str(newline);
        }
    }
    out
}

#[must_use]
pub fn apply_safe_fixes(
    input: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
) -> String {
    apply_safe_fixes_filtered(input, cfg, path, base_dir, &[])
}

/// Apply safe fixes, skipping any rule whose id is in `skip` (the markdown write-back path
/// passes [`suppressed_rules`]).
#[must_use]
pub fn apply_safe_fixes_filtered(
    input: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
    skip: &[&str],
) -> String {
    let passes = Passes::lint(cfg, path, base_dir, skip);
    let mut tracking = PassTracking {
        edited: &mut Vec::new(),
        reindented: None,
        strict_input: None,
        unprobed: false,
    };
    rewrite(input, TextVersion::new(input), &passes, &mut tracking).into_text()
}

/// The text `apply_safe_fixes` runs its fixers on: `input` where it is strictly valid,
/// else its indentation repair, or `None` where no repair makes it valid.
#[doc(hidden)]
#[must_use]
pub fn safe_fix_start(
    input: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
) -> Option<String> {
    let source = TextVersion::new(input);
    if crate::lint::parse_error_from_result(
        input,
        source.analysis().parsed().strict_error(),
    )
    .is_none()
    {
        return Some(input.to_owned());
    }
    repair(source.analysis(), &Passes::lint(cfg, path, base_dir, &[]))
        .map(TextVersion::into_text)
}

/// `input` (whose analysis is `source`) rewritten by `passes`.
pub(crate) fn rewrite<'a>(
    input: &'a str,
    mut source: TextVersion<'a>,
    passes: &Passes,
    tracking: &mut PassTracking<'_>,
) -> TextVersion<'static> {
    use crate::format::layout::Fallback;
    if crate::directives::disables_file(input) {
        crate::format::layout::stats::count(
            crate::format::layout::stats::Counter::Skip,
        );
        return TextVersion::new(input.to_string());
    }
    let strict = tracking.strict_input.unwrap_or_else(|| {
        crate::lint::parse_error_from_result(
            input,
            source.analysis().parsed().strict_error(),
        )
        .is_none()
    });
    if strict {
        source.mark_strict_valid();
    } else {
        let Some(repaired) = repair(source.analysis(), passes) else {
            return TextVersion::new(input.to_string());
        };
        tracking.edited.push(indentation::ID);
        source = repaired;
    }
    let mut current = source;
    for _ in 0..FIX_PIPELINE_MAX_PASSES {
        tracking.unprobed = true;
        let (mut output, ambiguous) =
            match crate::format::layout::format(&mut current, passes, tracking) {
                Ok(output) => (output, false),
                Err(Fallback::Ambiguous) => (current, true),
                Err(reason) => {
                    crate::format::layout::stats::Recording::fallback(reason);
                    return TextVersion::new(input.to_string());
                }
            };
        let truthy = !ambiguous
            && fix_rule(
                &mut output,
                truthy::ID,
                passes.truthy.as_ref(),
                passes,
                tracking.edited,
                truthy::fix,
            );
        let ordered = fix_rule(
            &mut output,
            key_ordering::ID,
            passes.key_ordering.as_ref(),
            passes,
            tracking.edited,
            |text, cfg| key_ordering::fix(text, cfg, &passes.per_line),
        );
        current = output;
        if !truthy && !ordered {
            break;
        }
    }
    current.into_owned()
}

pub(crate) fn ambiguous(input: Analysis<'_>) -> bool {
    crate::rules::block_scalar_chomping::ambiguous_blank_body_line_with_analysis(input)
        .is_some()
}

fn fix_rule<C>(
    content: &mut TextVersion<'_>,
    rule: &'static str,
    cfg: Option<&C>,
    passes: &Passes,
    edited: &mut Vec<&'static str>,
    mut fix: impl FnMut(&str, &C) -> Option<String>,
) -> bool {
    let Some(cfg) = cfg else {
        return false;
    };
    let guarded = passes
        .per_line
        .iter()
        .any(|entry| entry.rules.is_none_or(|ids| ids.contains(&rule)))
        || Directives::parse_per_line_with_analysis(
            content.analysis(),
            &passes.per_line,
        )
        .disables_any(rule);
    let mut changed = false;
    for _ in 0..RULE_FIX_MAX_ITERATIONS {
        let text = content.analysis().text();
        let Some(mut next) = fix(text, cfg) else {
            break;
        };
        if guarded {
            next = Directives::parse_per_line_with_analysis(
                content.analysis(),
                &passes.per_line,
            )
            .reconcile(rule, text, &next);
        }
        if next == text {
            break;
        }
        *content = TextVersion::new(next);
        edited.push(rule);
        changed = true;
    }
    changed
}

/// The line ending the final-newline fix appends.
#[derive(Clone)]
pub(crate) enum NewlinePolicy {
    Configured(String),
    /// Reuse the first line's ending so a `\r`-delimited file's appended final newline
    /// stays `\r` rather than falling back to LF.
    FirstBreak,
}

impl NewlinePolicy {
    pub(crate) fn newline<'a>(&'a self, content: &'a str) -> &'a str {
        match self {
            Self::Configured(newline) => newline,
            Self::FirstBreak => first_line_break(content).map_or("\n", |(_, nl)| nl),
        }
    }
}

/// Each fixer's resolved target, `None` (or `false`) where it does not run.
#[derive(Clone)]
pub(crate) struct Passes<'a> {
    pub(crate) preserve_events: bool,
    pub(crate) new_lines: Option<new_lines::Config>,
    pub(crate) comments: Option<comments::Config>,
    pub(crate) indentation: Option<indentation::Config>,
    pub(crate) repair_indentation: Option<indentation::Config>,
    pub(crate) comments_indentation: Option<comments_indentation::Config>,
    pub(crate) commas: Option<commas::Config>,
    pub(crate) braces: Option<braces::Config>,
    pub(crate) brackets: Option<brackets::Config>,
    pub(crate) colons: Option<colons::Config>,
    pub(crate) hyphens: Option<hyphens::Config>,
    pub(crate) final_newline: Option<NewlinePolicy>,
    pub(crate) quoted_strings: Option<quoted_strings::Config>,
    pub(crate) trailing_spaces: bool,
    pub(crate) document_start: Option<document_start::Config>,
    pub(crate) document_end: Option<document_end::Config>,
    pub(crate) empty_lines: Option<empty_lines::Config>,
    pub(crate) truthy: Option<truthy::Config>,
    pub(crate) key_ordering: Option<key_ordering::Config>,
    pub(crate) line_length: Option<line_length::Fold>,
    pub(crate) collection_style: Option<collection_style::Config>,
    /// Config `per-line-ignores` for this file; re-applied on each guarded re-parse since a
    /// structural fixer can shift which line a regex matches.
    pub(crate) per_line: Vec<PerLineRuleApply<'a>>,
}

impl<'a> Passes<'a> {
    /// The safe fixes `cfg` enables for `path`.
    fn lint(
        cfg: &'a YamlLintConfig,
        path: &Path,
        base_dir: &Path,
        skip: &[&str],
    ) -> Self {
        let on =
            |rule| !skip.contains(&rule) && rule_enabled(rule, cfg, path, base_dir);
        // Unlike the fixers, the ending ignores `fixable`: an unfixable `new-lines` still
        // dictates the appended newline.
        let newline = if cfg.rule_level(new_lines::ID).is_some()
            && !cfg.is_rule_ignored(new_lines::ID, path, base_dir)
        {
            NewlinePolicy::Configured(
                new_lines::expected_newline(
                    new_lines::Config::resolve(cfg),
                    new_lines::platform_newline(),
                )
                .into_owned(),
            )
        } else {
            NewlinePolicy::FirstBreak
        };
        Self {
            preserve_events: false,
            new_lines: on(new_lines::ID).then(|| new_lines::Config::resolve(cfg)),
            comments: on(comments::ID).then(|| comments::Config::resolve(cfg)),
            indentation: None,
            repair_indentation: (!skip.contains(&indentation::ID)
                && cfg
                    .fix()
                    .fixable()
                    .contains(&crate::config::FixRuleSelector::All)
                && !cfg.is_rule_ignored(indentation::ID, path, base_dir))
            .then(|| {
                crate::format::indentation_target(cfg, crate::format::indent_width(cfg))
            }),
            comments_indentation: on(comments_indentation::ID)
                .then(|| comments_indentation::Config::resolve(cfg)),
            commas: on(commas::ID).then(|| commas::Config::resolve(cfg)),
            braces: on(braces::ID).then(|| braces::Config::resolve(cfg)),
            brackets: on(brackets::ID).then(|| brackets::Config::resolve(cfg)),
            colons: on(colons::ID).then(|| colons::Config::resolve(cfg)),
            hyphens: on(hyphens::ID).then(|| hyphens::Config::resolve(cfg)),
            final_newline: on(new_line_at_end_of_file::ID).then_some(newline),
            quoted_strings: on(quoted_strings::ID)
                .then(|| quoted_strings::Config::resolve(cfg)),
            trailing_spaces: on(trailing_spaces::ID),
            document_start: on(document_start::ID)
                .then(|| document_start::Config::resolve(cfg)),
            document_end: on(document_end::ID)
                .then(|| document_end::Config::resolve(cfg)),
            empty_lines: on(empty_lines::ID).then(|| empty_lines::Config::resolve(cfg)),
            truthy: on(truthy::ID).then(|| truthy::Config::resolve(cfg)),
            key_ordering: on(key_ordering::ID)
                .then(|| key_ordering::Config::resolve(cfg, path)),
            line_length: None,
            collection_style: None,
            per_line: cfg.per_line_applies(path),
        }
    }
}

pub(crate) type ReindentCache = Option<ReindentEvidence>;

pub(crate) struct ReindentEvidence {
    epoch: std::rc::Rc<()>,
    config: indentation::Config,
    pub(crate) refused: Vec<indentation::Refusal>,
    pub(crate) kept_dash_lines: Vec<(usize, usize)>,
    /// The 1-based line and column of each consistent-width violation, when known.
    pub(crate) inconsistent: Option<Vec<(usize, usize)>>,
    /// Whether the plan showed the text holds no ambiguous block scalar.
    pub(crate) unambiguous: bool,
    /// Whether the plan left every collection in its configured style.
    pub(crate) settled: bool,
}

impl ReindentEvidence {
    pub(crate) fn new(
        input: Analysis<'_>,
        config: indentation::Config,
        refused: Vec<indentation::Refusal>,
        kept_dash_lines: Vec<(usize, usize)>,
        inconsistent: Option<Vec<(usize, usize)>>,
        (unambiguous, settled): (bool, bool),
    ) -> Self {
        Self {
            epoch: input.epoch(),
            config,
            refused,
            kept_dash_lines,
            inconsistent,
            unambiguous,
            settled,
        }
    }

    pub(crate) fn matches(
        &self,
        input: Analysis<'_>,
        config: indentation::Config,
    ) -> bool {
        self.describes(input) && self.config == config
    }

    pub(crate) fn describes(&self, input: Analysis<'_>) -> bool {
        std::rc::Rc::ptr_eq(&self.epoch, &input.epoch())
    }

    /// Whether the plan refused nothing, kept no dash, and left no ambiguous scalar or
    /// unconverted collection.
    pub(crate) fn is_quiet(&self) -> bool {
        self.unambiguous
            && self.settled
            && self.kept_dash_lines.is_empty()
            && self
                .refused
                .iter()
                .all(|refusal| matches!(refusal.cause, indentation::Cause::Disabled))
    }
}

pub(crate) struct PassTracking<'a> {
    pub edited: &'a mut Vec<&'static str>,
    pub reindented: Option<&'a mut ReindentCache>,
    pub strict_input: Option<bool>,
    pub unprobed: bool,
}

/// `input` re-indented to the repair target, where that alone makes it strictly valid
/// without changing its representation.
pub(crate) fn repair(
    input: Analysis<'_>,
    passes: &Passes,
) -> Option<TextVersion<'static>> {
    let target = passes.repair_indentation?;
    let spaced = separate_flow_keys(input).map(TextVersion::observed);
    let source = spaced.as_ref().map_or(input, TextVersion::analysis);
    let repaired = Directives::parse_per_line_with_analysis(source, &passes.per_line)
        .reconcile(
            indentation::ID,
            source.text(),
            &indentation::reindent_with_analysis(source, &target).0.text,
        );
    if repaired == input.text() {
        return None;
    }
    let mut version = TextVersion::observed(repaired);
    let output = version.analysis();
    let valid = output.parsed().valid
        && crate::lint::parse_error_from_result(
            output.text(),
            output.parsed().strict_error(),
        )
        .is_none()
        && representation_matches(input, output);
    valid.then(|| {
        version.mark_strict_valid();
        version
    })
}

/// `input` with a space after each flow `?` that a flow indicator follows, which relaxed
/// parsing reads as an empty explicit key.
fn separate_flow_keys(input: Analysis<'_>) -> Option<String> {
    let flow_key = |error: &ryl_yaml_parser::ScanError| {
        matches!(
            error.kind(),
            ryl_yaml_parser::ErrorKind::PlainScalarStartsWithIndicatorBeforeFlowIndicator {
                indicator: '?'
            }
        )
    };
    let mut error = input
        .parsed()
        .strict_error()
        .filter(|e| flow_key(e))
        .cloned();
    let mut spaced: Option<String> = None;
    while let Some(found) = error {
        let at = found
            .marker()
            .byte_offset()
            .expect("string input has byte offsets");
        let text = spaced.get_or_insert_with(|| input.text().to_owned());
        text.insert(at + 1, ' ');
        error = crate::lint::scan(text, true).err().filter(flow_key);
    }
    spaced
}

fn rule_enabled(
    rule: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
) -> bool {
    cfg.rule_level(rule).is_some()
        && !cfg.is_rule_ignored(rule, path, base_dir)
        && cfg.fix().allows_rule(rule)
}
