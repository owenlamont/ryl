use std::io::Write;
use std::path::{Path, PathBuf};

use similar::TextDiff;

use crate::cli_support::LintFile;
use crate::config::{SourceKind, YamlLintConfig};
use crate::decoder;
use crate::directives::{Directives, PerLineRuleApply};
use crate::markdown_embed::{
    EmbeddedRegion, MarkdownSources, extract_regions, markdown_region_problems,
};
use crate::rules::support::line_syntax::{buffer_newline, first_line_break};
use crate::rules::{
    braces, brackets, commas, comments, comments_indentation, document_end,
    document_start, empty_lines, key_ordering, new_line_at_end_of_file, new_lines,
    quoted_strings, trailing_spaces, truthy,
};

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
pub const SAFE_FIX_RULE_IDS: [&str; 14] = [
    new_lines::ID,
    comments::ID,
    comments_indentation::ID,
    commas::ID,
    braces::ID,
    brackets::ID,
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
    /// Files left untouched because they do not parse, with the parse error so the caller can
    /// say why `--fix` refused them.
    pub skipped: Vec<(PathBuf, crate::lint::LintProblem)>,
}

/// One file's in-place fix result. A file may both change and carry skips: a Markdown file
/// can fix some embedded regions while skipping others that do not parse. For a plain YAML
/// file `skipped` holds at most one entry (the whole-file parse error).
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
        path: &Path,
        base_dir: &Path,
        skip: &[&str],
    ) -> String {
        match self {
            Self::Fix => apply_safe_fixes_filtered(input, cfg, path, base_dir, skip),
            Self::Format => crate::format::format_str(input, cfg, path, skip),
        }
    }
}

/// `std::fs::write` follows symlinks, so `--fix` (and `--diff`, its preview) skips a
/// symlinked input with a warning rather than let an untrusted tree redirect the write
/// (`innocent.yaml -> ~/.bashrc`); read-only linting is unaffected. Best-effort: only the
/// final component is checked, not atomically (a full defense needs `O_NOFOLLOW`).
fn refuse_symlink(path: &Path, flag: &str) -> bool {
    if std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        eprintln!(
            "skipping {}: refusing to follow a symlink for {flag}",
            crate::cli_support::sanitize_control(&path.display().to_string())
        );
        return true;
    }
    false
}

/// Apply `rewrite` to `path` in place.
///
/// # Errors
///
/// Returns an error if the file cannot be read or the rewritten contents cannot be written.
pub fn rewrite_in_place(
    path: &Path,
    cfg: &YamlLintConfig,
    base_dir: &Path,
    kind: SourceKind,
    rewrite: Rewrite,
) -> Result<FixOutcome, String> {
    if refuse_symlink(path, rewrite.flag()) {
        return Ok(FixOutcome::default());
    }
    let decoded = decoder::read_file_lossless(path)?;
    let (rewritten, skipped) =
        rewrite_str(decoded.content(), cfg, path, base_dir, kind, rewrite);
    if let Some(rewritten) = &rewritten {
        decoded.write(path, rewritten)?;
    }
    Ok(FixOutcome {
        changed: rewritten.is_some(),
        skipped,
    })
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
            if let Some(problem) = crate::lint::parse_error(content) {
                return (None, vec![problem]);
            }
            let fixed = rewrite.apply(content, cfg, path, base_dir, &[]);
            let skipped = unfixed_notices(&fixed, cfg, path, base_dir, rewrite);
            ((fixed != content).then_some(fixed), skipped)
        }
        SourceKind::Markdown => {
            let fixed = fix_markdown_str(content, path, cfg, base_dir, rewrite);
            // Read from the *fixed* bytes (what gets written) so the reported line stays
            // correct after an earlier region's fix shifts the line count.
            let skipped = markdown_region_problems(
                fixed.as_deref().unwrap_or(content),
                cfg,
                |region| region_skips(&region.content, cfg, path, base_dir, rewrite),
            );
            (fixed, skipped)
        }
    }
}

/// Apply `rewrite` to each file in place.
///
/// # Errors
///
/// Returns an error if any file cannot be read or any rewritten contents cannot be written.
pub fn rewrite_files(files: &[LintFile], rewrite: Rewrite) -> Result<FixStats, String> {
    let mut stats = FixStats::default();
    for (path, base_dir, cfg, kind) in files {
        let outcome = rewrite_in_place(path, cfg, base_dir, *kind, rewrite)?;
        if outcome.changed {
            stats.changed_files += 1;
        }
        for problem in outcome.skipped {
            stats.skipped.push((path.clone(), problem));
        }
    }
    Ok(stats)
}

