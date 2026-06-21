use std::path::{Path, PathBuf};

use similar::TextDiff;

use crate::config::{SourceKind, YamlLintConfig};
use crate::decoder;
use crate::directives::{Directives, PerLineRuleApply};
use crate::markdown_embed::{MarkdownSources, extract_regions};
use crate::rules::support::line_syntax::{buffer_newline, first_line_break};
use crate::rules::{
    braces, brackets, commas, comments, comments_indentation, document_end,
    document_start, empty_lines, new_line_at_end_of_file, new_lines, quoted_strings,
    trailing_spaces,
};

const RULE_FIX_MAX_ITERATIONS: usize = 8;

/// File-shape rules suppressed inside embedded markdown regions: a region is not a standalone
/// file, so document-start/end and file-newline checks do not apply and `--fix` must never
/// inject `---`/`...` or a trailing newline. Shared by the check and fix paths so they cannot
/// drift.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixSafety {
    Safe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RuleFix {
    rule: &'static str,
    safety: FixSafety,
}

const NEW_LINES_FIX: RuleFix = RuleFix {
    rule: new_lines::ID,
    safety: FixSafety::Safe,
};
const COMMENTS_FIX: RuleFix = RuleFix {
    rule: comments::ID,
    safety: FixSafety::Safe,
};
const COMMENTS_INDENTATION_FIX: RuleFix = RuleFix {
    rule: comments_indentation::ID,
    safety: FixSafety::Safe,
};
const COMMAS_FIX: RuleFix = RuleFix {
    rule: commas::ID,
    safety: FixSafety::Safe,
};
const BRACES_FIX: RuleFix = RuleFix {
    rule: braces::ID,
    safety: FixSafety::Safe,
};
const BRACKETS_FIX: RuleFix = RuleFix {
    rule: brackets::ID,
    safety: FixSafety::Safe,
};
const FINAL_NEWLINE_FIX: RuleFix = RuleFix {
    rule: new_line_at_end_of_file::ID,
    safety: FixSafety::Safe,
};
const QUOTED_STRINGS_FIX: RuleFix = RuleFix {
    rule: quoted_strings::ID,
    safety: FixSafety::Safe,
};
const TRAILING_SPACES_FIX: RuleFix = RuleFix {
    rule: trailing_spaces::ID,
    safety: FixSafety::Safe,
};
const DOCUMENT_START_FIX: RuleFix = RuleFix {
    rule: document_start::ID,
    safety: FixSafety::Safe,
};
const DOCUMENT_END_FIX: RuleFix = RuleFix {
    rule: document_end::ID,
    safety: FixSafety::Safe,
};
const EMPTY_LINES_FIX: RuleFix = RuleFix {
    rule: empty_lines::ID,
    safety: FixSafety::Safe,
};

