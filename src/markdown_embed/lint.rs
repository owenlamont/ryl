//! Lint the YAML regions extracted from a markdown document and translate each
//! diagnostic's position back to the host file.

use std::path::Path;

use super::{EmbeddedRegion, MarkdownSources, extract_regions};
use crate::config::YamlLintConfig;
use crate::fix::suppressed_rules;
use crate::lint::{LintProblem, lint_str};

/// Lint every embedded YAML region in `markdown` and return diagnostics whose
/// line/column point into the original markdown document.
///
/// Each region is linted as an independent YAML document. File-shape rules that
/// only make sense for a standalone file are suppressed per region kind (see
/// [`suppressed_rules`]).
#[must_use]
pub fn lint_markdown_str(
    markdown: &str,
    path: &Path,
    cfg: &YamlLintConfig,
    base_dir: &Path,
) -> Vec<LintProblem> {
    let sources = MarkdownSources {
        front_matter: cfg.markdown_front_matter(),
        fenced_blocks: cfg.markdown_fenced_blocks(),
    };

    let mut problems = Vec::new();
    for region in extract_regions(markdown, sources) {
        if region.content.trim().is_empty() {
            continue;
        }
        let suppressed = suppressed_rules(region.kind);
        let stripped = stripped_indents(markdown, &region);
        for mut problem in lint_str(&region.content, path, cfg, base_dir) {
            if problem.rule.is_some_and(|id| suppressed.contains(&id)) {
                continue;
            }
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

/// Per content line, the indent pulldown actually stripped (`min(leading spaces on
/// the raw line, col_offset)`). A uniformly-indented block yields `col_offset` for
/// every line; a line indented less than the fence yields its own smaller value, so
/// its column is not over-shifted. Empty for regions with no indent (front matter),
/// where the `col_offset` fallback (0) applies.
fn stripped_indents(markdown: &str, region: &EmbeddedRegion) -> Vec<usize> {
    if region.col_offset == 0 {
        return Vec::new();
    }
    markdown[region.raw_span.clone()]
        .split('\n')
        .map(|line| {
            line.bytes()
                .take_while(|byte| *byte == b' ')
                .count()
                .min(region.col_offset)
        })
        .collect()
}
