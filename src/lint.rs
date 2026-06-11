use std::path::Path;

use crate::config::{RuleLevel, YamlLintConfig};
use crate::decoder;
use crate::rules::{
    anchors, block_scalar_chomping, braces, brackets, colons, commas, comments,
    comments_indentation, document_end, document_start, empty_lines, empty_values,
    float_values, hyphens, indentation, key_duplicates, key_ordering, line_length,
    merge_keys, new_line_at_end_of_file, new_lines, octal_values, quoted_strings, tags,
    trailing_spaces, truthy, unicode_line_breaks,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintProblem {
    pub line: usize,
    pub column: usize,
    pub level: Severity,
    pub message: String,
    pub rule: Option<&'static str>,
}

struct NullSink;
impl<'i> granit_parser::EventReceiver<'i> for NullSink {
    fn on_event(&mut self, _ev: granit_parser::Event<'i>) {}
}

/// Lint a single YAML file and return diagnostics in yamllint format order.
///
/// # Errors
///
/// Returns `Err(String)` when the file cannot be read.
pub fn lint_file(
    path: &Path,
    cfg: &YamlLintConfig,
    base_dir: &Path,
) -> Result<Vec<LintProblem>, String> {
    let content = decoder::read_file(path)?;
    Ok(lint_str(&content, path, cfg, base_dir))
}

/// Lint the YAML embedded in a markdown file and return diagnostics whose
/// positions point back into the markdown document.
///
/// # Errors
///
/// Returns `Err(String)` when the file cannot be read.
pub fn lint_markdown_file(
    path: &Path,
    cfg: &YamlLintConfig,
    base_dir: &Path,
) -> Result<Vec<LintProblem>, String> {
    let content = decoder::read_file(path)?;
    Ok(crate::markdown_embed::lint_markdown_str(
        &content, path, cfg, base_dir,
    ))
}

/// Run one rule under the standard gate — skip a disabled rule or a
/// per-rule-ignored file — and append a [`LintProblem`] per reported violation in
/// the rule's own report order. This replaces the hand-written `collect_*` helper
/// each rule used to need. The arms cover the shapes rules actually have: a
/// resolved `&Config` or none, a `Vec` or `Option` of violations, and a
/// per-violation message or a fixed module `MESSAGE`. `$m` is the rule module; its
/// `ID` / `check` / `Config` / `MESSAGE` are reached through it.
macro_rules! lint_rule {
    // config, `Vec<Violation>`, per-violation message (the common rule shape)
    ($d:ident, $cfg:expr, $content:expr, $path:expr, $base:expr, $m:ident) => {
        if let Some(level) = $cfg.rule_level($m::ID)
            && !$cfg.is_rule_ignored($m::ID, $path, $base)
        {
            for hit in $m::check($content, &$m::Config::resolve($cfg)) {
                $d.push(LintProblem {
                    line: hit.line,
                    column: hit.column,
                    level: level.into(),
                    message: hit.message,
                    rule: Some($m::ID),
                });
            }
        }
    };
    // config, `Vec<Violation>`, fixed module `MESSAGE`
    ($d:ident, $cfg:expr, $content:expr, $path:expr, $base:expr, $m:ident, message) => {
        if let Some(level) = $cfg.rule_level($m::ID)
            && !$cfg.is_rule_ignored($m::ID, $path, $base)
        {
            for hit in $m::check($content, &$m::Config::resolve($cfg)) {
                $d.push(LintProblem {
                    line: hit.line,
                    column: hit.column,
                    level: level.into(),
                    message: $m::MESSAGE.to_string(),
                    rule: Some($m::ID),
                });
            }
        }
    };
    // no config, `Vec<Violation>`, per-violation message
    ($d:ident, $cfg:expr, $content:expr, $path:expr, $base:expr, $m:ident, no_config) => {
        if let Some(level) = $cfg.rule_level($m::ID)
            && !$cfg.is_rule_ignored($m::ID, $path, $base)
        {
            for hit in $m::check($content) {
                $d.push(LintProblem {
                    line: hit.line,
                    column: hit.column,
                    level: level.into(),
                    message: hit.message,
                    rule: Some($m::ID),
                });
            }
        }
    };
    // no config, `Vec<Violation>`, fixed module `MESSAGE`
    ($d:ident, $cfg:expr, $content:expr, $path:expr, $base:expr, $m:ident, no_config, message) => {
        if let Some(level) = $cfg.rule_level($m::ID)
            && !$cfg.is_rule_ignored($m::ID, $path, $base)
        {
            for hit in $m::check($content) {
                $d.push(LintProblem {
                    line: hit.line,
                    column: hit.column,
                    level: level.into(),
                    message: $m::MESSAGE.to_string(),
                    rule: Some($m::ID),
                });
            }
        }
    };
    // no config, `Option<Violation>`, fixed module `MESSAGE` (new-line-at-end-of-file)
    ($d:ident, $cfg:expr, $content:expr, $path:expr, $base:expr, $m:ident, option, message) => {
        if let Some(level) = $cfg.rule_level($m::ID)
            && !$cfg.is_rule_ignored($m::ID, $path, $base)
            && let Some(hit) = $m::check($content)
        {
            $d.push(LintProblem {
                line: hit.line,
                column: hit.column,
                level: level.into(),
                message: $m::MESSAGE.to_string(),
                rule: Some($m::ID),
            });
        }
    };
    // config by value + platform newline, `Option<Violation>`, per-violation
    // message (new-lines: the platform default is injected for testability)
    ($d:ident, $cfg:expr, $content:expr, $path:expr, $base:expr, $m:ident, platform) => {
        if let Some(level) = $cfg.rule_level($m::ID)
            && !$cfg.is_rule_ignored($m::ID, $path, $base)
            && let Some(hit) =
                $m::check($content, $m::Config::resolve($cfg), $m::platform_newline())
        {
            $d.push(LintProblem {
                line: hit.line,
                column: hit.column,
                level: level.into(),
                message: hit.message,
                rule: Some($m::ID),
            });
        }
    };
}

// `lint_str`'s rule dispatch is split into three ordered batches purely to keep
// each function within clippy's cognitive-complexity threshold (one flat 26-rule
// table trips it). The batch boundaries are pragmatic, not a strict taxonomy; what
// matters is that they run in this order — layout, then value, then block —
// because that concatenation IS ryl's reported diagnostic order (there is no later
// per-file sort) and must match yamllint's. The `yamllint_compat_*` suite guards
// it, so keep the overall sequence stable when editing.

/// Document-shape and layout / punctuation rules (first dispatch batch).
fn collect_layout_diagnostics(
    diagnostics: &mut Vec<LintProblem>,
    content: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
) {
    lint_rule!(diagnostics, cfg, content, path, base_dir, document_start);
    lint_rule!(diagnostics, cfg, content, path, base_dir, document_end);
    lint_rule!(
        diagnostics,
        cfg,
        content,
        path,
        base_dir,
        new_line_at_end_of_file,
        option,
        message
    );
    lint_rule!(
        diagnostics,
        cfg,
        content,
        path,
        base_dir,
        new_lines,
        platform
    );
    lint_rule!(diagnostics, cfg, content, path, base_dir, empty_lines);
    lint_rule!(diagnostics, cfg, content, path, base_dir, commas);
    lint_rule!(diagnostics, cfg, content, path, base_dir, colons);
    lint_rule!(diagnostics, cfg, content, path, base_dir, braces);
    lint_rule!(diagnostics, cfg, content, path, base_dir, brackets);
}

/// Comment, node-property, and scalar-value rules (second dispatch batch).
fn collect_value_diagnostics(
    diagnostics: &mut Vec<LintProblem>,
    content: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
) {
    lint_rule!(diagnostics, cfg, content, path, base_dir, comments);
    lint_rule!(diagnostics, cfg, content, path, base_dir, anchors);
    lint_rule!(diagnostics, cfg, content, path, base_dir, tags);
    lint_rule!(diagnostics, cfg, content, path, base_dir, octal_values);
    lint_rule!(diagnostics, cfg, content, path, base_dir, float_values);
    lint_rule!(diagnostics, cfg, content, path, base_dir, empty_values);
    lint_rule!(diagnostics, cfg, content, path, base_dir, quoted_strings);
    lint_rule!(diagnostics, cfg, content, path, base_dir, truthy);
}

/// Key, indentation, and line / whitespace rules (third dispatch batch).
fn collect_block_diagnostics(
    diagnostics: &mut Vec<LintProblem>,
    content: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
) {
    lint_rule!(diagnostics, cfg, content, path, base_dir, key_duplicates);
    lint_rule!(diagnostics, cfg, content, path, base_dir, key_ordering);
    lint_rule!(diagnostics, cfg, content, path, base_dir, hyphens, message);
    lint_rule!(
        diagnostics,
        cfg,
        content,
        path,
        base_dir,
        comments_indentation,
        message
    );
    lint_rule!(diagnostics, cfg, content, path, base_dir, indentation);
    lint_rule!(diagnostics, cfg, content, path, base_dir, line_length);
    lint_rule!(
        diagnostics,
        cfg,
        content,
        path,
        base_dir,
        trailing_spaces,
        no_config,
        message
    );
    lint_rule!(
        diagnostics,
        cfg,
        content,
        path,
        base_dir,
        unicode_line_breaks,
        no_config
    );
    lint_rule!(
        diagnostics,
        cfg,
        content,
        path,
        base_dir,
        merge_keys,
        no_config
    );
    lint_rule!(
        diagnostics,
        cfg,
        content,
        path,
        base_dir,
        block_scalar_chomping,
        no_config,
        message
    );
}

/// Lint YAML content held in memory and return diagnostics in yamllint format
/// order.
///
/// `path` is used purely for diagnostic context and per-rule ignore matching;
/// no filesystem reads are performed.
#[must_use]
pub fn lint_str(
    content: &str,
    path: &Path,
    cfg: &YamlLintConfig,
    base_dir: &Path,
) -> Vec<LintProblem> {
    if crate::directives::disables_file(content) {
        return Vec::new();
    }

    let mut diagnostics: Vec<LintProblem> = Vec::new();
    collect_layout_diagnostics(&mut diagnostics, content, cfg, path, base_dir);
    collect_value_diagnostics(&mut diagnostics, content, cfg, path, base_dir);
    collect_block_diagnostics(&mut diagnostics, content, cfg, path, base_dir);

    let per_line = cfg.per_line_applies(path, base_dir);
    let directives =
        crate::directives::Directives::parse_with_per_line(content, &per_line);
    diagnostics.retain(|problem| {
        !problem
            .rule
            .is_some_and(|rule| directives.is_disabled(rule, problem.line))
    });

    if let Some(syntax) = syntax_diagnostic(content) {
        diagnostics.clear();
        diagnostics.push(syntax);
    }

    diagnostics
}

fn scan(content: &str) -> Result<(), granit_parser::ScanError> {
    let mut parser = granit_parser::Parser::new_from_str(content);
    let mut sink = NullSink;
    parser.load(&mut sink, true)
}

fn syntax_problem(err: &granit_parser::ScanError) -> LintProblem {
    let marker = err.marker();
    LintProblem {
        line: marker.line(),
        column: marker.col() + 1,
        level: Severity::Error,
        message: format!("syntax error: {} (syntax)", err.info()),
        rule: None,
    }
}

/// Any granit parse error as a diagnostic, or `None` if `content` parses. Stricter
/// than [`syntax_diagnostic`]: it does *not* suppress the undefined-alias error.
/// The `--fix` gate uses this so it refuses to mutate any file granit cannot fully
/// parse — and always reports why — rather than the lint view that tolerates
/// undefined aliases.
pub(crate) fn parse_error(content: &str) -> Option<LintProblem> {
    scan(content).err().as_ref().map(syntax_problem)
}

/// The syntax error ryl reports for `content` during linting, or `None` if it
/// lints cleanly. Deliberately suppresses granit's undefined-alias error (ryl
/// reports that via the `anchors` rule, matching yamllint), so lint stays
/// yamllint-compatible.
fn syntax_diagnostic(content: &str) -> Option<LintProblem> {
    match scan(content) {
        Ok(()) => None,
        Err(err) if err.info() == "while parsing node, found unknown anchor" => {
            // The parser stops at the tolerated undefined alias, which would mask a
            // later lexical error (e.g. an empty anchor name). The scanner tokenises
            // undefined aliases without erroring, so it surfaces that real syntax
            // error; a clean scan means the only problem is the alias (lint reports
            // it via the `anchors` rule, matching yamllint).
            scanner_error(content).map(|err| syntax_problem(&err))
        }
        Err(err) => Some(syntax_problem(&err)),
    }
}

/// The first lexical scan error in `content`, or `None`. Used by
/// [`syntax_diagnostic`] to find a malformed-token error the parser cannot reach
/// because it halts on an earlier (tolerated) undefined alias.
fn scanner_error(content: &str) -> Option<granit_parser::ScanError> {
    let mut scanner =
        granit_parser::Scanner::new(granit_parser::StrInput::new(content));
    loop {
        match scanner.next_token() {
            Ok(Some(_)) => {}
            Ok(None) => return None,
            Err(err) => return Some(err),
        }
    }
}
