//! Maps the lint/fix engine results into LSP types. No lint/fix logic lives here: it
//! reuses `lint_str` / `lint_markdown_str` and the CLI's `rewrite_str` /
//! `apply_safe_fixes_filtered`.

use std::path::Path;

use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, TextEdit};
use sha2::{Digest, Sha256};

use crate::config::{SourceKind, YamlLintConfig};
use crate::fix::{
    Rewrite, SAFE_FIX_RULE_IDS, UNSAFE_FIX_RULE_IDS, apply_safe_fixes_filtered,
    rewrite_str,
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

/// Splits off a leading UTF-8 BOM, which the CLI's decoder strips before linting or
/// rewriting, so the engine sees the same text here as on the command line.
fn split_bom(text: &str) -> (&str, &str) {
    text.strip_prefix('\u{feff}')
        .map_or(("", text), |body| ("\u{feff}", body))
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
    let (bom, body) = split_bom(text);
    let problems = match kind {
        SourceKind::Markdown => lint_markdown_str(body, path, cfg, base_dir),
        SourceKind::Yaml => lint_str(body, path, cfg, base_dir),
    };
    let lines = line_contents(text);
    problems
        .into_iter()
        .map(|mut problem| {
            // Columns on the first line count from after the BOM; the buffer keeps it.
            if problem.line == 1 {
                problem.column += bom.chars().count();
            }
            to_diagnostic(&lines, problem, enc)
        })
        .collect()
}

/// The whole-document edit applying `rewrite` as the CLI does, or `None` when nothing
/// changes or the document is refused (it does not parse, or is markdown with a bare CR).
#[must_use]
pub fn rewrite_edit(
    text: &str,
    path: &Path,
    cfg: &YamlLintConfig,
    base_dir: &Path,
    kind: SourceKind,
    enc: PositionEncoding,
    rewrite: Rewrite,
) -> Option<TextEdit> {
    rewrite_edit_with_notices(text, path, cfg, base_dir, kind, enc, rewrite).0
}

pub(crate) fn rewrite_edit_with_notices(
    text: &str,
    path: &Path,
    cfg: &YamlLintConfig,
    base_dir: &Path,
    kind: SourceKind,
    enc: PositionEncoding,
    rewrite: Rewrite,
) -> (Option<TextEdit>, Vec<String>) {
    let (bom, body) = split_bom(text);
    let (rewritten, notices) = rewrite_str(body, cfg, path, base_dir, kind, rewrite);
    let notices = notices
        .iter()
        .filter(|notice| notice.level == Severity::Warning && notice.rule.is_none())
        .map(|notice| crate::fix::skip_notice(path, notice, rewrite.flag()))
        .collect();
    let edit = rewritten
        .filter(|rewritten| rewritten != body)
        .map(|rewritten| {
            TextEdit::new(full_range(text, enc), format!("{bom}{rewritten}"))
        });
    (edit, notices)
}

/// The whole-document edit applying only the fixes `cfg` applies for `rule`, or `None`
/// when nothing changes, `rule` has no fix, or `kind` is not plain YAML (the markdown fix
/// path has no per-rule variant).
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
    let fix_rules = || {
        SAFE_FIX_RULE_IDS
            .iter()
            .chain(&UNSAFE_FIX_RULE_IDS)
            .copied()
    };
    if matches!(kind, SourceKind::Markdown) || !fix_rules().any(|id| id == rule) {
        return None;
    }
    let skip: Vec<&str> = fix_rules()
        .filter(|id| *id != rule)
        .chain(std::iter::once(crate::rules::indentation::ID))
        .collect();
    let (bom, body) = split_bom(text);
    let fixed = apply_safe_fixes_filtered(body, cfg, path, base_dir, &skip);
    (fixed != body)
        .then(|| TextEdit::new(full_range(text, enc), format!("{bom}{fixed}")))
}

/// A fingerprint of `items`, returned as a `resultId` and sent back on the next pull so an
/// unchanged file is answered `Unchanged`. `None` for a clean file, which the client then
/// stops tracking.
#[must_use]
pub fn result_id(items: &[Diagnostic]) -> Option<String> {
    if items.is_empty() {
        return None;
    }
    // `Debug` over serialisation: derived, so it renders every field, and infallible.
    let rendered = format!("{items:?}");
    Some(crate::report::hex_digest(&Sha256::digest(
        rendered.as_bytes(),
    )))
}
