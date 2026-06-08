use std::path::{Path, PathBuf};

use similar::TextDiff;

use crate::config::{SourceKind, YamlLintConfig};
use crate::decoder;
use crate::directives::Directives;
use crate::markdown_embed::{MarkdownSources, extract_regions};
use crate::rules::support::line_syntax::buffer_newline;
use crate::rules::{
    braces, brackets, commas, comments, comments_indentation, document_end,
    document_start, empty_lines, new_line_at_end_of_file, new_lines, quoted_strings,
    trailing_spaces,
};

const RULE_FIX_MAX_ITERATIONS: usize = 8;

/// File-shape rules suppressed inside embedded markdown regions: a region is not a
/// standalone file, so "missing document start/end" and the file-newline checks do
/// not apply, and `--fix` must never inject `---`/`...` or a trailing newline into a
/// fragment. Shared by the check path ([`lint_markdown_str`]) and the fix path so
/// the two cannot drift.
const SUPPRESSED: [&str; 4] = [
    document_start::ID,
    document_end::ID,
    new_line_at_end_of_file::ID,
    new_lines::ID,
];

/// Rules suppressed inside any embedded region.
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

#[derive(Debug, Clone, Default)]
pub struct FixStats {
    pub changed_files: usize,
    /// Files left untouched because they do not parse, with the parse error so the
    /// caller can tell the user why `--fix` refused them.
    pub skipped: Vec<(PathBuf, crate::lint::LintProblem)>,
}

/// The result of fixing a single file in place. A file may both have changed and
/// carry skips: a Markdown file can fix some embedded regions while skipping others
/// that do not parse. For a plain YAML file `skipped` holds at most one entry (the
/// whole file's parse error).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FixOutcome {
    pub changed: bool,
    pub skipped: Vec<crate::lint::LintProblem>,
}

/// `--fix` rewrites a path in place, and `std::fs::write` follows symlinks, so a
/// symlinked input would let an untrusted tree redirect the write to a file outside
/// it (e.g. `innocent.yaml -> ~/.bashrc`). Skip a symlinked input with a warning —
/// consistent with the directory walker's `follow_links(false)`, which is the path
/// by which untrusted trees are scanned. Linting (read-only) through symlinks is
/// unaffected. `--diff` skips symlinks too (`flag` distinguishes the message): it is
/// a preview of `--fix`, so it must report the same skip rather than diffing a file
/// `--fix` would never touch.
///
/// This checks only the final path component (`symlink_metadata` resolves parent
/// components), and the check is not atomic with the later write. It is therefore
/// best-effort, not a hard sandbox: an explicitly-named path through a symlinked
/// parent directory, or an attacker who swaps the file for a symlink between this
/// check and the write (TOCTOU), is not covered. A complete defense would need
/// `openat`/`O_NOFOLLOW`, which is not portable here.
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

/// Apply all currently supported safe fixes to `path` in place.
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

/// Apply all currently supported safe fixes to each discovered file in place.
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

/// A single file's `--diff` result: the unified diff (`None` when the safe fixes
/// would change nothing) plus any parse-skips — a plain YAML file contributes at most
/// one (its whole-file parse error); a Markdown file one per region that does not
/// parse — reported like [`FixStats::skipped`].
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
    /// Fold one file's outcome into the aggregate: record its diff (if any) and tag
    /// each parse-skip with the file path. Shared by the file-walk and stdin paths so
    /// the diff-vs-skip routing lives in one place.
    pub fn record(&mut self, path: &Path, outcome: DiffOutcome) {
        if let Some(diff) = outcome.diff {
            self.diffs.push(diff);
        }
        for problem in outcome.skipped {
            self.skipped.push((path.to_path_buf(), problem));
        }
    }
}

