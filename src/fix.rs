use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use granit_parser::{Event, Parser, Span};
use rayon::prelude::*;
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
use crate::text_version::{Analysis, TextVersion};

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

/// Every rule with a safe `--fix`, in application order; extend together with the `apply`
/// sequence in `FixContext::pass` when adding a safe fixer. The LSP drives per-rule
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
        parsed: Option<&ParsedInput<'_>>,
    ) -> String {
        match self {
            Self::Fix => apply_safe_fixes_filtered(input, cfg, path, base_dir, skip),
            Self::Format => cache
                .format_with_events(input, cfg, path, skip, parsed)
                .0
                .clone(),
        }
    }
}

pub(crate) struct ParsedInput<'a> {
    pub(crate) events: Option<Vec<(Event<'a>, Span)>>,
    pub(crate) strict_error: Option<crate::lint::LintProblem>,
}

pub(crate) fn rewrite_parse_gate(
    content: &str,
    rewrite: Rewrite,
) -> Result<ParsedInput<'_>, crate::lint::LintProblem> {
    if rewrite == Rewrite::Fix {
        let strict_error = crate::lint::strict_parse_error(content);
        if strict_error.is_some()
            && let Some(problem) = crate::lint::relaxed_parse_error(content)
        {
            return Err(problem);
        }
        return Ok(ParsedInput {
            events: None,
            strict_error,
        });
    }
    let mut parsed = Parser::new_from_str_with_options(
        content,
        granit_parser::options! { strict_indentation: true },
    )
    .collect::<Result<Vec<_>, _>>();
    let strict_error =
        crate::lint::parse_error_from_result(content, parsed.as_ref().err());
    if strict_error.is_some() {
        parsed = Parser::new_from_str(content).collect::<Result<Vec<_>, _>>();
        if let Some(problem) =
            crate::lint::parse_error_from_result(content, parsed.as_ref().err())
        {
            return Err(problem);
        }
    }
    Ok(ParsedInput {
        events: Some(parsed.expect("the parse gate accepted these events")),
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
            let events = match rewrite_parse_gate(content, rewrite) {
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
                Some(&events),
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
            // Read from the *fixed* bytes (what gets written) so the reported line stays
            // correct after an earlier region's fix shifts the line count.
            let mut skipped = markdown_region_problems(
                fixed.as_deref().unwrap_or(content),
                cfg,
                |region| {
                    region_skips(
                        &region.content,
                        cfg,
                        path,
                        base_dir,
                        rewrite,
                        &mut cache,
                    )
                },
            );
            skipped.extend(refusals.into_iter().map(|refusal| refusal.problem));
            (fixed, skipped)
        }
    }
}

