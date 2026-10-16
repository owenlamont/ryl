use std::path::Path;

use crate::config::{RuleLevel, YamlLintConfig};
use crate::decoder;
use crate::rules::support::yaml_version;
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

macro_rules! emit_problems {
    ($diagnostics:expr, $rule:ident, $hits:expr, $level:expr, $message:expr) => {
        $diagnostics.extend($hits.into_iter().map(|hit| $crate::lint::LintProblem {
            line: hit.line,
            column: hit.column,
            level: $level,
            message: $message(hit),
            rule: Some($rule::ID),
        }))
    };
}

pub(crate) use emit_problems;

macro_rules! lint_rule {
    ($d:ident, $cfg:expr, $content:expr, $path:expr, $base:expr, $m:ident) => {
        lint_rule!(@gated $d, $cfg, $path, $base, $m,
            $m::check($content, &$m::Config::resolve($cfg)),
            |hit: $m::Violation| hit.message);
    };
    ($d:ident, $cfg:expr, $content:expr, $path:expr, $base:expr, $m:ident, path) => {
        lint_rule!(@gated $d, $cfg, $path, $base, $m,
            $m::check($content, &$m::Config::resolve($cfg, $path)),
            |hit: $m::Violation| hit.message);
    };
    ($d:ident, $cfg:expr, $content:expr, $path:expr, $base:expr, $m:ident, message) => {
        lint_rule!(@gated $d, $cfg, $path, $base, $m,
            $m::check($content, &$m::Config::resolve($cfg)),
            |_| $m::MESSAGE.to_string());
    };
    ($d:ident, $cfg:expr, $content:expr, $path:expr, $base:expr, $m:ident, no_config) => {
        lint_rule!(@gated $d, $cfg, $path, $base, $m,
            $m::check($content),
            |hit: $m::Violation| hit.message);
    };
    ($d:ident, $cfg:expr, $content:expr, $path:expr, $base:expr, $m:ident, no_config, message) => {
        lint_rule!(@gated $d, $cfg, $path, $base, $m,
            $m::check($content),
            |_| $m::MESSAGE.to_string());
    };
    ($d:ident, $cfg:expr, $content:expr, $path:expr, $base:expr, $m:ident, platform) => {
        lint_rule!(@gated $d, $cfg, $path, $base, $m,
            $m::check($content, $m::Config::resolve($cfg), $m::platform_newline()),
            |hit: $m::Violation| hit.message);
    };
    (@gated $d:ident, $cfg:expr, $path:expr, $base:expr, $m:ident, $hits:expr, $message:expr) => {
        if let Some(level) = $cfg.rule_level($m::ID)
            && !$cfg.is_rule_ignored($m::ID, $path, $base)
        {
            emit_problems!($d, $m, $hits, level.into(), $message);
        }
    };
}

// Split dispatch for clippy's complexity limit; preserve layout/value/block order
// because diagnostics are not sorted after collection.
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
        no_config,
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

fn collect_block_diagnostics(
    diagnostics: &mut Vec<LintProblem>,
    content: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
) {
    lint_rule!(diagnostics, cfg, content, path, base_dir, key_duplicates);
    lint_rule!(
        diagnostics,
        cfg,
        content,
        path,
        base_dir,
        key_ordering,
        path
    );
    lint_rule!(diagnostics, cfg, content, path, base_dir, hyphens);
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

    let per_line = cfg.per_line_applies(path);
    let directives =
        crate::directives::Directives::parse_with_per_line(content, &per_line);
    diagnostics.retain(|problem| {
        !problem
            .rule
            .is_some_and(|rule| directives.is_disabled(rule, problem.line))
    });

    if let Some(warning) = higher_minor_version_warning(content) {
        diagnostics.push(warning);
    }

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

/// Any granit parse error as a diagnostic, or `None` if `content` parses. Stricter than
/// [`syntax_diagnostic`]: it does *not* suppress the undefined-alias error. The `--fix`
/// gate uses this to refuse to mutate any file granit cannot fully parse, rather than the
/// lint view that tolerates undefined aliases.
pub(crate) fn parse_error(content: &str) -> Option<LintProblem> {
    unsupported_version_error(content)
        .or_else(|| scan(content).err().as_ref().map(syntax_problem))
}

/// A `%YAML` directive whose major version is not 1; the spec mandates rejecting a
/// higher major version, so ryl surfaces it as a syntax error (yamllint parity).
fn unsupported_version_error(content: &str) -> Option<LintProblem> {
    yaml_version::first_unsupported_major(content).map(|directive| LintProblem {
        line: directive.line,
        column: directive.column,
        level: Severity::Error,
        message: "syntax error: found incompatible YAML document (version 1.* is \
                  required) (syntax)"
            .to_string(),
        rule: None,
    })
}

/// A `%YAML 1.x` directive with a minor above 2 is processed as 1.2 with a warning, as
/// the spec directs for a higher minor version.
fn higher_minor_version_warning(content: &str) -> Option<LintProblem> {
    yaml_version::first_higher_minor(content).map(|directive| LintProblem {
        line: directive.line,
        column: directive.column,
        level: Severity::Warning,
        message: format!(
            "YAML version {}.{} is newer than 1.2; processing as YAML 1.2",
            directive.version.0, directive.version.1
        ),
        rule: None,
    })
}

/// The syntax error ryl reports for `content` during linting, or `None` if it lints
/// cleanly. Suppresses granit's undefined-alias error (ryl reports that via the `anchors`
/// rule, matching yamllint).
fn syntax_diagnostic(content: &str) -> Option<LintProblem> {
    if let Some(problem) = unsupported_version_error(content) {
        return Some(problem);
    }
    match scan(content) {
        Ok(()) => None,
        Err(err) if err.info() == "while parsing node, found unknown anchor" => {
            // The parser halts at the tolerated undefined alias, masking any later lexical
            // error (e.g. an empty anchor name). The scanner tokenises undefined aliases
            // without erroring, so it surfaces that real error; a clean scan means the
            // alias is the only problem (reported via the `anchors` rule).
            scanner_error(content).map(|err| syntax_problem(&err))
        }
        Err(err) => Some(syntax_problem(&err)),
    }
}

/// The first lexical scan error in `content`, or `None`. [`syntax_diagnostic`] uses it to
/// find a malformed-token error the parser cannot reach, having halted on an earlier
/// (tolerated) undefined alias.
fn scanner_error(content: &str) -> Option<granit_parser::ScanError> {
    granit_parser::Scanner::new(granit_parser::StrInput::new(content))
        .find_map(Result::err)
}
