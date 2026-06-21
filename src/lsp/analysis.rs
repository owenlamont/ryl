//! Maps the lint/fix engine results into LSP types. No lint/fix logic lives here: it
//! reuses `lint_str` / `lint_markdown_str` and `apply_safe_fixes` / `fix_markdown_str`.

use std::path::Path;

use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, TextEdit};

use crate::config::{SourceKind, YamlLintConfig};
use crate::fix::{
    SAFE_FIX_RULE_IDS, apply_safe_fixes, apply_safe_fixes_filtered, fix_markdown_str,
};
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

/// The whole-document edit applying every safe fix, or `None` when nothing changes
/// (the fix engine returns the input unchanged for an unparsable file; markdown with
/// an unsupported bare CR yields `None`).
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

/// The whole-document edit applying only `rule`'s safe fix, or `None` when nothing
/// changes, `rule` has no safe fix, or `kind` is not plain YAML (the markdown fix path
/// has no per-rule variant).
#[must_use]
pub fn fix_rule_edit(
    text: &str,
    path: &Path,
    cfg: &YamlLintConfig,
    base_dir: &Path,
    kind: SourceKind,
    enc: PositionEncoding,
    rule: &str,
) -> Option<TextEdit> {
    if matches!(kind, SourceKind::Markdown) || !SAFE_FIX_RULE_IDS.contains(&rule) {
        return None;
    }
    let skip: Vec<&str> = SAFE_FIX_RULE_IDS
        .iter()
        .copied()
        .filter(|id| *id != rule)
        .collect();
    let fixed = apply_safe_fixes_filtered(text, cfg, path, base_dir, &skip);
    (fixed != text).then(|| TextEdit::new(full_range(text, enc), fixed))
}