/// One file's `--diff` result: the unified diff (`None` when nothing would change) plus any
/// parse-skips: a plain YAML file contributes at most one (its whole-file parse error), a
/// Markdown file one per region that does not parse. A `ryl format` preview also carries
/// the [`crate::format::problems`] explaining the diff.
#[derive(Debug, Default)]
pub struct DiffOutcome {
    pub diff: Option<String>,
    pub skipped: Vec<crate::lint::LintProblem>,
    pub problems: Vec<crate::lint::LintProblem>,
}

/// Aggregated `--diff` results across all linted files.
#[derive(Debug, Default)]
pub struct DiffStats {
    /// Unified diffs for files that would change, in input order.
    pub diffs: Vec<String>,
    /// Files left unchanged because they (or, for Markdown, a region) do not parse.
    pub skipped: Vec<(PathBuf, crate::lint::LintProblem)>,
    pub problems: Vec<(PathBuf, Vec<crate::lint::LintProblem>)>,
}

impl DiffStats {
    /// Fold one file's outcome into the aggregate, tagging each parse-skip with the file path.
    /// Shared by the file-walk and stdin paths.
    pub fn record(&mut self, path: &Path, outcome: DiffOutcome) {
        if let Some(diff) = outcome.diff {
            self.diffs.push(diff);
        }
        for problem in outcome.skipped {
            self.skipped.push((path.to_path_buf(), problem));
        }
        if !outcome.problems.is_empty() {
            self.problems.push((path.to_path_buf(), outcome.problems));
        }
    }
}

/// `path` `lexical_abspath`-normalized, relativized to CWD and control-sanitized.
fn cwd_relative_label(path: &Path) -> String {
    let abspath = crate::cli_support::lexical_abspath(path);
    let cwd = std::env::current_dir().unwrap_or_default();
    let display = abspath.strip_prefix(&cwd).unwrap_or(&abspath);
    crate::cli_support::sanitize_control(&display.display().to_string()).into_owned()
}

/// A unified diff, or `None` when identical, like `ruff check --diff`: 3 context lines
/// (pinned against a `similar` default change) and a plain `--- path`/`+++ path` header,
/// `lexical_abspath`-normalized, CWD-relative (so `git apply -p0` works) and sanitized
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
    let mut outcome = diff_and_skips(content, cfg, path, base_dir, kind, rewrite);
    if rewrite == Rewrite::Format && outcome.diff.is_some() {
        outcome.problems = crate::format::problems(content, cfg, path, kind);
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
) -> DiffOutcome {
    if path_unrepresentable_in_diff(path) {
        return DiffOutcome {
            diff: None,
            skipped: vec![diff_skip(
                "filename has non-UTF-8 bytes or control characters; no applicable \
                 diff path",
            )],
            ..DiffOutcome::default()
        };
    }
    match kind {
        SourceKind::Yaml => {
            if let Some(problem) = crate::lint::parse_error(content) {
                return DiffOutcome {
                    diff: None,
                    skipped: vec![problem],
                    ..DiffOutcome::default()
                };
            }
            let fixed = rewrite.apply(content, cfg, path, base_dir, &[]);
            if content != fixed && ends_in_bare_cr(content, &fixed) {
                return DiffOutcome {
                    diff: None,
                    skipped: vec![bare_cr_diff_skip()],
                    ..DiffOutcome::default()
                };
            }
            DiffOutcome {
                diff: render_unified_diff(content, &fixed, path),
                skipped: unfixed_notices(&fixed, cfg, path, base_dir, rewrite),
                ..DiffOutcome::default()
            }
        }
        SourceKind::Markdown => {
            // A bare-`\r` markdown host is skipped upstream (`fix_markdown_str` returns
            // `None`), so content reaching `render_unified_diff` never carries a bare `\r`.
            let fixed = fix_markdown_str(content, path, cfg, base_dir, rewrite);
            // Report skips against the *original* content: `--diff` never writes, so the file
            // stays `content` and a skip notice must point at the original line (the in-place
            // path uses `fixed` because it writes it).
            let skips = |markdown| {
                markdown_region_problems(markdown, cfg, |region| {
                    region_skips(&region.content, cfg, path, base_dir, rewrite)
                })
            };
            let mut skipped = skips(content);
            // `key-ordering` notices describe the fixed text, so they come from it.
            if let Some(fixed) = &fixed {
                skipped.retain(|problem| problem.rule.is_none());
                skipped.extend(skips(fixed).into_iter().filter(|p| p.rule.is_some()));
            }
            let diff =
                fixed.and_then(|fixed| render_unified_diff(content, &fixed, path));
            DiffOutcome {
                diff,
                skipped,
                ..DiffOutcome::default()
            }
        }
    }
}

