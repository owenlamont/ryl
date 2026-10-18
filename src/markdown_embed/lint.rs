//! Lint the YAML regions extracted from a markdown document and translate each
//! diagnostic's position back to the host file.

use std::collections::HashSet;
use std::path::Path;

use super::{EmbeddedRegion, MarkdownSources, extract_regions};
use crate::config::YamlLintConfig;
use crate::fix::{Rewrite, fix_markdown_str, suppressed_rules};
use crate::lint::{LintProblem, add_indentation_help, lint_str_with_hint_options};

/// Lint every embedded YAML region in `markdown` and return diagnostics whose line/column
/// point into the original markdown document. Each region is linted as an independent
/// document; file-shape rules are suppressed per region kind (see [`suppressed_rules`]).
#[must_use]
pub fn lint_markdown_str(
    markdown: &str,
    path: &Path,
    cfg: &YamlLintConfig,
    base_dir: &Path,
) -> Vec<LintProblem> {
    lint_markdown_str_with_fix(markdown, path, cfg, base_dir, true)
}

#[must_use]
pub fn lint_markdown_stdin_str(
    markdown: &str,
    path: &Path,
    cfg: &YamlLintConfig,
    base_dir: &Path,
) -> Vec<LintProblem> {
    lint_markdown_str_with_fix(markdown, path, cfg, base_dir, false)
}

pub(crate) fn lint_markdown_str_with_fix(
    markdown: &str,
    path: &Path,
    cfg: &YamlLintConfig,
    base_dir: &Path,
    allow_fix: bool,
) -> Vec<LintProblem> {
    let suppressed = suppressed_rules();
    let mut repaired = None;
    markdown_region_problems(markdown, cfg, |region| {
        let mut problems = lint_str_with_hint_options(
            &region.content,
            path,
            cfg,
            base_dir,
            false,
            false,
        );
        if !(allow_fix && path.is_symlink())
            && problems.iter().any(|problem| {
                problem.message == "syntax error: invalid indentation (syntax)"
            })
        {
            let (fix, format) = repaired.get_or_insert_with(|| {
                (
                    if allow_fix {
                        repaired_regions(markdown, path, cfg, base_dir, Rewrite::Fix)
                    } else {
                        HashSet::new()
                    },
                    repaired_regions(markdown, path, cfg, base_dir, Rewrite::Format),
                )
            });
            for problem in &mut problems {
                add_indentation_help(
                    problem,
                    fix.contains(&region.raw_span.start),
                    format.contains(&region.raw_span.start),
                );
            }
        }
        problems
            .retain(|problem| !problem.rule.is_some_and(|id| suppressed.contains(&id)));
        problems
    })
}

fn repaired_regions(
    markdown: &str,
    path: &Path,
    cfg: &YamlLintConfig,
    base_dir: &Path,
    rewrite: Rewrite,
) -> HashSet<usize> {
    let Some(output) = fix_markdown_str(markdown, path, cfg, base_dir, rewrite) else {
        return HashSet::new();
    };
    let sources = MarkdownSources {
        front_matter: cfg.markdown_front_matter(),
        fenced_blocks: cfg.markdown_fenced_blocks(),
    };
    extract_regions(markdown, sources)
        .into_iter()
        .zip(extract_regions(&output, sources))
        .filter_map(|(before, after)| {
            (before.content != after.content).then_some(before.raw_span.start)
        })
        .collect()
}

#[must_use]
pub fn markdown_region_problems(
    markdown: &str,
    cfg: &YamlLintConfig,
    mut region_problems: impl FnMut(&EmbeddedRegion) -> Vec<LintProblem>,
) -> Vec<LintProblem> {
    if super::markdown_has_unsupported_cr(markdown) {
        return vec![super::unsupported_cr_skip()];
    }
    let sources = MarkdownSources {
        front_matter: cfg.markdown_front_matter(),
        fenced_blocks: cfg.markdown_fenced_blocks(),
    };
    let mut skips = Vec::new();
    for region in extract_regions(markdown, sources) {
        if region.content.trim().is_empty() {
            continue;
        }
        let problems = region_problems(&region);
        if problems.is_empty() {
            continue;
        }
        let stripped = stripped_indents(markdown, &region);
        for mut problem in problems {
            problem.column += stripped
                .get(problem.line - 1)
                .copied()
                .unwrap_or(region.col_offset);
            problem.line += region.line_offset;
            skips.push(problem);
        }
    }
    skips
}

/// Per content line, the chars `CommonMark` stripped from its start (`chars(raw) -
/// chars(content)`: indent, blockquote markers, CRLF normalisation alike), added back to
/// each diagnostic column so positions point into the host document. Per-line, so a ragged
/// block (a line dedented less than the fence) stays correct. Always 0 for front matter.
fn stripped_indents(markdown: &str, region: &EmbeddedRegion) -> Vec<usize> {
    markdown[region.raw_span.clone()]
        .split('\n')
        .zip(region.content.split('\n'))
        .map(|(raw, content)| {
            raw.trim_end_matches('\r')
                .chars()
                .count()
                .saturating_sub(content.trim_end_matches('\r').chars().count())
        })
        .collect()
}
