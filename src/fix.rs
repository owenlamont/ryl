use std::path::{Path, PathBuf};

use crate::config::{SourceKind, YamlLintConfig};
use crate::decoder;
use crate::markdown_embed::{MarkdownSources, RegionKind, extract_regions};
use crate::rules::{
    braces, brackets, commas, comments, comments_indentation, document_end,
    document_start, empty_lines, new_line_at_end_of_file, new_lines, quoted_strings,
    trailing_spaces,
};

const RULE_FIX_MAX_ITERATIONS: usize = 8;

/// File-shape rules suppressed inside embedded markdown regions, per region kind:
/// a region is not a standalone file, so "missing document start/end" and the
/// file-newline checks do not apply, and `--fix` must never inject `---`/`...` or a
/// trailing newline into a fragment. Front matter and fenced blocks suppress the
/// same four rules today; the per-kind signature lets them diverge without a
/// refactor. The single source is shared by the check path ([`lint_markdown_str`])
/// and the fix path, so the two cannot drift.
const FRONT_MATTER_SUPPRESSED: [&str; 4] = [
    document_start::ID,
    document_end::ID,
    new_line_at_end_of_file::ID,
    new_lines::ID,
];
const FENCED_BLOCK_SUPPRESSED: [&str; 4] = [
    document_start::ID,
    document_end::ID,
    new_line_at_end_of_file::ID,
    new_lines::ID,
];