/// Render a unified diff from `original` and `fixed`, or `None` when they are
/// identical. The format follows `ruff check --diff`: 3 lines of context (also
/// `similar`'s default, pinned here so a crate upgrade can't silently change it) and a
/// plain `--- path` / `+++ path` header (no git `a/`/`b/` prefixes). The header path is
/// `lexical_abspath`-normalized (so `./f`/`sub/../f` become a clean `f`) and relativized
/// to CWD — like ruff, an absolute path under CWD becomes relative, an out-of-tree one
/// stays absolute — so the patch applies with `git apply -p0` / hk rather than failing
/// on a `.`/`..`/absolute header. The path is sanitized so a crafted filename can't
/// inject terminal escapes or forge a hunk header; the diff *body* is emitted verbatim
/// so a consumer can re-apply the content unchanged (like `git diff`, the raw bytes).
fn render_unified_diff(original: &str, fixed: &str, path: &Path) -> Option<String> {
    if original == fixed {
        return None;
    }
    let abspath = crate::cli_support::lexical_abspath(path);
    let cwd = std::env::current_dir().unwrap_or_default();
    let display = abspath.strip_prefix(&cwd).unwrap_or(&abspath);
    let label = crate::cli_support::sanitize_control(&display.display().to_string())
        .into_owned();
    // git/patch headers use forward slashes; on Windows the path uses `\`, so normalize
    // it (Unix leaves `\` alone — there it is a valid filename character, not a
    // separator).
    #[cfg(windows)]
    let label = label.replace('\\', "/");
    Some(
        TextDiff::from_lines(original, fixed)
            .unified_diff()
            .context_radius(3)
            .header(&label, &label)
            .to_string(),
    )
}

/// Compute the `--diff` outcome for in-memory `content` (shared by the file and stdin
/// paths). Mirrors the in-place fixers' gating: an unparsable plain YAML file yields
/// no diff and one skip so the CLI can tell the user why; a Markdown file diffs at the
/// host level via [`fix_markdown_str`] and reports each region that does not parse. A
/// path (or `--stdin-filename` label) that can't be written in a diff header is skipped
/// here, so the file and stdin paths share the guard.
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
            DiffOutcome {
                diff: render_unified_diff(content, &fixed, path),
                skipped: Vec::new(),
            }
        }
        SourceKind::Markdown => {
            let fixed = fix_markdown_str(content, path, cfg, base_dir);
            // Skips are reported against the *original* content, not `fixed`: unlike
            // the in-place path (which writes `fixed`, so its skip line numbers must
            // match the post-fix file), `--diff` never writes, so the file on disk
            // stays `content` and a skip notice must point at the original line.
            let skipped = crate::markdown_embed::markdown_parse_skips(content, cfg);
            let diff =
                fixed.and_then(|fixed| render_unified_diff(content, &fixed, path));
            DiffOutcome { diff, skipped }
        }
    }
}

/// A `--diff` skip notice (`<path>:1:1 skipped by --diff: <message>`) for an input that
/// cannot produce an applicable diff.
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

/// The skip a non-UTF-8 (or BOM) input gets under `--diff`: a textual unified diff of the
/// decoded content cannot be applied back to the BOM'd/transcoded on-disk bytes, and a
/// text diff cannot round-trip the original encoding the way `--fix`'s re-encode does.
/// Shared by the file path and the stdin path (`main::run_stdin_diff`).
#[must_use]
pub fn non_utf8_diff_skip() -> crate::lint::LintProblem {
    diff_skip("non-UTF-8 or BOM content has no applicable text diff; use --fix")
}

/// Whether the path can't be faithfully written in a unified-diff header — it is not
/// valid UTF-8, or it contains a control character. Either way the header would name a
/// different path than the on-disk file (non-UTF-8 bytes become `�`; a raw control char
/// corrupts the `---`/`+++` line or is sanitized away), so no consumer could apply the
/// patch. `--diff` skips these, like the non-UTF-8-*content* case.
fn path_unrepresentable_in_diff(path: &Path) -> bool {
    let name = path.as_os_str();
    name.to_str().is_none() || name.to_string_lossy().contains(char::is_control)
}

/// Compute unified diffs for the safe fixes of each file, reading from disk. Never
/// writes; a symlinked input is skipped with a warning (parity with `--fix`). Other
/// un-diffable inputs (non-UTF-8/BOM content, an unparsable file, or a name that can't
/// appear in a header) are skipped with a notice via [`diff_outcome`].
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
/// Returns an error if the file cannot be read or the rewritten contents cannot be
/// written.
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
    // Regions that do not parse are skipped by the per-region gate in
    // `fix_markdown_str`; collect their parse errors (mapped to host coordinates) so
    // the CLI reports them, like a plain YAML file's whole-file skip. Read them from
    // the *fixed* bytes (what gets written) so the reported line stays correct even
    // when an earlier region's fix changed the line count; a skipped region is left
    // untouched, so it still appears there at its post-fix position.
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

