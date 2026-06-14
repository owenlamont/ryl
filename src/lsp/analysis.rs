//! Bridges an in-memory document to ryl's existing lint and fix engine and maps
//! the results into LSP types. No linting or fixing logic lives here: it reuses
//! `lint_str` / `lint_markdown_str` for diagnostics and `apply_safe_fixes` /
//! `fix_markdown_str` for the whole-file fix, exactly as the CLI does.

use std::path::Path;

use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, TextEdit};

use crate::config::{SourceKind, YamlLintConfig};
use crate::fix::{apply_safe_fixes, fix_markdown_str};
use crate::lint::{LintProblem, Severity, lint_str};
use crate::lsp::encoding::{PositionEncoding, full_range, problem_range};
use crate::markdown_embed::lint_markdown_str;
use crate::rules::support::line_syntax::line_contents;

fn severity(level: Severity) -> DiagnosticSeverity {
    match level {
        Severity::Error => DiagnosticSeverity::ERROR,
        Severity::Warning => DiagnosticSeverity::WARNING,
    }
}

fn to_diagnostic(
    lines: &[&str],
    problem: LintProblem,
    enc: PositionEncoding,
) -> Diagnostic {
    Diagnostic {
        range: problem_range(lines, problem.line, problem.column, enc),
        severity: Some(severity(problem.level)),
        code: problem
            .rule
            .map(|rule| NumberOrString::String(rule.to_string())),
        source: Some("ryl".to_string()),
        message: problem.message,
        ..Default::default()
    }
}

/// Lint `text` (already resolved to `kind`) and convert each problem to an LSP
/// diagnostic under the negotiated `enc`.
#[must_use]
pub fn diagnostics(
    text: &str,
    path: &Path,
    cfg: &YamlLintConfig,
    base_dir: &Path,
    kind: SourceKind,
    enc: PositionEncoding,
) -> Vec<Diagnostic> {
    let problems = match kind {
        SourceKind::Markdown => lint_markdown_str(text, path, cfg, base_dir),
        SourceKind::Yaml => lint_str(text, path, cfg, base_dir),
    };
    let lines = line_contents(text);
    problems
        .into_iter()
        .map(|problem| to_diagnostic(&lines, problem, enc))
        .collect()
}

/// The whole-document edit that applies every safe fix, or `None` when the
/// content already conforms or cannot be fixed (the fix engine returns the input
/// unchanged for an unparsable file; markdown with an unsupported bare CR yields
/// `None`). Backs both `source.fixAll.ryl` and `textDocument/formatting`.
#[must_use]
pub fn fix_all_edit(
    text: &str,
    path: &Path,
    cfg: &YamlLintConfig,
    base_dir: &Path,
    kind: SourceKind,
    enc: PositionEncoding,
) -> Option<TextEdit> {
    let fixed = match kind {
        SourceKind::Markdown => fix_markdown_str(text, path, cfg, base_dir)?,
        SourceKind::Yaml => apply_safe_fixes(text, cfg, path, base_dir),
    };
    (fixed != text).then(|| TextEdit::new(full_range(text, enc), fixed))
}