/// Rules suppressed inside an embedded region of the given kind.
#[must_use]
pub fn suppressed_rules(kind: RegionKind) -> &'static [&'static str] {
    match kind {
        RegionKind::FrontMatter => &FRONT_MATTER_SUPPRESSED,
        RegionKind::FencedBlock => &FENCED_BLOCK_SUPPRESSED,
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FixStats {
    pub changed_files: usize,
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
) -> Result<bool, String> {
    let decoded = decoder::read_file_lossless(path)?;
    let fixed = apply_safe_fixes(decoded.content(), cfg, path, base_dir);
    if fixed == decoded.content() {
        return Ok(false);
    }

    decoded.write(path, &fixed)?;
    Ok(true)
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
        let changed = match kind {
            SourceKind::Markdown => {
                apply_markdown_safe_fixes_in_place(path, cfg, base_dir)?
            }
            SourceKind::Yaml => apply_safe_fixes_in_place(path, cfg, base_dir)?,
        };
        if changed {
            stats.changed_files += 1;
        }
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
) -> Result<bool, String> {
    let decoded = decoder::read_file_lossless(path)?;
    match fix_markdown_str(decoded.content(), path, cfg, base_dir) {
        Some(fixed) => {
            decoded.write(path, &fixed)?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Apply safe fixes to each embedded YAML region of `markdown` and splice the
/// results back in, returning the rewritten document (or `None` if nothing
/// changed).
///
/// File-shape rules are excluded per [`suppressed_rules`]. A region is only
/// rewritten when its fixed YAML can be re-indented to reproduce the original raw
/// bytes exactly (the reconstruct-and-verify guard); ragged-indent, tab-indented,
/// or otherwise non-round-trippable regions are left untouched (still reported in
/// check mode). Regions are spliced back-to-front so earlier edits do not shift
/// later offsets.
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
            suppressed_rules(region.kind),
        );
        if fixed == region.content {
            continue;
        }
        let raw = &markdown[region.raw_span.clone()];
        let newline = detect_newline(raw);
        if reindent(&region.content, region.col_offset, newline) != raw {
            continue;
        }
        out.replace_range(
            region.raw_span.clone(),
            &reindent(&fixed, region.col_offset, newline),
        );
        changed = true;
    }
    changed.then_some(out)
}

/// The newline style of a raw region: CRLF if the first line ending is `\r\n`,
/// otherwise LF.
fn detect_newline(raw: &str) -> &'static str {
    match raw.find('\n') {
        Some(index) if raw.as_bytes().get(index.wrapping_sub(1)) == Some(&b'\r') => {
            "\r\n"
        }
        _ => "\n",
    }
}

/// Re-encode dedented region content into its host: each non-empty line regains
/// `col_offset` leading spaces and lines are joined with `newline`. Empty lines
/// stay empty (matching how the parser dedents blank lines), and the trailing
/// newline is preserved.
fn reindent(content: &str, col_offset: usize, newline: &str) -> String {
    let indent = " ".repeat(col_offset);
    let mut out = String::with_capacity(content.len());
    for piece in content.replace("\r\n", "\n").split_inclusive('\n') {
        let (line, terminated) = piece
            .strip_suffix('\n')
            .map_or((piece, false), |line| (line, true));
        if !line.is_empty() {
            out.push_str(&indent);
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
    let mut content = input.to_string();
    let run = |content: String, rule: RuleFix, fix: &dyn Fn(&str) -> Option<String>| {
        apply_rule_fix(content, rule, cfg, path, base_dir, skip, fix)
    };

    content = run(content, NEW_LINES_FIX, &|buffer: &str| {
        new_lines::fix(
            buffer,
            new_lines::Config::resolve(cfg),
            new_lines::platform_newline(),
        )
    });
    content = run(content, COMMENTS_FIX, &|buffer: &str| {
        comments::fix(buffer, &comments::Config::resolve(cfg))
    });
    content = run(content, COMMENTS_INDENTATION_FIX, &|buffer: &str| {
        comments_indentation::fix(buffer, &comments_indentation::Config::resolve(cfg))
    });
    content = run(content, COMMAS_FIX, &|buffer: &str| {
        commas::fix(buffer, &commas::Config::resolve(cfg))
    });
    content = run(content, BRACES_FIX, &|buffer: &str| {
        braces::fix(buffer, &braces::Config::resolve(cfg))
    });
    content = run(content, BRACKETS_FIX, &|buffer: &str| {
        brackets::fix(buffer, &brackets::Config::resolve(cfg))
    });
    content = run(content, FINAL_NEWLINE_FIX, &|buffer: &str| {
        let newline = target_newline(buffer, cfg, path, base_dir);
        new_line_at_end_of_file::fix(buffer, newline.as_str())
    });
    content = run(content, QUOTED_STRINGS_FIX, &|buffer: &str| {
        quoted_strings::fix(buffer, &quoted_strings::Config::resolve(cfg))
    });
    content = run(content, TRAILING_SPACES_FIX, &trailing_spaces::fix);
    content = run(content, DOCUMENT_START_FIX, &|buffer: &str| {
        document_start::fix(buffer, &document_start::Config::resolve(cfg))
    });
    content = run(content, DOCUMENT_END_FIX, &|buffer: &str| {
        document_end::fix(buffer, &document_end::Config::resolve(cfg))
    });
    content = run(content, EMPTY_LINES_FIX, &|buffer: &str| {
        empty_lines::fix(buffer, &empty_lines::Config::resolve(cfg))
    });

    content
}

fn apply_rule_fix(
    content: String,
    rule: RuleFix,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
    skip: &[&str],
    fix: &dyn Fn(&str) -> Option<String>,
) -> String {
    if skip.contains(&rule.rule) || !rule_enabled(rule, cfg, path, base_dir) {
        return content;
    }

    // Run the rule's fix to a fixed point. A single pass is not enough for
    // rules like quoted-strings where one fix exposes a follow-up diagnostic
    // (e.g. converting double quotes to single quotes leaves a now-redundant
    // pair that must be removed); without convergence here, the CLI's single
    // --fix invocation would leave the output non-idempotent. Well-behaved
    // fix functions signal completion by returning None, so the loop exits
    // after at most one extra no-op call per rule.
    let mut current = content;
    for _ in 0..RULE_FIX_MAX_ITERATIONS {
        let Some(next) = fix(&current) else { break };
        current = next;
    }
    current
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