/// Apply safe fixes to each embedded YAML region of `markdown` and splice the
/// results back in, returning the rewritten document (or `None` if nothing
/// changed).
///
/// File-shape rules are excluded per [`suppressed_rules`]. Each line regains the
/// prefix the parser stripped (leading spaces, a blockquote `> `, or a tab), and a
/// region is only rewritten when re-applying that prefix reproduces the original raw
/// bytes exactly (the reconstruct-and-verify guard); a region whose lines do not
/// share one prefix (ragged indentation) is left untouched (still reported in check
/// mode). Regions are spliced back-to-front so earlier edits do not shift later
/// offsets.
#[must_use]
pub fn fix_markdown_str(
    markdown: &str,
    path: &Path,
    cfg: &YamlLintConfig,
    base_dir: &Path,
) -> Option<String> {
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
        // The prefix the parser stripped from each content line — spaces for an
        // indented fence, `> ` for a blockquoted one, a tab, etc. `raw` starts at the
        // first content line and `col_offset` (its stripped char count) never spans a
        // newline, so the first `col_offset` chars of `raw` are exactly that prefix.
        // The guard below re-checks it reproduces every line, so a non-uniform
        // (ragged) prefix still fails and is skipped.
        let prefix: String = raw.chars().take(region.col_offset).collect();
        if reindent(&region.content, &prefix, newline) != raw {
            continue;
        }
        out.replace_range(region.raw_span.clone(), &reindent(&fixed, &prefix, newline));
        changed = true;
    }
    changed.then_some(out)
}

/// Re-encode dedented region content into its host: each non-empty line regains the
/// region's leading `prefix` and lines are joined with `newline`. Empty lines stay
/// empty (matching how the parser dedents blank lines), and the trailing newline is
/// preserved.
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

/// Apply safe fixes, skipping any rule whose id is in `skip`. Used by the markdown
/// write-back path to exclude the file-shape rules (see [`suppressed_rules`]).
#[must_use]
pub fn apply_safe_fixes_filtered(
    input: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
    skip: &[&str],
) -> String {
    // Never mutate a file that does not fully parse. `parse_error` is stricter than
    // lint's `syntax_diagnostic` — it does not tolerate undefined aliases — so a
    // file with any granit error (including an alias that masks a later syntax
    // error) is left byte-for-byte unchanged. The CLI surfaces the reason via
    // `apply_safe_fixes_in_place`'s `Skipped` outcome.
    if crate::directives::disables_file(input)
        || crate::lint::parse_error(input).is_some()
    {
        return input.to_string();
    }
    let ctx = FixContext {
        cfg,
        path,
        base_dir,
        skip,
        directives: Directives::parse(input),
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

/// Shared arguments for a sequence of rule fixes, bundled so each per-rule call
/// site stays short. `apply` is generic over the fix closure — a method can be,
/// where a capturing closure cannot — so there is no dynamic dispatch.
struct FixContext<'a> {
    cfg: &'a YamlLintConfig,
    path: &'a Path,
    base_dir: &'a Path,
    skip: &'a [&'a str],
    /// Parsed once from the original input. `disables_any` is stable across fixes
    /// (no fixer adds or removes a directive comment), so the per-rule guard can be
    /// read from here without re-parsing for every rule.
    directives: Directives,
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

        // Run the rule's fix to a fixed point. A single pass is not enough for
        // rules like quoted-strings where one fix exposes a follow-up diagnostic
        // (e.g. converting double quotes to single quotes leaves a now-redundant
        // pair that must be removed); without convergence here, the CLI's single
        // --fix invocation would leave the output non-idempotent. Well-behaved
        // fix functions signal completion by returning None, so the loop exits
        // after at most one extra no-op call per rule.
        //
        // When a directive disables this rule on some line, reconcile each pass so the
        // fixer's edits to those lines are reverted; directives are re-parsed after a
        // change because structural fixers shift line numbers (and the comments with
        // them). The guard is read from the once-parsed `self.directives`; only a
        // guarded rule pays for the per-pass re-parse.
        let guarded = self.directives.disables_any(rule.rule);
        let mut current = content;
        let mut directives = Directives::default();
        if guarded {
            directives = Directives::parse(&current);
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
                directives = Directives::parse(&current);
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

    first_newline(content).unwrap_or("\n").to_string()
}

fn first_newline(content: &str) -> Option<&'static str> {
    let bytes = content.as_bytes();
    let mut idx = 0usize;
    while idx < bytes.len() {
        match bytes[idx] {
            b'\r' if bytes.get(idx + 1) == Some(&b'\n') => return Some("\r\n"),
            b'\n' => return Some("\n"),
            _ => idx += 1,
        }
    }
    None
}