/// Every rule with a safe `--fix`, in application order; extend together with the `ctx.apply`
/// sequence in [`apply_safe_fixes_filtered`] when adding a safe fixer. The LSP drives per-rule
/// "Fix all `<rule>`" actions off this list.
pub const SAFE_FIX_RULE_IDS: [&str; 12] = [
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

/// `--fix` rewrites in place and `std::fs::write` follows symlinks, so a symlinked input
/// would let an untrusted tree redirect the write outside it (e.g. `innocent.yaml ->
/// ~/.bashrc`). Skip it with a warning, consistent with the walker's `follow_links(false)`;
/// read-only linting through symlinks is unaffected. `--diff` skips too (`flag` distinguishes
/// the message), since it previews `--fix`.
///
/// Best-effort, not a hard sandbox: it checks only the final component (parents are resolved)
/// and is not atomic with the write, so a symlinked parent or a TOCTOU swap is not covered. A
/// complete defense needs `openat`/`O_NOFOLLOW`, which is not portable here.
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

/// Apply every safe fix to `path` in place.
///
/// # Errors
///
/// Returns an error if the file cannot be read or the fixed contents cannot be written.
pub fn apply_safe_fixes_in_place(
    path: &Path,
    cfg: &YamlLintConfig,
    base_dir: &Path,
) -> Result<FixOutcome, String> {
    if refuse_symlink(path, "--fix") {
        return Ok(FixOutcome::default());
    }
    let decoded = decoder::read_file_lossless(path)?;
    if let Some(problem) = crate::lint::parse_error(decoded.content()) {
        return Ok(FixOutcome {
            changed: false,
            skipped: vec![problem],
        });
    }
    let fixed = apply_safe_fixes(decoded.content(), cfg, path, base_dir);
    if fixed == decoded.content() {
        return Ok(FixOutcome::default());
    }

    decoded.write(path, &fixed)?;
    Ok(FixOutcome {
        changed: true,
        skipped: Vec::new(),
    })
}

/// Apply every safe fix to each file in place.
///
/// # Errors
///
/// Returns an error if any file cannot be read or any fixed contents cannot be written.
pub fn apply_safe_fixes_to_files(
    files: &[(PathBuf, PathBuf, YamlLintConfig, SourceKind)],
) -> Result<FixStats, String> {
    let mut stats = FixStats::default();
    for (path, base_dir, cfg, kind) in files {
        let outcome = match kind {
            SourceKind::Markdown => {
                apply_markdown_safe_fixes_in_place(path, cfg, base_dir)?
            }
            SourceKind::Yaml => apply_safe_fixes_in_place(path, cfg, base_dir)?,
        };
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
/// Markdown file one per region that does not parse.
#[derive(Debug, Default)]
pub struct DiffOutcome {
    pub diff: Option<String>,
    pub skipped: Vec<crate::lint::LintProblem>,
}

/// Aggregated `--diff` results across all linted files.
#[derive(Debug, Default)]
pub struct DiffStats {
    /// Unified diffs for files that would change, in input order.
    pub diffs: Vec<String>,
    /// Files left unchanged because they (or, for Markdown, a region) do not parse.
    pub skipped: Vec<(PathBuf, crate::lint::LintProblem)>,
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
    }
}

/// Render a unified diff from `original` and `fixed`, or `None` when identical. Follows
/// `ruff check --diff`: 3 lines of context (pinned, since it is also `similar`'s default a
/// crate upgrade could silently change) and a plain `--- path`/`+++ path` header (no git
/// `a/`/`b/`). The header path is `lexical_abspath`-normalized and relativized to CWD (like
/// ruff) so it applies with `git apply -p0` rather than failing on a `.`/`..`/absolute header,
/// and sanitized so a crafted filename can't inject escapes or forge a hunk header. The diff
/// *body* is emitted verbatim so a consumer can re-apply it unchanged.
fn render_unified_diff(original: &str, fixed: &str, path: &Path) -> Option<String> {
    if original == fixed {
        return None;
    }
    let abspath = crate::cli_support::lexical_abspath(path);
    let cwd = std::env::current_dir().unwrap_or_default();
    let display = abspath.strip_prefix(&cwd).unwrap_or(&abspath);
    let label = crate::cli_support::sanitize_control(&display.display().to_string())
        .into_owned();
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

/// The `--diff` outcome for in-memory `content`, shared by the file and stdin paths. Mirrors
/// the in-place fixers' gating: an unparsable plain YAML file yields no diff and one skip; a
/// Markdown file diffs at the host level via [`fix_markdown_str`], reporting each region that
/// does not parse. A path unrepresentable in a diff header is skipped here.
#[must_use]
pub fn diff_outcome(
    content: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
    kind: SourceKind,
) -> DiffOutcome {
    if path_unrepresentable_in_diff(path) {
        return DiffOutcome {
            diff: None,
            skipped: vec![diff_skip(
                "filename has non-UTF-8 bytes or control characters; no applicable \
                 diff path",
            )],
        };
    }
    match kind {
        SourceKind::Yaml => {
            if let Some(problem) = crate::lint::parse_error(content) {
                return DiffOutcome {
                    diff: None,
                    skipped: vec![problem],
                };
            }
            let fixed = apply_safe_fixes(content, cfg, path, base_dir);
            if content != fixed && ends_in_bare_cr(content, &fixed) {
                return DiffOutcome {
                    diff: None,
                    skipped: vec![bare_cr_diff_skip()],
                };
            }
            DiffOutcome {
                diff: render_unified_diff(content, &fixed, path),
                skipped: Vec::new(),
            }
        }
        SourceKind::Markdown => {
            // A bare-`\r` markdown host is skipped upstream (`fix_markdown_str` returns
            // `None`), so content reaching `render_unified_diff` never carries a bare `\r`.
            let fixed = fix_markdown_str(content, path, cfg, base_dir);
            // Report skips against the *original* content: `--diff` never writes, so the file
            // stays `content` and a skip notice must point at the original line (the in-place
            // path uses `fixed` because it writes it).
            let skipped = crate::markdown_embed::markdown_parse_skips(content, cfg);
            let diff =
                fixed.and_then(|fixed| render_unified_diff(content, &fixed, path));
            DiffOutcome { diff, skipped }
        }
    }
}

/// Whether either side ends in a bare `\r`. `similar`'s `ends_with_newline` counts a trailing
/// `\r` as a terminator and emits a hunk line no patch tool accepts (a mid-line `\r` is fine),
/// so `--diff` skips that case (use `--fix`).
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

/// The `--diff` skip for a non-UTF-8 (or BOM) input: a textual diff of the decoded content
/// cannot apply back to the BOM'd/transcoded bytes the way `--fix`'s re-encode does. Shared by
/// the file and stdin paths.
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

/// Unified diffs for each file's safe fixes, reading from disk and never writing. A symlinked
/// input is skipped with a warning (parity with `--fix`); other un-diffable inputs (non-UTF-8/
/// BOM content, unparsable, or an unrepresentable name) are skipped via [`diff_outcome`].
///
/// # Errors
///
/// Returns an error if any file cannot be read.
pub fn diff_safe_fixes_for_files(
    files: &[(PathBuf, PathBuf, YamlLintConfig, SourceKind)],
) -> Result<DiffStats, String> {
    let mut stats = DiffStats::default();
    for (path, base_dir, cfg, kind) in files {
        if refuse_symlink(path, "--diff") {
            continue;
        }
        let decoded = decoder::read_file_lossless(path)?;
        if !decoded.is_plain_utf8() {
            stats.skipped.push((path.clone(), non_utf8_diff_skip()));
            continue;
        }
        let outcome = diff_outcome(decoded.content(), cfg, path, base_dir, *kind);
        stats.record(path, outcome);
    }
    Ok(stats)
}

/// Apply safe fixes to every embedded YAML region of a markdown file in place.
///
/// # Errors
///
/// Returns an error if the file cannot be read or the rewritten contents cannot be written.
pub fn apply_markdown_safe_fixes_in_place(
    path: &Path,
    cfg: &YamlLintConfig,
    base_dir: &Path,
) -> Result<FixOutcome, String> {
    if refuse_symlink(path, "--fix") {
        return Ok(FixOutcome::default());
    }
    let decoded = decoder::read_file_lossless(path)?;
    let fixed = fix_markdown_str(decoded.content(), path, cfg, base_dir);
    // Collect parse errors for regions the per-region gate in `fix_markdown_str` skipped, so
    // the CLI reports them. Read from the *fixed* bytes (what gets written) so the reported
    // line stays correct after an earlier region's fix shifts the line count.
    let skipped = crate::markdown_embed::markdown_parse_skips(
        fixed.as_deref().unwrap_or_else(|| decoded.content()),
        cfg,
    );
    let changed = match fixed {
        Some(fixed) => {
            decoded.write(path, &fixed)?;
            true
        }
        None => false,
    };
    Ok(FixOutcome { changed, skipped })
}

/// Apply safe fixes to each embedded YAML region of `markdown` and splice the results back
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
        let fixed = apply_safe_fixes_filtered(
            &region.content,
            cfg,
            path,
            base_dir,
            suppressed_rules(),
        );
        if fixed == region.content {
            continue;
        }
        let raw = &markdown[region.raw_span.clone()];
        let newline = buffer_newline(raw);
        // `raw` starts at the first content line and `col_offset` (its stripped char count)
        // never spans a newline, so the first `col_offset` chars of `raw` are exactly the
        // prefix the parser stripped. The guard below re-checks it against every line, so a
        // ragged prefix still fails and is skipped.
        let prefix: String = raw.chars().take(region.col_offset).collect();
        if reindent(&region.content, &prefix, newline) != raw {
            continue;
        }
        out.replace_range(region.raw_span.clone(), &reindent(&fixed, &prefix, newline));
        changed = true;
    }
    changed.then_some(out)
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
    // Never mutate a file that does not fully parse. `parse_error` is stricter than lint's
    // `syntax_diagnostic` (it does not tolerate undefined aliases), so any granit error leaves
    // the file byte-for-byte unchanged.
    if crate::directives::disables_file(input)
        || crate::lint::parse_error(input).is_some()
    {
        return input.to_string();
    }
    let per_line = cfg.per_line_applies(path, base_dir);
    let ctx = FixContext {
        cfg,
        path,
        base_dir,
        skip,
        directives: Directives::parse_with_per_line(input, &per_line),
        per_line,
    };
    let mut content = input.to_string();
    content = ctx.apply(content, NEW_LINES_FIX, |buffer| {
        new_lines::fix(
            buffer,
            new_lines::Config::resolve(cfg),
            new_lines::platform_newline(),
        )
    });
    content = ctx.apply(content, COMMENTS_FIX, |buffer| {
        comments::fix(buffer, &comments::Config::resolve(cfg))
    });
    content = ctx.apply(content, COMMENTS_INDENTATION_FIX, |buffer| {
        comments_indentation::fix(buffer, &comments_indentation::Config::resolve(cfg))
    });
    content = ctx.apply(content, COMMAS_FIX, |buffer| {
        commas::fix(buffer, &commas::Config::resolve(cfg))
    });
    content = ctx.apply(content, BRACES_FIX, |buffer| {
        braces::fix(buffer, &braces::Config::resolve(cfg))
    });
    content = ctx.apply(content, BRACKETS_FIX, |buffer| {
        brackets::fix(buffer, &brackets::Config::resolve(cfg))
    });
    content = ctx.apply(content, FINAL_NEWLINE_FIX, |buffer| {
        let newline = target_newline(buffer, cfg, path, base_dir);
        new_line_at_end_of_file::fix(buffer, newline.as_str())
    });
    content = ctx.apply(content, QUOTED_STRINGS_FIX, |buffer| {
        quoted_strings::fix(buffer, &quoted_strings::Config::resolve(cfg))
    });
    content = ctx.apply(content, TRAILING_SPACES_FIX, trailing_spaces::fix);
    content = ctx.apply(content, DOCUMENT_START_FIX, |buffer| {
        document_start::fix(buffer, &document_start::Config::resolve(cfg))
    });
    content = ctx.apply(content, DOCUMENT_END_FIX, |buffer| {
        document_end::fix(buffer, &document_end::Config::resolve(cfg))
    });
    content = ctx.apply(content, EMPTY_LINES_FIX, |buffer| {
        empty_lines::fix(buffer, &empty_lines::Config::resolve(cfg))
    });

    content
}

/// Shared arguments for a sequence of rule fixes. `apply` is a method so it can be generic
/// over the fix closure (a capturing closure cannot), avoiding dynamic dispatch.
struct FixContext<'a> {
    cfg: &'a YamlLintConfig,
    path: &'a Path,
    base_dir: &'a Path,
    skip: &'a [&'a str],
    /// Parsed once from the original input. `disables_any` is stable across fixes (no fixer
    /// adds or removes a directive comment), so the per-rule guard reads it without re-parsing.
    directives: Directives,
    /// Config `per-line-ignores` for this file; re-applied on each guarded re-parse since a
    /// structural fixer can shift which line a regex matches.
    per_line: Vec<PerLineRuleApply<'a>>,
}