/// Each mapping `key-ordering`'s fix left unsorted in fixed `content`, as a notice.
fn unfixed_notices(
    content: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
    rewrite: Rewrite,
) -> Vec<crate::lint::LintProblem> {
    if rewrite == Rewrite::Format
        || !rule_enabled(key_ordering::ID, cfg, path, base_dir)
        || crate::directives::disables_file(content)
    {
        return Vec::new();
    }
    let rule = key_ordering::Config::resolve(cfg, path);
    key_ordering::unfixed(content, &rule, &cfg.per_line_applies(path))
        .into_iter()
        .map(|violation| crate::lint::LintProblem {
            line: violation.line,
            column: violation.column,
            level: crate::lint::Severity::Error,
            message: violation.message,
            rule: Some(key_ordering::ID),
        })
        .collect()
}

/// A Markdown region's `--fix` skips: its parse error, else its unsorted mappings.
fn region_skips(
    region: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
    rewrite: Rewrite,
) -> Vec<crate::lint::LintProblem> {
    crate::lint::parse_error(region).map_or_else(
        || unfixed_notices(region, cfg, path, base_dir, rewrite),
        |problem| vec![problem],
    )
}

/// Whether either side ends in a bare `\r`, which `similar` renders as a hunk line no patch
/// tool accepts (a mid-line `\r` is fine), so `--diff` skips it (use `--fix`).
fn ends_in_bare_cr(original: &str, fixed: &str) -> bool {
    original.ends_with('\r') || fixed.ends_with('\r')
}

#[must_use]
fn bare_cr_diff_skip() -> crate::lint::LintProblem {
    diff_skip(
        "content ends in a bare carriage return, which has no applicable text diff; use --fix",
    )
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
    let mut stats = DiffStats::default();
    for (path, base_dir, cfg, kind) in files {
        if refuse_symlink(path, flag) {
            continue;
        }
        let decoded = decoder::read_file_lossless(path)?;
        if !decoded.is_plain_utf8() {
            stats.skipped.push((path.clone(), non_utf8_diff_skip()));
            continue;
        }
        let outcome =
            diff_outcome(decoded.content(), cfg, path, base_dir, *kind, rewrite);
        stats.record(path, outcome);
    }
    Ok(stats)
}

/// Apply `rewrite` to each embedded YAML region of `markdown` and splice the results back
/// in, or `None` if nothing changed. File-shape rules are excluded per [`suppressed_rules`].
/// Each line regains the prefix the parser stripped (spaces, a blockquote `> `, or a tab), and
/// a region is rewritten only when re-applying that prefix reproduces the original raw bytes
/// exactly (the reconstruct-and-verify guard), so a ragged region is left untouched. Regions
/// are spliced back-to-front so earlier edits do not shift later offsets.
#[must_use]
pub fn fix_markdown_str(
    markdown: &str,
    path: &Path,
    cfg: &YamlLintConfig,
    base_dir: &Path,
    rewrite: Rewrite,
) -> Option<String> {
    if crate::markdown_embed::markdown_has_unsupported_cr(markdown) {
        return None;
    }
    let sources = MarkdownSources {
        front_matter: cfg.markdown_front_matter(),
        fenced_blocks: cfg.markdown_fenced_blocks(),
    };
    let mut regions = extract_regions(markdown, sources);
    regions.sort_by_key(|region| std::cmp::Reverse(region.raw_span.start));

    let mut out = markdown.to_string();
    let mut changed = false;
    for region in &regions {
        if region.content.trim().is_empty() {
            continue;
        }
        let fixed =
            rewrite.apply(&region.content, cfg, path, base_dir, suppressed_rules());
        if fixed == region.content {
            continue;
        }
        let Some((prefix, newline)) = region_prefix(markdown, region) else {
            continue;
        };
        out.replace_range(region.raw_span.clone(), &reindent(&fixed, &prefix, newline));
        changed = true;
    }
    changed.then_some(out)
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
    pub(crate) comments_indentation: Option<comments_indentation::Config>,
    pub(crate) commas: Option<commas::Config>,
    pub(crate) braces: Option<braces::Config>,
    pub(crate) brackets: Option<brackets::Config>,
    pub(crate) final_newline: Option<NewlinePolicy>,
    pub(crate) quoted_strings: Option<quoted_strings::Config>,
    pub(crate) trailing_spaces: bool,
    pub(crate) document_start: Option<document_start::Config>,
    pub(crate) document_end: Option<document_end::Config>,
    pub(crate) empty_lines: Option<empty_lines::Config>,
    pub(crate) truthy: Option<truthy::Config>,
    pub(crate) key_ordering: Option<key_ordering::Config>,
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
            comments_indentation: on(comments_indentation::ID)
                .then(|| comments_indentation::Config::resolve(cfg)),
            commas: on(commas::ID).then(|| commas::Config::resolve(cfg)),
            braces: on(braces::ID).then(|| braces::Config::resolve(cfg)),
            brackets: on(brackets::ID).then(|| brackets::Config::resolve(cfg)),
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
    // Never mutate a file that does not fully parse. `parse_error` is stricter than lint's
    // `syntax_diagnostic` (it does not tolerate undefined aliases), so any granit error leaves
    // the file byte-for-byte unchanged.
    if crate::directives::disables_file(input)
        || crate::lint::parse_error(input).is_some()
    {
        return input.to_string();
    }
    let ctx = FixContext {
        passes,
        directives: Directives::parse_with_per_line(input, &passes.per_line),
    };
    let mut content = input.to_string();
    for _ in 0..max_passes {
        let next = ctx.pass(&content, edited);
        if next == content {
            return content;
        }
        content = next;
    }
    let mut changed_rules = Vec::new();
    if ctx.pass(&content, &mut changed_rules) != content {
        changed_rules.dedup();
        // A failed write to stderr has nowhere better to go.
        let _ = writeln!(
            err,
            "\nerror: Failed to converge after {max_passes} iterations.\n\n\
             This indicates a bug in ryl. If you could open an issue at:\n\n    \
             https://github.com/owenlamont/ryl/issues/new?title=%5BInfinite%20loop%5D\n\n\
             ...quoting the contents of `{}`, the rule ids {}, along with the ryl config \
             and executed command, we'd be very appreciative!\n",
            cwd_relative_label(path),
            changed_rules.join(", "),
        );
    }
    content
}

