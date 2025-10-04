use std::fs;
use std::path::Path;

use crate::config::{RuleLevel, YamlLintConfig};
use crate::rules::{
    colons, commas, comments, comments_indentation, document_end, document_start, empty_lines,
    empty_values, float_values, hyphens, indentation, key_duplicates, key_ordering, line_length,
    new_line_at_end_of_file, new_lines, octal_values, quoted_strings, trailing_spaces, truthy,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }
}

impl From<RuleLevel> for Severity {
    fn from(value: RuleLevel) -> Self {
        match value {
            RuleLevel::Error => Self::Error,
            RuleLevel::Warning => Self::Warning,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LintProblem {
    pub line: usize,
    pub column: usize,
    pub level: Severity,
    pub message: String,
    pub rule: Option<&'static str>,
}

struct NullSink;
impl<'i> saphyr_parser::EventReceiver<'i> for NullSink {
    fn on_event(&mut self, _ev: saphyr_parser::Event<'i>) {}
}

/// Lint a single YAML file and return diagnostics in yamllint format order.
///
/// # Errors
///
/// Returns `Err(String)` when the file cannot be read.
#[allow(clippy::too_many_lines)]
pub fn lint_file(
    path: &Path,
    cfg: &YamlLintConfig,
    base_dir: &Path,
) -> Result<Vec<LintProblem>, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {}", path.display(), e))?;

    let mut diagnostics: Vec<LintProblem> = Vec::new();

    collect_document_start_diagnostics(&mut diagnostics, &content, cfg, path, base_dir);

    collect_document_end_diagnostics(&mut diagnostics, &content, cfg, path, base_dir);

    if let Some(level) = cfg.rule_level(new_line_at_end_of_file::ID)
        && !cfg.is_rule_ignored(new_line_at_end_of_file::ID, path, base_dir)
        && let Some(hit) = new_line_at_end_of_file::check(&content)
    {
        diagnostics.push(LintProblem {
            line: hit.line,
            column: hit.column,
            level: level.into(),
            message: new_line_at_end_of_file::MESSAGE.to_string(),
            rule: Some(new_line_at_end_of_file::ID),
        });
    }

    if let Some(level) = cfg.rule_level(new_lines::ID)
        && !cfg.is_rule_ignored(new_lines::ID, path, base_dir)
    {
        let rule_cfg = new_lines::Config::resolve(cfg);
        if let Some(hit) = new_lines::check(&content, rule_cfg, new_lines::platform_newline()) {
            diagnostics.push(LintProblem {
                line: hit.line,
                column: hit.column,
                level: level.into(),
                message: hit.message,
                rule: Some(new_lines::ID),
            });
        }
    }

    collect_empty_lines_diagnostics(&mut diagnostics, &content, cfg, path, base_dir);

    collect_commas_diagnostics(&mut diagnostics, &content, cfg, path, base_dir);

    collect_colons_diagnostics(&mut diagnostics, &content, cfg, path, base_dir);

    collect_comments_diagnostics(&mut diagnostics, &content, cfg, path, base_dir);

    if let Some(level) = cfg.rule_level(octal_values::ID)
        && !cfg.is_rule_ignored(octal_values::ID, path, base_dir)
    {
        let rule_cfg = octal_values::Config::resolve(cfg);
        for hit in octal_values::check(&content, &rule_cfg) {
            diagnostics.push(LintProblem {
                line: hit.line,
                column: hit.column,
                level: level.into(),
                message: hit.message,
                rule: Some(octal_values::ID),
            });
        }
    }

    if let Some(level) = cfg.rule_level(float_values::ID)
        && !cfg.is_rule_ignored(float_values::ID, path, base_dir)
    {
        let rule_cfg = float_values::Config::resolve(cfg);
        for hit in float_values::check(&content, &rule_cfg) {
            diagnostics.push(LintProblem {
                line: hit.line,
                column: hit.column,
                level: level.into(),
                message: hit.message,
                rule: Some(float_values::ID),
            });
        }
    }

    if let Some(level) = cfg.rule_level(empty_values::ID)
        && !cfg.is_rule_ignored(empty_values::ID, path, base_dir)
    {
        let rule_cfg = empty_values::Config::resolve(cfg);
        for hit in empty_values::check(&content, &rule_cfg) {
            diagnostics.push(LintProblem {
                line: hit.line,
                column: hit.column,
                level: level.into(),
                message: hit.message,
                rule: Some(empty_values::ID),
            });
        }
    }

    if let Some(level) = cfg.rule_level(quoted_strings::ID)
        && !cfg.is_rule_ignored(quoted_strings::ID, path, base_dir)
    {
        let rule_cfg = quoted_strings::Config::resolve(cfg);
        for hit in quoted_strings::check(&content, &rule_cfg) {
            diagnostics.push(LintProblem {
                line: hit.line,
                column: hit.column,
                level: level.into(),
                message: hit.message,
                rule: Some(quoted_strings::ID),
            });
        }
    }

    if let Some(level) = cfg.rule_level(truthy::ID)
        && !cfg.is_rule_ignored(truthy::ID, path, base_dir)
    {
        let rule_cfg = truthy::Config::resolve(cfg);
        for hit in truthy::check(&content, &rule_cfg) {
            let truthy::Violation {
                line,
                column,
                message,
            } = hit;
            diagnostics.push(LintProblem {
                line,
                column,
                level: level.into(),
                message,
                rule: Some(truthy::ID),
            });
        }
    }

    if let Some(level) = cfg.rule_level(key_duplicates::ID)
        && !cfg.is_rule_ignored(key_duplicates::ID, path, base_dir)
    {
        let rule_cfg = key_duplicates::Config::resolve(cfg);
        for hit in key_duplicates::check(&content, &rule_cfg) {
            diagnostics.push(LintProblem {
                line: hit.line,
                column: hit.column,
                level: level.into(),
                message: hit.message,
                rule: Some(key_duplicates::ID),
            });
        }
    }

    if let Some(level) = cfg.rule_level(key_ordering::ID)
        && !cfg.is_rule_ignored(key_ordering::ID, path, base_dir)
    {
        let rule_cfg = key_ordering::Config::resolve(cfg);
        for hit in key_ordering::check(&content, &rule_cfg) {
            diagnostics.push(LintProblem {
                line: hit.line,
                column: hit.column,
                level: level.into(),
                message: hit.message,
                rule: Some(key_ordering::ID),
            });
        }
    }

    if let Some(level) = cfg.rule_level(hyphens::ID)
        && !cfg.is_rule_ignored(hyphens::ID, path, base_dir)
    {
        let rule_cfg = hyphens::Config::resolve(cfg);
        for hit in hyphens::check(&content, &rule_cfg) {
            diagnostics.push(LintProblem {
                line: hit.line,
                column: hit.column,
                level: level.into(),
                message: hyphens::MESSAGE.to_string(),
                rule: Some(hyphens::ID),
            });
        }
    }

    collect_comments_indentation_diagnostics(&mut diagnostics, &content, cfg, path, base_dir);

    if let Some(level) = cfg.rule_level(indentation::ID)
        && !cfg.is_rule_ignored(indentation::ID, path, base_dir)
    {
        let rule_cfg = indentation::Config::resolve(cfg);
        for hit in indentation::check(&content, &rule_cfg) {
            diagnostics.push(LintProblem {
                line: hit.line,
                column: hit.column,
                level: level.into(),
                message: hit.message,
                rule: Some(indentation::ID),
            });
        }
    }

    collect_line_length_diagnostics(&mut diagnostics, &content, cfg, path, base_dir);

    if let Some(level) = cfg.rule_level(trailing_spaces::ID)
        && !cfg.is_rule_ignored(trailing_spaces::ID, path, base_dir)
    {
        for hit in trailing_spaces::check(&content) {
            diagnostics.push(LintProblem {
                line: hit.line,
                column: hit.column,
                level: level.into(),
                message: trailing_spaces::MESSAGE.to_string(),
                rule: Some(trailing_spaces::ID),
            });
        }
    }

    if let Some(syntax) = syntax_diagnostic(&content) {
        diagnostics.clear();
        diagnostics.push(syntax);
    }

    Ok(diagnostics)
}

fn collect_document_end_diagnostics(
    diagnostics: &mut Vec<LintProblem>,
    content: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
) {
    if let Some(level) = cfg.rule_level(document_end::ID)
        && !cfg.is_rule_ignored(document_end::ID, path, base_dir)
    {
        let rule_cfg = document_end::Config::resolve(cfg);
        for hit in document_end::check(content, &rule_cfg) {
            diagnostics.push(LintProblem {
                line: hit.line,
                column: hit.column,
                level: level.into(),
                message: hit.message,
                rule: Some(document_end::ID),
            });
        }
    }
}

fn collect_document_start_diagnostics(
    diagnostics: &mut Vec<LintProblem>,
    content: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
) {
    if let Some(level) = cfg.rule_level(document_start::ID)
        && !cfg.is_rule_ignored(document_start::ID, path, base_dir)
    {
        let rule_cfg = document_start::Config::resolve(cfg);
        for hit in document_start::check(content, &rule_cfg) {
            diagnostics.push(LintProblem {
                line: hit.line,
                column: hit.column,
                level: level.into(),
                message: hit.message,
                rule: Some(document_start::ID),
            });
        }
    }
}

fn collect_empty_lines_diagnostics(
    diagnostics: &mut Vec<LintProblem>,
    content: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
) {
    if let Some(level) = cfg.rule_level(empty_lines::ID)
        && !cfg.is_rule_ignored(empty_lines::ID, path, base_dir)
    {
        let rule_cfg = empty_lines::Config::resolve(cfg);
        for hit in empty_lines::check(content, &rule_cfg) {
            diagnostics.push(LintProblem {
                line: hit.line,
                column: hit.column,
                level: level.into(),
                message: hit.message,
                rule: Some(empty_lines::ID),
            });
        }
    }
}

fn collect_commas_diagnostics(
    diagnostics: &mut Vec<LintProblem>,
    content: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
) {
    if let Some(level) = cfg.rule_level(commas::ID)
        && !cfg.is_rule_ignored(commas::ID, path, base_dir)
    {
        let rule_cfg = commas::Config::resolve(cfg);
        for hit in commas::check(content, &rule_cfg) {
            diagnostics.push(LintProblem {
                line: hit.line,
                column: hit.column,
                level: level.into(),
                message: hit.message,
                rule: Some(commas::ID),
            });
        }
    }
}

fn collect_colons_diagnostics(
    diagnostics: &mut Vec<LintProblem>,
    content: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
) {
    if let Some(level) = cfg.rule_level(colons::ID)
        && !cfg.is_rule_ignored(colons::ID, path, base_dir)
    {
        let rule_cfg = colons::Config::resolve(cfg);
        for hit in colons::check(content, &rule_cfg) {
            diagnostics.push(LintProblem {
                line: hit.line,
                column: hit.column,
                level: level.into(),
                message: hit.message,
                rule: Some(colons::ID),
            });
        }
    }
}

fn collect_comments_diagnostics(
    diagnostics: &mut Vec<LintProblem>,
    content: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
) {
    if let Some(level) = cfg.rule_level(comments::ID)
        && !cfg.is_rule_ignored(comments::ID, path, base_dir)
    {
        let rule_cfg = comments::Config::resolve(cfg);
        for hit in comments::check(content, &rule_cfg) {
            diagnostics.push(LintProblem {
                line: hit.line,
                column: hit.column,
                level: level.into(),
                message: hit.message,
                rule: Some(comments::ID),
            });
        }
    }
}

fn collect_comments_indentation_diagnostics(
    diagnostics: &mut Vec<LintProblem>,
    content: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
) {
    if let Some(level) = cfg.rule_level(comments_indentation::ID)
        && !cfg.is_rule_ignored(comments_indentation::ID, path, base_dir)
    {
        let rule_cfg = comments_indentation::Config::resolve(cfg);
        for hit in comments_indentation::check(content, &rule_cfg) {
            diagnostics.push(LintProblem {
                line: hit.line,
                column: hit.column,
                level: level.into(),
                message: comments_indentation::MESSAGE.to_string(),
                rule: Some(comments_indentation::ID),
            });
        }
    }
}

fn collect_line_length_diagnostics(
    diagnostics: &mut Vec<LintProblem>,
    content: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
) {
    if let Some(level) = cfg.rule_level(line_length::ID)
        && !cfg.is_rule_ignored(line_length::ID, path, base_dir)
    {
        let rule_cfg = line_length::Config::resolve(cfg);
        for hit in line_length::check(content, &rule_cfg) {
            diagnostics.push(LintProblem {
                line: hit.line,
                column: hit.column,
                level: level.into(),
                message: hit.message,
                rule: Some(line_length::ID),
            });
        }
    }
}

fn syntax_diagnostic(content: &str) -> Option<LintProblem> {
    let mut parser = saphyr_parser::Parser::new_from_str(content);
    let mut sink = NullSink;
    match parser.load(&mut sink, true) {
        Ok(()) => None,
        Err(err) => {
            let marker = err.marker();
            let column = marker.col() + 1;
            Some(LintProblem {
                line: marker.line(),
                column,
                level: Severity::Error,
                message: format!("syntax error: {} (syntax)", err.info()),
                rule: None,
            })
        }
    }
}