impl FixContext<'_> {
    fn apply(
        &self,
        content: String,
        rule: RuleFix,
        fix: impl Fn(&str) -> Option<String>,
    ) -> String {
        if self.skip.contains(&rule.rule)
            || !rule_enabled(rule, self.cfg, self.path, self.base_dir)
        {
            return content;
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
        let guarded = self.directives.disables_any(rule.rule)
            || self
                .per_line
                .iter()
                .any(|entry| entry.rules.is_none_or(|ids| ids.contains(&rule.rule)));
        let mut current = content;
        let mut directives = Directives::default();
        if guarded {
            directives = Directives::parse_with_per_line(&current, &self.per_line);
        }
        for _ in 0..RULE_FIX_MAX_ITERATIONS {
            let Some(next) = fix(&current) else { break };
            let next = if guarded {
                directives.reconcile(rule.rule, &current, &next)
            } else {
                next
            };
            if next == current {
                break;
            }
            current = next;
            if guarded {
                directives = Directives::parse_with_per_line(&current, &self.per_line);
            }
        }
        current
    }
}

fn rule_enabled(
    rule: RuleFix,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
) -> bool {
    match rule.safety {
        FixSafety::Safe => {
            cfg.rule_level(rule.rule).is_some()
                && !cfg.is_rule_ignored(rule.rule, path, base_dir)
                && cfg.fix().allows_rule(rule.rule)
        }
    }
}

fn target_newline(
    content: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
) -> String {
    if cfg.rule_level(new_lines::ID).is_some()
        && !cfg.is_rule_ignored(new_lines::ID, path, base_dir)
    {
        return new_lines::expected_newline(
            new_lines::Config::resolve(cfg),
            new_lines::platform_newline(),
        )
        .into_owned();
    }

    // Reuse the first line's ending so a `\r`-delimited file's appended final newline
    // stays `\r` rather than falling back to LF.
    first_line_break(content)
        .map_or("\n", |(_, nl)| nl)
        .to_string()
}