/// Apply `rewrite` to each file in place.
#[must_use]
pub fn rewrite_files(files: &[LintFile], rewrite: Rewrite) -> FixStats {
    let mut stats = FixStats::default();
    for batch in hard_link_batches(files) {
        let results = read_in_parallel(
            &files[batch.clone()],
            |decoded, (path, base_dir, cfg, kind)| {
                let (rewritten, skipped) =
                    rewrite_str(decoded.content(), cfg, path, base_dir, *kind, rewrite);
                (decoded, rewritten, skipped)
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
            let written = result.and_then(|(decoded, rewritten, skipped)| {
                if let Some(rewritten) = &rewritten {
                    decoded.write(path, rewritten)?;
                }
                Ok((rewritten.is_some(), skipped))
            });
            match written {
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

/// `compute` over each file in parallel, never reading a symlink, paired with the stderr it
/// produced so the caller can replay both in input order.
fn read_in_parallel<T: Send>(
    files: &[LintFile],
    compute: impl Fn(decoder::DecodedFile, &LintFile) -> T + Sync,
) -> Vec<(Result<T, String>, Vec<u8>)> {
    files
        .par_iter()
        .map(|file| {
            crate::cli_support::capture_file_stderr(|| {
                if is_symlink(&file.0) {
                    Err(format!("skipped symlink {}", file.0.display()))
                } else {
                    decoder::read_file_lossless(&file.0)
                        .map(|decoded| compute(decoded, file))
                }
            })
        })
        .collect()
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
    pub problems: Vec<(PathBuf, Vec<crate::lint::LintProblem>)>,
}

impl DiffStats {
    pub fn record(&mut self, path: &Path, outcome: DiffOutcome, flag: &str) {
        self.changed_files += usize::from(outcome.changed);
        if !outcome.skipped.is_empty() {
            self.skipped.push(path.to_path_buf());
        }
        if let Some(diff) = outcome.diff {
            self.diffs.push(diff);
        }
        for problem in &outcome.skipped {
            self.notices
                .extend_from_slice(skip_notice(path, problem, flag).as_bytes());
        }
        if !outcome.problems.is_empty() {
            self.problems.push((path.to_path_buf(), outcome.problems));
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
    if rewrite == Rewrite::Format && outcome.changed {
        outcome.problems.extend(crate::format::problems(
            content, cfg, path, kind, &mut cache,
        ));
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
            let events = match rewrite_parse_gate(content, rewrite) {
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
                Some(&events),
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
            // A bare-`\r` markdown host is skipped upstream (`fix_markdown_str` returns
            // `None`), so content reaching `render_unified_diff` never carries a bare `\r`.
            let (fixed, refusals) = fix_markdown_cached(
                content, path, cfg, base_dir, rewrite, false, cache,
            );
            // Report skips against the *original* content: `--diff` never writes, so the file
            // stays `content` and a skip notice must point at the original line (the in-place
            // path uses `fixed` because it writes it).
            let mut skips = |markdown| {
                markdown_region_problems(markdown, cfg, |region| {
                    region_skips(&region.content, cfg, path, base_dir, rewrite, cache)
                })
            };
            let mut skipped = skips(content);
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
            let changed = fixed.is_some()
                || refusals.iter().any(|refusal| refusal.blocks_preview);
            let diff =
                fixed.and_then(|fixed| render_unified_diff(content, &fixed, path));
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

/// A Markdown region's skips: its parse error, else unsafe scalar or unsorted mappings.
fn region_skips(
    region: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
    rewrite: Rewrite,
    cache: &mut FormatCache,
) -> Vec<crate::lint::LintProblem> {
    crate::lint::strict_parse_error(region).map_or_else(
        || unfixed_notices(region, cfg, path, base_dir, rewrite, cache),
        |problem| vec![problem],
    )
}

fn rewrite_parse_notice(
    original: &str,
    fixed: &str,
    parsed: &ParsedInput<'_>,
) -> Option<crate::lint::LintProblem> {
    if crate::directives::disables_file(original) {
        return None;
    }
    parsed.strict_error.clone().map(|mut problem| {
        if original != fixed {
            problem.level = crate::lint::Severity::Warning;
            problem.message =
                "input was YAML the spec rejects; repaired indentation".to_string();
        }
        problem
    })
}

fn repair_events_match(original: &str, repaired: &str) -> bool {
    let before = repair_representation(original);
    let after = repair_representation(repaired);
    before
        .iter()
        .map(|doc| &doc.events)
        .eq(after.iter().map(|doc| &doc.events))
}

fn repair_representation(
    text: &str,
) -> Vec<crate::rules::support::event_compare::Document<'_>> {
    use crate::rules::support::yaml_version;
    use granit_parser::{ScalarStyle, StructureStyle};
    let (documents, parsed) = crate::rules::support::event_compare::documents(text);
    let mut documents = parsed
        .then_some(documents)
        .expect("repair operands have complete parses");
    for document in &mut documents {
        let yaml11 = matches!(document.events.first(), Some(Event::DocumentStart(_, Some(version)))
            if yaml_version::resolves_as_yaml_1_1(Some(yaml_version::event_version(*version))));
        for event in &mut document.events {
            match event {
                Event::DocumentStart(explicit, _) => *explicit = false,
                Event::SequenceStart(style, ..) | Event::MappingStart(style, ..) => {
                    *style = StructureStyle::Block;
                }
                Event::Scalar(value, style, _, tag)
                    if (*style != ScalarStyle::Plain
                        || tag.is_some()
                        || (matches!(
                            crate::yaml_dom::Scalar::resolve_plain_scalar(
                                value.clone()
                            ),
                            crate::yaml_dom::Scalar::String(_)
                        ) && !crate::yaml_dom::is_core_schema_int_spelling(
                            value,
                        ) && (!yaml11
                            || !yaml_version::resolves_to_nonstring_in_yaml_1_1(
                                value,
                            )))) =>
                {
                    *style = ScalarStyle::DoubleQuoted;
                }
                _ => {}
            }
        }
    }
    documents
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
        decoded_diff_outcome(&decoded, cfg, path, base_dir, *kind, rewrite)
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
        return (None, Vec::new());
    }
    let sources = MarkdownSources {
        front_matter: cfg.markdown_front_matter(),
        fenced_blocks: cfg.markdown_fenced_blocks(),
    };
    let mut regions = extract_regions(markdown, sources);
    regions.sort_by_key(|region| std::cmp::Reverse(region.raw_span.start));

    let mut out = markdown.to_string();
    let mut changed = false;
    let mut refusals = Vec::new();
    for region in &regions {
        if region.content.trim().is_empty() {
            continue;
        }
        let Ok(parsed) = rewrite_parse_gate(&region.content, rewrite) else {
            continue;
        };
        let fixed = rewrite.apply(
            &region.content,
            cfg,
            (path, base_dir),
            suppressed_rules(),
            cache,
            Some(&parsed),
        );
        if fixed == region.content {
            continue;
        }
        let Some((prefix, newline)) = region_prefix(markdown, region) else {
            let raw = &markdown[region.raw_span.clone()];
            if raw.contains("\r\n")
                && raw.matches('\n').count() > raw.matches("\r\n").count()
            {
                refusals.push(markdown_refusal(
                    region,
                    "mixes CRLF and LF line endings; cannot write it back safely",
                    rewrite == Rewrite::Format,
                ));
            }
            continue;
        };
        let replacement = reindent(&fixed, &prefix, newline);
        let mut candidate = out.clone();
        candidate.replace_range(region.raw_span.clone(), &replacement);
        let expected_span =
            region.raw_span.start..region.raw_span.start + replacement.len();
        if !extract_regions(&candidate, sources).iter().any(|updated| {
            updated.kind == region.kind
                && updated.raw_span == expected_span
                && updated.content.replace("\r\n", "\n") == fixed.replace("\r\n", "\n")
        }) {
            if rewrite == Rewrite::Format {
                refusals.push(markdown_refusal(
                    region,
                    "would change Markdown fence boundaries; left unchanged",
                    false,
                ));
            }
            continue;
        }
        if refusal_lines_in_output {
            let line_delta = replacement.matches('\n').count().cast_signed()
                - markdown[region.raw_span.clone()]
                    .matches('\n')
                    .count()
                    .cast_signed();
            for refusal in &mut refusals {
                refusal.problem.line =
                    refusal.problem.line.checked_add_signed(line_delta).expect(
                        "rewritten region shifts a later refusal within the document",
                    );
            }
        }
        if let Some(mut problem) =
            rewrite_parse_notice(&region.content, &fixed, &parsed)
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
    (changed.then_some(out), refusals)
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
    apply_safe_fixes_capped(
        input,
        cfg,
        path,
        base_dir,
        skip,
        FIX_PIPELINE_MAX_PASSES,
        &mut std::io::stderr(),
    )
}

/// [`apply_safe_fixes_filtered`] with the pass cap and error sink exposed.
#[must_use]
pub fn apply_safe_fixes_capped(
    input: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
    skip: &[&str],
    max_passes: usize,
    err: &mut dyn Write,
) -> String {
    let passes = Passes::lint(cfg, path, base_dir, skip);
    run_passes(input, &passes, path, max_passes, err, &mut Vec::new())
}

/// The line ending the final-newline fix appends.
pub(crate) enum NewlinePolicy {
    Configured(String),
    /// Reuse the first line's ending so a `\r`-delimited file's appended final newline
    /// stays `\r` rather than falling back to LF.
    FirstBreak,
}

impl NewlinePolicy {
    fn newline<'a>(&'a self, content: &'a str) -> &'a str {
        match self {
            Self::Configured(newline) => newline,
            Self::FirstBreak => first_line_break(content).map_or("\n", |(_, nl)| nl),
        }
    }
}

/// Each fixer's resolved target, `None` (or `false`) where it does not run.
pub(crate) struct Passes<'a> {
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

/// Run `passes` over `input` until a pass changes nothing, pushing onto `edited` each rule
/// whose fix changed the text. Like ruff, if a pass after the last allowed one would still
/// change the text, report it to `err` and return the text as of the cap.
pub(crate) fn run_passes(
    input: &str,
    passes: &Passes,
    path: &Path,
    max_passes: usize,
    err: &mut dyn Write,
    edited: &mut Vec<&'static str>,
) -> String {
    run_passes_with_events(
        input,
        passes,
        path,
        max_passes,
        err,
        &mut PassTracking {
            edited,
            reindented: None,
            strict_input: None,
        },
        None,
    )
}

pub(crate) type ReindentCache =
    Option<(String, indentation::Config, indentation::Reindented)>;

pub(crate) struct PassTracking<'a> {
    pub edited: &'a mut Vec<&'static str>,
    pub reindented: Option<&'a mut ReindentCache>,
    pub strict_input: Option<bool>,
}

fn reindent_for_pass(
    buffer: &str,
    cfg: &indentation::Config,
    events: Option<&[(Event<'_>, Span)]>,
    cache: Option<&mut ReindentCache>,
) -> Option<String> {
    let result = indentation::reindent_with_events(buffer, cfg, events);
    if result.text != buffer {
        return Some(result.text);
    }
    if let Some(slot) = cache {
        *slot = Some((buffer.to_string(), *cfg, result));
    }
    None
}

pub(crate) fn run_passes_with_events(
    input: &str,
    passes: &Passes,
    path: &Path,
    max_passes: usize,
    err: &mut dyn Write,
    tracking: &mut PassTracking<'_>,
    initial_events: Option<&[(Event<'_>, Span)]>,
) -> String {
    if crate::directives::disables_file(input) {
        return input.to_string();
    }
    let repaired;
    let original = input;
    let mut events = initial_events;
    let input = if tracking
        .strict_input
        .unwrap_or_else(|| crate::lint::strict_parse_error(input).is_none())
    {
        input
    } else {
        let Some(target) = passes.repair_indentation else {
            return input.to_string();
        };
        repaired = strict_output(
            input,
            Directives::parse_with_per_line(input, &passes.per_line).reconcile(
                indentation::ID,
                input,
                &indentation::reindent_with_events(input, &target, events).text,
            ),
        );
        if repaired == input || !repair_events_match(input, &repaired) {
            return input.to_string();
        }
        tracking.edited.push(indentation::ID);
        events = None;
        &repaired
    };
    let formatted = strict_output(
        original,
        run_valid_passes(input, passes, path, max_passes, err, tracking, events),
    );
    if original != input && !repair_events_match(original, &formatted) {
        return original.to_string();
    }
    formatted
}

fn strict_output(original: &str, formatted: String) -> String {
    if formatted != original && crate::lint::strict_parse_error(&formatted).is_some() {
        original.to_string()
    } else {
        formatted
    }
}

fn run_valid_passes(
    input: &str,
    passes: &Passes,
    path: &Path,
    max_passes: usize,
    err: &mut dyn Write,
    tracking: &mut PassTracking<'_>,
    initial_events: Option<&[(Event<'_>, Span)]>,
) -> String {
    let parsed = passes
        .collection_style
        .filter(|_| initial_events.is_none())
        .map(|_| Parser::new_from_str(input).collect::<Result<Vec<_>, _>>());
    let mut content = TextVersion::new(input);
    if let Some(events) = initial_events.or_else(|| {
        parsed
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .map(Vec::as_slice)
    }) {
        content.analysis().seed_events(events);
    }
    let mut ctx = FixContext {
        passes,
        directives: Directives::parse_with_per_line(input, &passes.per_line),
        edit_epoch: 0,
        clean_rules: HashMap::new(),
        clean_syntax_epoch: None,
    };
    if crate::rules::block_scalar_chomping::ambiguous_blank_body_line_with_analysis(
        content.analysis(),
    )
    .is_some()
    {
        // Key ordering verifies its output before applying it.
        ctx.apply(
            &mut content,
            tracking.edited,
            key_ordering::ID,
            passes.key_ordering.as_ref(),
            |view, cfg| key_ordering::fix(view.text(), cfg, &passes.per_line),
        );
        if content.analysis().text() != input && max_passes > 1 {
            return run_passes(
                &content,
                passes,
                path,
                max_passes - 1,
                err,
                tracking.edited,
            );
        }
        return content.into_text();
    }
    for pass in 0..max_passes {
        let events = if pass == 0 {
            initial_events.or_else(|| {
                parsed
                    .as_ref()
                    .and_then(|result| result.as_ref().ok())
                    .map(Vec::as_slice)
            })
        } else {
            None
        };
        let changed = ctx.pass(
            &mut content,
            tracking.edited,
            events,
            tracking.reindented.as_deref_mut(),
        );
        if !changed {
            return content.into_text();
        }
    }
    let mut changed_rules = Vec::new();
    let mut probe = TextVersion::new(content.analysis().text());
    if ctx.pass(&mut probe, &mut changed_rules, None, None) {
        changed_rules.dedup();
        let warning = format!(
            "\nerror: Failed to converge after {max_passes} iterations.\n\n\
             This indicates a bug in ryl. If you could open an issue at:\n\n    \
             https://github.com/owenlamont/ryl/issues/new?title=%5BInfinite%20loop%5D\n\n\
             ...quoting the contents of `{}`, the rule ids {}, along with the ryl config \
             and executed command, we'd be very appreciative!\n\n",
            cwd_relative_label(path),
            changed_rules.join(", "),
        );
        crate::cli_support::write_file_stderr(err, warning.as_bytes());
    }
    content.into_text()
}

/// Shared arguments for a sequence of rule fixes. `apply` is a method so it can be generic
/// over the fix closure (a capturing closure cannot), avoiding dynamic dispatch.
struct FixContext<'a> {
    passes: &'a Passes<'a>,
    /// Parsed once from the original input. `disables_any` is stable across fixes (no fixer
    /// adds or removes a directive comment), so the per-rule guard reads it without re-parsing.
    directives: Directives,
    edit_epoch: usize,
    clean_rules: HashMap<&'static str, usize>,
    clean_syntax_epoch: Option<usize>,
}

impl FixContext<'_> {
    /// One pass of every enabled fixer, pushing onto `changed_rules` each rule whose fix
    /// changed the text.
    fn pass(
        &mut self,
        content: &mut TextVersion<'_>,
        changed_rules: &mut Vec<&'static str>,
        events: Option<&[(Event<'_>, Span)]>,
        reindented: Option<&mut ReindentCache>,
    ) -> bool {
        let passes = self.passes;
        let input = content.to_string();
        macro_rules! fix {
            ($rule:ident) => {
                fix!($rule, passes.$rule.as_ref(), |view, cfg| $rule::fix(
                    view.text(),
                    cfg
                ))
            };
            ($rule:ident, $cfg:expr, $fix:expr $(,)?) => {
                self.apply(content, changed_rules, $rule::ID, $cfg, $fix)
            };
        }
        fix!(new_lines, passes.new_lines.as_ref(), |view, cfg| {
            new_lines::fix(view.text(), *cfg, new_lines::platform_newline())
        });
        fix!(comments);
        fix!(comments_indentation);
        fix!(commas, passes.commas.as_ref(), commas::fix_with_analysis);
        let syntax_epoch = self.edit_epoch;
        if self.clean_syntax_epoch != Some(syntax_epoch) {
            if passes.collection_style.is_some() || passes.indentation.is_some() {
                self.syntax_pass(content, &input, changed_rules, events, reindented);
            } else {
                self.lint_syntax_pass(content, changed_rules);
            }
        }
        if self.edit_epoch == syntax_epoch {
            self.clean_syntax_epoch = Some(syntax_epoch);
        }
        fix!(
            trailing_spaces,
            passes.trailing_spaces.then_some(&()),
            |view, ()| trailing_spaces::fix_with_analysis(view)
        );
        fix!(document_start);
        fix!(document_end, passes.document_end.as_ref(), |view, cfg| {
            document_end::fix_with_analysis(view, *cfg)
        });
        fix!(
            empty_lines,
            passes.empty_lines.as_ref(),
            empty_lines::fix_with_analysis
        );
        fix!(truthy);
        fix!(key_ordering, passes.key_ordering.as_ref(), |view, cfg| {
            key_ordering::fix(view.text(), cfg, &passes.per_line)
        });
        // `fold` skips disabled lines itself: `Directives::reconcile` duplicates a split line.
        if let Some(cfg) = passes.line_length
            && let Some(folded) = line_length::fold(content, cfg)
        {
            changed_rules.push(line_length::ID);
            let plan = content.analysis().plan(folded);
            content
                .apply(plan)
                .expect("fold plan belongs to the current epoch");
            self.edit_epoch += 1;
        }
        content.analysis().text() != input
    }

    fn lint_syntax_pass(
        &mut self,
        content: &mut TextVersion<'_>,
        changed_rules: &mut Vec<&'static str>,
    ) {
        let passes = self.passes;
        macro_rules! fix {
            ($rule:ident) => {
                fix!($rule, |view, cfg| $rule::fix(view.text(), cfg))
            };
            ($rule:ident, $fix:expr) => {
                self.apply(
                    content,
                    changed_rules,
                    $rule::ID,
                    passes.$rule.as_ref(),
                    $fix,
                )
            };
        }
        fix!(braces, braces::fix_with_analysis);
        fix!(brackets, brackets::fix_with_analysis);
        fix!(colons);
        fix!(hyphens);
        self.apply(
            content,
            changed_rules,
            new_line_at_end_of_file::ID,
            passes.final_newline.as_ref(),
            |view, policy| {
                new_line_at_end_of_file::fix(view.text(), policy.newline(view.text()))
            },
        );
        fix!(quoted_strings);
    }

    fn syntax_pass(
        &mut self,
        content: &mut TextVersion<'_>,
        input: &str,
        changed_rules: &mut Vec<&'static str>,
        events: Option<&[(Event<'_>, Span)]>,
        mut reindented: Option<&mut ReindentCache>,
    ) {
        let passes = self.passes;
        macro_rules! fix {
            ($rule:ident) => {
                fix!($rule, |view, cfg| $rule::fix(view.text(), cfg))
            };
            ($rule:ident, $fix:expr) => {
                self.apply(
                    content,
                    changed_rules,
                    $rule::ID,
                    passes.$rule.as_ref(),
                    $fix,
                )
            };
            ($rule:ident, $cfg:expr, $fix:expr $(,)?) => {
                self.apply(content, changed_rules, $rule::ID, $cfg, $fix)
            };
        }
        let snapshot = content.to_string();
        let reparsed = (snapshot != input || events.is_none())
            .then(|| Parser::new_from_str(&snapshot).collect::<Result<Vec<_>, _>>());
        let shared = events
            .filter(|_| snapshot == input)
            .or_else(|| {
                reparsed
                    .as_ref()
                    .and_then(|result| result.as_ref().ok())
                    .map(Vec::as_slice)
            })
            .inspect(|events| content.analysis().seed_events(events));
        let current_events = |buffer: &str| shared.filter(|_| snapshot == buffer);
        if let Some(cfg) = passes.collection_style
            && let Some((restyled, rules)) = collection_style::restyle(
                content,
                cfg,
                passes.indentation.as_ref(),
                current_events(content),
            )
        {
            changed_rules.extend(rules);
            let plan = content.analysis().plan(restyled);
            content
                .apply(plan)
                .expect("collection plan belongs to the current epoch");
            self.edit_epoch += 1;
        }
        fix!(braces, braces::fix_with_analysis);
        fix!(brackets, brackets::fix_with_analysis);
        fix!(colons);
        fix!(hyphens);
        let indent_snapshot = passes
            .indentation
            .filter(|_| snapshot != content.analysis().text())
            .map(|_| content.to_string());
        let indent_reparsed = indent_snapshot
            .as_deref()
            .map(|text| Parser::new_from_str(text).collect::<Result<Vec<_>, _>>());
        let indent_events = indent_reparsed
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .map(Vec::as_slice);
        if let Some(events) = indent_events {
            content.analysis().seed_events(events);
        }
        let events_after_spacing = |buffer: &str| {
            if indent_snapshot.as_deref() == Some(buffer) {
                indent_events
            } else {
                current_events(buffer)
            }
        };
        fix!(indentation, passes.indentation.as_ref(), |view, cfg| {
            let buffer = view.text();
            reindent_for_pass(
                buffer,
                cfg,
                events_after_spacing(buffer),
                reindented.as_deref_mut(),
            )
        },);
        fix!(
            new_line_at_end_of_file,
            passes.final_newline.as_ref(),
            |view, policy| new_line_at_end_of_file::fix(
                view.text(),
                policy.newline(view.text())
            )
        );
        fix!(
            quoted_strings,
            passes.quoted_strings.as_ref(),
            |view, cfg| {
                let buffer = view.text();
                quoted_strings::fix_with_events(
                    buffer,
                    cfg,
                    events_after_spacing(buffer),
                )
            },
        );
    }

    fn apply<C>(
        &mut self,
        content: &mut TextVersion<'_>,
        changed_rules: &mut Vec<&'static str>,
        rule: &'static str,
        cfg: Option<&C>,
        mut fix: impl FnMut(Analysis<'_>, &C) -> Option<String>,
    ) {
        let Some(cfg) = cfg else {
            return;
        };
        if self.clean_rules.get(rule) == Some(&self.edit_epoch) {
            return;
        }

        // Run the fix to a fixed point: one pass is not enough where a fix exposes a follow-up
        // diagnostic (e.g. quoted-strings double-to-single leaves a now-redundant pair to
        // remove), which would leave the single `--fix` non-idempotent. A well-behaved fixer
        // returns None at completion, so the loop exits after at most one extra no-op call.
        //
        // A guarded rule reconciles each pass so the fixer's edits to disabled lines are
        // reverted, re-parsing after a change since structural fixers shift line numbers.
        // Guard on an inline directive disabling this rule OR any per-line entry targeting it:
        // a content regex can newly match a line a fixer *produces*, so re-parse even if no
        // original line matched.
        let per_line = &self.passes.per_line;
        let guarded = self.directives.disables_any(rule)
            || per_line
                .iter()
                .any(|entry| entry.rules.is_none_or(|ids| ids.contains(&rule)));
        let current = content;
        let mut directives = Directives::default();
        if guarded {
            directives = Directives::parse_with_per_line(current, per_line);
        }
        for _ in 0..RULE_FIX_MAX_ITERATIONS {
            let Some(next) = fix(current.analysis(), cfg) else {
                self.clean_rules.insert(rule, self.edit_epoch);
                break;
            };
            let next = if guarded {
                directives.reconcile(rule, current, &next)
            } else {
                next
            };
            let plan = current.analysis().plan(next);
            if !current
                .apply(plan)
                .expect("rule plan belongs to the current epoch")
            {
                self.clean_rules.insert(rule, self.edit_epoch);
                break;
            }
            changed_rules.push(rule);
            self.edit_epoch += 1;
            if guarded {
                directives = Directives::parse_with_per_line(current, per_line);
            }
        }
    }
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
