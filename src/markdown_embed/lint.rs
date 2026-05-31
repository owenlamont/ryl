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

/// Per content line, the number of characters the `CommonMark` parser stripped from
/// its start — `chars(raw line) - chars(content line)` — which covers leading
/// space/tab indentation, blockquote markers, and CRLF normalisation alike. Added
/// back to each diagnostic column so positions point into the host document, and
/// correct for ragged blocks (a line dedented less than the fence yields a smaller
/// value). Front matter content equals its raw span, so every entry is 0.
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
