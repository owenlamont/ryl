//! Lint the YAML regions extracted from a markdown document and translate each
//! diagnostic's position back to the host file.

use std::path::Path;

use super::{EmbeddedRegion, MarkdownSources, extract_regions};
use crate::config::YamlLintConfig;
use crate::fix::suppressed_rules;
use crate::lint::{LintProblem, lint_str};

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
    if super::markdown_has_unsupported_cr(markdown) {
        return vec![super::unsupported_cr_skip()];
    }
    let sources = MarkdownSources {
        front_matter: cfg.markdown_front_matter(),
        fenced_blocks: cfg.markdown_fenced_blocks(),
    };

    let suppressed = suppressed_rules();
    let mut problems = Vec::new();
    for region in extract_regions(markdown, sources) {
        if region.content.trim().is_empty() {
            continue;
        }
        let mut region_problems = lint_str(&region.content, path, cfg, base_dir);
        region_problems
            .retain(|problem| !problem.rule.is_some_and(|id| suppressed.contains(&id)));
        if region_problems.is_empty() {
            continue;
        }
        let stripped = stripped_indents(markdown, &region);
        for mut problem in region_problems {
            problem.column += stripped
                .get(problem.line - 1)
                .copied()
                .unwrap_or(region.col_offset);
            problem.line += region.line_offset;
            problems.push(problem);
        }
    }
    problems
}

/// For each embedded region that does not parse, the parse error mapped to host
/// coordinates, so `--fix` can report which regions its strict gate refused to rewrite.
/// Unlike a true syntax error (which [`lint_markdown_str`] already surfaces), an undefined
/// alias is otherwise silent.
#[must_use]
pub fn markdown_parse_skips(markdown: &str, cfg: &YamlLintConfig) -> Vec<LintProblem> {
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
        let Some(mut problem) = crate::lint::parse_error(&region.content) else {
            continue;
        };
        let stripped = stripped_indents(markdown, &region);
        problem.column += stripped
            .get(problem.line - 1)
            .copied()
            .unwrap_or(region.col_offset);
        problem.line += region.line_offset;
        skips.push(problem);
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
