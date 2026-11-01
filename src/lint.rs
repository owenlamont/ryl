use std::path::Path;

use crate::config::{RuleLevel, SourceKind, YamlLintConfig};
use crate::decoder;
use crate::fix::{Rewrite, rewrite_str};
use crate::rules::support::yaml_version;
use crate::rules::{
    anchors, block_scalar_chomping, braces, brackets, colons, commas, comments,
    comments_indentation, document_end, document_start, empty_lines, empty_values,
    float_values, hyphens, indentation, key_duplicates, key_ordering, line_length,
    merge_keys, new_line_at_end_of_file, new_lines, octal_values, quoted_strings,
    schema_dependent_values, tags, trailing_spaces, truthy, unicode_line_breaks,
    yaml_directive,
};
use crate::text_version::{Analysis, ParsedFacts, TextVersion};

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
impl<'i> ryl_yaml_parser::EventReceiver<'i> for NullSink {
    fn on_event(&mut self, _ev: ryl_yaml_parser::Event<'i>) {}
}

/// Lint a single YAML file and return diagnostics sorted by line, column and rule.
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
    Ok(lint_str_with_repair_help(&content, path, cfg, base_dir))
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

macro_rules! lint_rule {
    ($d:ident, $cfg:expr, $input:expr, $path:expr, $base:expr, $m:ident, analysis, value) => {
        lint_rule!(@gated $d, $cfg, $path, $base, $m,
            $m::check_with_analysis($input, $m::Config::resolve($cfg)),
            |hit: $m::Violation| hit.message);
    };
    ($d:ident, $cfg:expr, $input:expr, $path:expr, $base:expr, $m:ident, analysis, path) => {
        lint_rule!(@gated $d, $cfg, $path, $base, $m,
            $m::check_with_analysis($input, &$m::Config::resolve($cfg, $path)),
            |hit: $m::Violation| hit.message);
    };
    ($d:ident, $cfg:expr, $input:expr, $path:expr, $base:expr, $m:ident, analysis, no_config) => {
        lint_rule!(@gated $d, $cfg, $path, $base, $m,
            $m::check_with_analysis($input),
            |hit: $m::Violation| hit.message);
    };
    ($d:ident, $cfg:expr, $input:expr, $path:expr, $base:expr, $m:ident, analysis, no_config, message) => {
        lint_rule!(@gated $d, $cfg, $path, $base, $m,
            $m::check_with_analysis($input),
            |_| $m::MESSAGE.to_string());
    };
    ($d:ident, $cfg:expr, $input:expr, $path:expr, $base:expr, $m:ident, analysis) => {
        lint_rule!(@gated $d, $cfg, $path, $base, $m,
            $m::check_with_analysis($input, &$m::Config::resolve($cfg)),
            |hit: $m::Violation| hit.message);
    };
    ($d:ident, $cfg:expr, $input:expr, $path:expr, $base:expr, $m:ident, analysis, message) => {
        lint_rule!(@gated $d, $cfg, $path, $base, $m,
            $m::check_with_analysis($input, $m::Config::resolve($cfg)),
            |_| $m::MESSAGE.to_string());
    };
    ($d:ident, $cfg:expr, $content:expr, $path:expr, $base:expr, $m:ident) => {
        lint_rule!(@gated $d, $cfg, $path, $base, $m,
            $m::check($content, &$m::Config::resolve($cfg)),
            |hit: $m::Violation| hit.message);
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

// Split dispatch for clippy's complexity limit.
fn collect_layout_diagnostics(
    diagnostics: &mut Vec<LintProblem>,
    input: Analysis<'_>,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
) {
    let content = input.text();
    lint_rule!(
        diagnostics,
        cfg,
        input,
        path,
        base_dir,
        document_start,
        analysis,
        value
    );
    lint_rule!(
        diagnostics,
        cfg,
        input,
        path,
        base_dir,
        document_end,
        analysis,
        value
    );
    lint_rule!(
        diagnostics,
        cfg,
        input,
        path,
        base_dir,
        yaml_directive,
        analysis,
        value
    );
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
    lint_rule!(diagnostics, cfg, input, path, base_dir, commas, analysis);
    lint_rule!(diagnostics, cfg, input, path, base_dir, colons, analysis);
    lint_rule!(diagnostics, cfg, input, path, base_dir, braces, analysis);
    lint_rule!(diagnostics, cfg, input, path, base_dir, brackets, analysis);
}

fn collect_value_diagnostics(
    diagnostics: &mut Vec<LintProblem>,
    input: Analysis<'_>,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
) {
    lint_rule!(diagnostics, cfg, input, path, base_dir, comments, analysis);
    lint_rule!(
        diagnostics,
        cfg,
        input,
        path,
        base_dir,
        anchors,
        analysis,
        value
    );
    lint_rule!(diagnostics, cfg, input, path, base_dir, tags, analysis);
    lint_rule!(
        diagnostics,
        cfg,
        input,
        path,
        base_dir,
        octal_values,
        analysis
    );
    lint_rule!(
        diagnostics,
        cfg,
        input,
        path,
        base_dir,
        float_values,
        analysis
    );
    lint_rule!(
        diagnostics,
        cfg,
        input,
        path,
        base_dir,
        empty_values,
        analysis,
        value
    );
    lint_rule!(
        diagnostics,
        cfg,
        input,
        path,
        base_dir,
        quoted_strings,
        analysis
    );
    lint_rule!(diagnostics, cfg, input, path, base_dir, truthy, analysis);
    lint_rule!(
        diagnostics,
        cfg,
        input,
        path,
        base_dir,
        schema_dependent_values,
        analysis
    );
}

fn collect_block_diagnostics(
    diagnostics: &mut Vec<LintProblem>,
    input: Analysis<'_>,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
) {
    let content = input.text();
    lint_rule!(
        diagnostics,
        cfg,
        input,
        path,
        base_dir,
        key_duplicates,
        analysis,
        value
    );
    lint_rule!(
        diagnostics,
        cfg,
        input,
        path,
        base_dir,
        key_ordering,
        analysis,
        path
    );
    lint_rule!(diagnostics, cfg, input, path, base_dir, hyphens, analysis);
    lint_rule!(
        diagnostics,
        cfg,
        input,
        path,
        base_dir,
        comments_indentation,
        analysis,
        message
    );
    lint_rule!(
        diagnostics,
        cfg,
        input,
        path,
        base_dir,
        indentation,
        analysis
    );
    lint_rule!(diagnostics, cfg, content, path, base_dir, line_length);
    lint_rule!(
        diagnostics,
        cfg,
        input,
        path,
        base_dir,
        trailing_spaces,
        analysis,
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
        input,
        path,
        base_dir,
        merge_keys,
        analysis,
        no_config
    );
    lint_rule!(
        diagnostics,
        cfg,
        input,
        path,
        base_dir,
        block_scalar_chomping,
        analysis,
        no_config,
        message
    );
}

/// Lint YAML content held in memory and return diagnostics sorted by line, column and
/// rule.
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
    lint_unless_syntax_error(content, path, cfg, base_dir)
        .unwrap_or_else(|(syntax, _)| vec![syntax])
}

/// [`lint_str`]'s diagnostics, or the syntax error that stops linting and whether a rewrite
/// may repair it.
fn lint_unless_syntax_error(
    content: &str,
    path: &Path,
    cfg: &YamlLintConfig,
    base_dir: &Path,
) -> Result<Vec<LintProblem>, (LintProblem, bool)> {
    if crate::directives::disables_file(content) {
        return Ok(Vec::new());
    }

    let mut diagnostics: Vec<LintProblem> = Vec::new();
    let snapshot = TextVersion::new(content);
    let input = snapshot.analysis();
    let lexical = [
        "anchors",
        "block-scalar-chomping",
        "braces",
        "brackets",
        "colons",
        "commas",
        "comments",
        "comments-indentation",
        "hyphens",
        "indentation",
        "line-length",
        "trailing-spaces",
        "empty-lines",
        "key-ordering",
    ]
    .iter()
    .any(|rule| {
        cfg.rule_level(rule).is_some() && !cfg.is_rule_ignored(rule, path, base_dir)
    });
    let (facts, tokens) = if lexical {
        ParsedFacts::parse_observed(content, 0)
    } else {
        (ParsedFacts::parse(content, 0), None)
    };
    if let Some(syntax) = syntax_diagnostic(content, facts.strict_error()) {
        let repairable = facts.strict_error().is_some_and(|error| {
            matches!(
                error.kind(),
                ryl_yaml_parser::ErrorKind::InvalidIndentation
                    | ryl_yaml_parser::ErrorKind::PlainScalarStartsWithIndicatorBeforeFlowIndicator {
                        indicator: '?'
                    }
            )
        });
        return Err((syntax, repairable));
    }
    if facts.valid {
        input.seed_facts(facts);
        if let Some(tokens) = tokens {
            input.seed_tokens(tokens);
        }
    }
    collect_layout_diagnostics(&mut diagnostics, input, cfg, path, base_dir);
    collect_value_diagnostics(&mut diagnostics, input, cfg, path, base_dir);
    collect_block_diagnostics(&mut diagnostics, input, cfg, path, base_dir);

    let per_line = cfg.per_line_applies(path);
    let directives =
        crate::directives::Directives::parse_per_line_with_analysis(input, &per_line);
    diagnostics.retain(|problem| {
        !problem
            .rule
            .is_some_and(|rule| directives.is_disabled(rule, problem.line))
    });

    if let Some(warning) = higher_minor_version_warning(content) {
        diagnostics.push(warning);
    }
    diagnostics.sort_by_key(|problem| (problem.line, problem.column, problem.rule));

    Ok(diagnostics)
}

pub(crate) fn scan(
    content: &str,
    strict_indentation: bool,
) -> Result<(), ryl_yaml_parser::ScanError> {
    let mut parser = ryl_yaml_parser::Parser::new_from_str_with_options(
        content,
        ryl_yaml_parser::options! { strict_indentation: strict_indentation },
    );
    let mut sink = NullSink;
    parser.load(&mut sink, true)
}

fn syntax_problem(err: &ryl_yaml_parser::ScanError) -> LintProblem {
    let marker = err.marker();
    LintProblem {
        line: marker.line(),
        column: marker.col() + 1,
        level: Severity::Error,
        message: format!("syntax error: {} (syntax)", err.info()),
        rule: None,
    }
}

/// `content`'s relaxed parse error and the error a strict parse stops at, from one parse.
pub(crate) fn parse_errors(
    content: &str,
) -> (Option<LintProblem>, Option<LintProblem>) {
    if let Some(problem) = unsupported_version_error(content) {
        return (Some(problem.clone()), Some(problem));
    }
    let mut parser = ryl_yaml_parser::Parser::new_from_str(content);
    let relaxed = parser.load(&mut NullSink, true).err();
    let strict = parser.strict_indentation_error().or(relaxed.as_ref());
    (
        relaxed.as_ref().map(syntax_problem),
        strict.map(syntax_problem),
    )
}

pub(crate) fn strict_parse_error(content: &str) -> Option<LintProblem> {
    unsupported_version_error(content)
        .or_else(|| scan(content, true).err().as_ref().map(syntax_problem))
}

pub(crate) fn parse_error_from_result(
    content: &str,
    error: Option<&ryl_yaml_parser::ScanError>,
) -> Option<LintProblem> {
    unsupported_version_error(content).or_else(|| error.map(syntax_problem))
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
/// cleanly. Suppresses the parser's undefined-alias error (ryl reports that via the
/// `anchors` rule, matching yamllint).
fn syntax_diagnostic(
    content: &str,
    error: Option<&ryl_yaml_parser::ScanError>,
) -> Option<LintProblem> {
    if let Some(problem) = unsupported_version_error(content) {
        return Some(problem);
    }
    match error {
        None => None,
        Some(err)
            if matches!(err.kind(), ryl_yaml_parser::ErrorKind::UnknownAnchor) =>
        {
            // Scanning still finds lexical errors after an undefined alias halts parsing.
            scanner_error(content).map(|err| syntax_problem(&err))
        }
        Some(err) => Some(syntax_problem(err)),
    }
}

/// [`lint_str`], naming the rewrites that repair a syntax error ryl can repair.
#[must_use]
pub fn lint_str_with_repair_help(
    content: &str,
    path: &Path,
    cfg: &YamlLintConfig,
    base_dir: &Path,
) -> Vec<LintProblem> {
    let mut problem = match lint_unless_syntax_error(content, path, cfg, base_dir) {
        Ok(problems) => return problems,
        Err((problem, false)) => return vec![problem],
        Err((problem, true)) => problem,
    };
    let repairable = |rewrite| {
        rewrite_str(content, cfg, path, base_dir, SourceKind::Yaml, rewrite)
            .0
            .is_some_and(|fixed| strict_parse_error(&fixed).is_none())
    };
    let fix = repairable(Rewrite::Fix);
    let format = repairable(Rewrite::Format);
    let commands = [
        "",
        "ryl format",
        "ryl check --fix",
        "ryl check --fix or ryl format",
    ][usize::from(fix) * 2 + usize::from(format)];
    if !commands.is_empty() {
        problem.message.push_str("; help: ");
        problem.message.push_str(commands);
        problem.message.push_str(" can repair it");
    }
    vec![problem]
}

/// The first lexical scan error in `content`, or `None`. [`syntax_diagnostic`] uses it to
/// find a malformed-token error the parser cannot reach, having halted on an earlier
/// (tolerated) undefined alias.
fn scanner_error(content: &str) -> Option<ryl_yaml_parser::ScanError> {
    ryl_yaml_parser::Scanner::with_options(
        ryl_yaml_parser::StrInput::new(content),
        ryl_yaml_parser::options! { strict_indentation: true },
    )
    .find_map(Result::err)
}
