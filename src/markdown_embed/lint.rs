//! Lint the YAML regions extracted from a markdown document and translate each
//! diagnostic's position back to the host file.

use std::path::Path;

use super::{MarkdownSources, extract_regions};
use crate::config::YamlLintConfig;
use crate::lint::{LintProblem, lint_str};
use crate::rules::{document_end, document_start, new_line_at_end_of_file, new_lines};

/// File-shape rules suppressed inside embedded regions: a region is not a
/// standalone file, so "missing document start/end" and file-newline checks do
/// not apply (the analog of markdown linters skipping their first-line/EOF rules).
const SUPPRESSED: [&str; 4] = [
    new_line_at_end_of_file::ID,
    new_lines::ID,
    document_start::ID,
    document_end::ID,
];

/// Lint every embedded YAML region in `markdown` and return diagnostics whose
/// line/column point into the original markdown document.
///
/// Each region is linted as an independent YAML document. File-shape rules that
/// only make sense for a standalone file are suppressed (see [`is_suppressed`]).
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
        for mut problem in lint_str(&region.content, path, cfg, base_dir) {
            if is_suppressed(problem.rule) {
                continue;
            }
            problem.line += region.line_offset;
            problem.column += region.col_offset;
            problems.push(problem);
        }
    }
    problems
}

fn is_suppressed(rule: Option<&str>) -> bool {
    rule.is_some_and(|id| SUPPRESSED.contains(&id))
}