/// Shared arguments for a sequence of rule fixes. `apply` is a method so it can be generic
/// over the fix closure (a capturing closure cannot), avoiding dynamic dispatch.
struct FixContext<'a> {
    passes: &'a Passes<'a>,
    /// Parsed once from the original input. `disables_any` is stable across fixes (no fixer
    /// adds or removes a directive comment), so the per-rule guard reads it without re-parsing.
    directives: Directives,
}

impl FixContext<'_> {
    /// One pass of every enabled fixer, pushing onto `changed_rules` each rule whose fix
    /// changed the text.
    fn pass(&self, input: &str, changed_rules: &mut Vec<&'static str>) -> String {
        let passes = self.passes;
        let mut content = input.to_string();
        macro_rules! fix {
            ($rule:ident) => {
                fix!($rule, passes.$rule.as_ref(), $rule::fix)
            };
            ($rule:ident, $cfg:expr, $fix:expr) => {
                content = self.apply(content, changed_rules, $rule::ID, $cfg, $fix)
            };
        }
        fix!(new_lines, passes.new_lines.as_ref(), |buffer, cfg| {
            new_lines::fix(buffer, *cfg, new_lines::platform_newline())
        });
        fix!(comments);
        fix!(comments_indentation);
        fix!(commas);
        fix!(braces);
        fix!(brackets);
        fix!(
            new_line_at_end_of_file,
            passes.final_newline.as_ref(),
            |buffer, policy| new_line_at_end_of_file::fix(
                buffer,
                policy.newline(buffer)
            )
        );
        fix!(quoted_strings);
        fix!(
            trailing_spaces,
            passes.trailing_spaces.then_some(&()),
            |buffer, ()| trailing_spaces::fix(buffer)
        );
        fix!(document_start);
        fix!(document_end);
        fix!(empty_lines);
        fix!(truthy);
        fix!(key_ordering, passes.key_ordering.as_ref(), |buffer, cfg| {
            key_ordering::fix(buffer, cfg, &passes.per_line)
        });
        content
    }

    fn apply<C>(
        &self,
        content: String,
        changed_rules: &mut Vec<&'static str>,
        rule: &'static str,
        cfg: Option<&C>,
        fix: impl Fn(&str, &C) -> Option<String>,
    ) -> String {
        let Some(cfg) = cfg else {
            return content;
        };

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
        let mut current = content;
        let mut directives = Directives::default();
        if guarded {
            directives = Directives::parse_with_per_line(&current, per_line);
        }
        for _ in 0..RULE_FIX_MAX_ITERATIONS {
            let Some(next) = fix(&current, cfg) else {
                break;
            };
            let next = if guarded {
                directives.reconcile(rule, &current, &next)
            } else {
                next
            };
            if next == current {
                break;
            }
            current = next;
            changed_rules.push(rule);
            if guarded {
                directives = Directives::parse_with_per_line(&current, per_line);
            }
        }
        current
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
