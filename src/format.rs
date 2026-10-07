use std::collections::{BTreeMap, BTreeSet};
use std::num::{NonZeroU8, NonZeroU16};
use std::path::Path;

use similar::{DiffTag, TextDiff};

use crate::config::{SourceKind, YamlLintConfig};
use crate::config_schema::{
    FormatTable, LineEndingTarget, MarkerTarget, QuoteStyleTarget,
};
use crate::directives::Directives;
use crate::fix::{
    FIX_PIPELINE_MAX_PASSES, NewlinePolicy, Passes, region_prefix, run_passes,
    suppressed_rules,
};
use crate::lint::{LintProblem, Severity, lint_str};
use crate::markdown_embed::markdown_region_problems;
use crate::rules::braces::Forbid;
use crate::rules::{
    braces, brackets, colons, commas, comments, comments_indentation, document_end,
    document_start, empty_lines, hyphens, line_length, new_line_at_end_of_file,
    new_lines, quoted_strings, trailing_spaces,
};

/// The rules `ryl format` applies, whatever the lint config enables.
pub const FORMAT_RULE_IDS: [&str; 15] = [
    new_lines::ID,
    comments::ID,
    comments_indentation::ID,
    commas::ID,
    braces::ID,
    brackets::ID,
    colons::ID,
    hyphens::ID,
    new_line_at_end_of_file::ID,
    quoted_strings::ID,
    trailing_spaces::ID,
    document_start::ID,
    document_end::ID,
    empty_lines::ID,
    line_length::ID,
];

/// Formatter output for [`conflicts`] to lint: one instance of each target's concern.
const CONFLICT_PROBE: &str = "# lead\nkey: value  # note\n'a: b': 'c'\nplain: 'x'\n\
    necessary: 'a: b'\n\
    escape: \"tab\\there\"\napostrophe: \"it's: x\"\nquote: 'say \"hi\": x'\n\
    flow: {a: 1, b: [1, 2]}\nempty: {}\nnone: []\nlist:\n- item\n\n\nlast: 1\n";

impl Passes<'static> {
    fn format(cfg: &YamlLintConfig, skip: &[&str]) -> Self {
        let table = cfg.format();
        let on = |rule| !skip.contains(&rule);
        let line_ending = new_lines::Config {
            kind: match table.line_ending {
                LineEndingTarget::Lf => new_lines::LineKind::Unix,
                LineEndingTarget::CrLf => new_lines::LineKind::Dos,
                LineEndingTarget::Native => new_lines::LineKind::Platform,
            },
        };
        let brace_padding = i64::from(table.brace_spacing);
        let quote_style = match table.quote_style {
            QuoteStyleTarget::Single => Some(quoted_strings::QuoteStyle::Single),
            QuoteStyleTarget::Double => Some(quoted_strings::QuoteStyle::Double),
            QuoteStyleTarget::Preserve => None,
        };
        Self {
            new_lines: on(new_lines::ID).then_some(line_ending),
            comments: on(comments::ID).then_some(comments::Config::exact_gap(2)),
            comments_indentation: on(comments_indentation::ID)
                .then_some(comments_indentation::Config::new(false)),
            commas: on(commas::ID).then_some(commas::Config::new(0, 1, 1)),
            braces: on(braces::ID).then_some(braces::Config::new(
                Forbid::None,
                brace_padding,
                brace_padding,
                0,
                0,
            )),
            brackets: on(brackets::ID).then_some(brackets::Config::new(
                Forbid::None,
                0,
                0,
                -1,
                -1,
            )),
            colons: on(colons::ID).then_some(colons::Config::format()),
            hyphens: on(hyphens::ID).then_some(hyphens::Config::format()),
            final_newline: on(new_line_at_end_of_file::ID).then(|| {
                NewlinePolicy::Configured(
                    new_lines::expected_newline(
                        line_ending,
                        new_lines::platform_newline(),
                    )
                    .into_owned(),
                )
            }),
            quoted_strings: quote_style
                .filter(|_| on(quoted_strings::ID))
                .map(quoted_strings::Config::ladder),
            trailing_spaces: on(trailing_spaces::ID),
            document_start: (table.document_start == MarkerTarget::Add
                && on(document_start::ID))
            .then_some(document_start::Config::new(true)),
            document_end: (table.document_end == MarkerTarget::Add
                && on(document_end::ID))
            .then_some(document_end::Config::new(true)),
            empty_lines: on(empty_lines::ID)
                .then_some(empty_lines::Config::new(2, 0, 0)),
            truthy: None,
            key_ordering: None,
            line_length: (table.fold_long_lines && on(line_length::ID)).then(|| {
                line_length::Fold {
                    width: line_length(cfg),
                    indent: indent_width(cfg),
                }
            }),
            per_line: Vec::new(),
        }
    }
}

/// `input` formatted per `cfg`'s `[format]` table, skipping any rule in `skip`.
#[must_use]
pub fn format_str(
    input: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    skip: &[&str],
) -> String {
    format_tracked(input, cfg, path, skip, &mut Vec::new())
}

/// The indent width `ryl format` targets: the top-level `indent-width`, else 2.
#[must_use]
pub fn indent_width(cfg: &YamlLintConfig) -> u8 {
    cfg.indent_width().map_or(2, NonZeroU8::get)
}

/// The line length `ryl format` targets: the top-level `line-length`, else 80.
#[must_use]
pub fn line_length(cfg: &YamlLintConfig) -> u16 {
    cfg.line_length().map_or(80, NonZeroU16::get)
}

fn format_tracked(
    input: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    skip: &[&str],
    edited: &mut Vec<&'static str>,
) -> String {
    run_passes(
        input,
        &Passes::format(cfg, skip),
        path,
        FIX_PIPELINE_MAX_PASSES,
        &mut std::io::stderr(),
        edited,
    )
}

/// Why `ryl format` would rewrite `content`: the diagnostics of each formatting rule that
/// would edit it, or a `would reformat` line for a rule that edits what its check misses.
#[must_use]
pub fn problems(
    content: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    kind: SourceKind,
) -> Vec<LintProblem> {
    match kind {
        SourceKind::Yaml => region_problems(content, cfg, path, &[]),
        SourceKind::Markdown => markdown_region_problems(content, cfg, |region| {
            if region_prefix(content, region).is_none() {
                return Vec::new();
            }
            region_problems(&region.content, cfg, path, suppressed_rules())
        }),
    }
}

fn region_problems(
    content: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    skip: &[&str],
) -> Vec<LintProblem> {
    let mut edited = Vec::new();
    if format_tracked(content, cfg, path, skip, &mut edited) == content {
        return Vec::new();
    }
    let mut changed: BTreeMap<&str, BTreeSet<usize>> = BTreeMap::new();
    let mut problems: Vec<LintProblem> = checks(content, &Passes::format(cfg, skip))
        .into_iter()
        .filter(|problem| {
            problem.rule.is_some_and(|rule| {
                changed
                    .entry(rule)
                    .or_insert_with(|| lines_changed_by(rule, content, cfg, path))
                    .contains(&problem.line)
            })
        })
        .collect();
    let unexplained: BTreeSet<&'static str> = edited
        .into_iter()
        .filter(|rule| !problems.iter().any(|problem| problem.rule == Some(rule)))
        .collect();
    problems.extend(unexplained.into_iter().map(|rule| LintProblem {
        line: 1,
        column: 1,
        level: Severity::Error,
        message: "would reformat".to_string(),
        rule: Some(rule),
    }));
    problems.sort_by_key(|problem| (problem.line, problem.column));
    problems
}

/// The 1-based lines of `content` that `rule`'s fix alone rewrites, an insertion counting
/// against the line before it (the first line at the start).
fn lines_changed_by(
    rule: &str,
    content: &str,
    cfg: &YamlLintConfig,
    path: &Path,
) -> BTreeSet<usize> {
    let others: Vec<&str> = FORMAT_RULE_IDS
        .into_iter()
        .filter(|other| *other != rule)
        .collect();
    let alone = format_tracked(content, cfg, path, &others, &mut Vec::new());
    let before: Vec<&str> = content.split_inclusive('\n').collect();
    let after: Vec<&str> = alone.split_inclusive('\n').collect();
    TextDiff::from_slices(&before, &after)
        .ops()
        .iter()
        .filter(|op| op.tag() != DiffTag::Equal)
        .flat_map(|op| {
            let lines = op.old_range();
            let first = if lines.is_empty() {
                lines.start.max(1)
            } else {
                lines.start + 1
            };
            first..=lines.end.max(first)
        })
        .collect()
}

/// Each `colons` and `hyphens` finding `ryl format` leaves in `content`, with why.
pub(crate) fn unfixed(content: &str) -> Vec<LintProblem> {
    let directives = Directives::parse(content);
    let colons = colons::unfixed(content, &colons::Config::format())
        .into_iter()
        .map(|hit| (colons::ID, hit.line, hit.column, hit.message));
    let hyphens = hyphens::unfixed(content, &hyphens::Config::format())
        .into_iter()
        .map(|hit| (hyphens::ID, hit.line, hit.column, hit.message));
    let mut problems: Vec<LintProblem> = colons
        .chain(hyphens)
        .filter(|&(rule, line, ..)| !directives.is_disabled(rule, line))
        .map(|(rule, line, column, message)| LintProblem {
            line,
            column,
            level: Severity::Error,
            message: format!(
                "{message}; respacing would re-indent the block collection after it"
            ),
            rule: Some(rule),
        })
        .collect();
    problems.sort_by_key(|problem| (problem.line, problem.column));
    problems
}

fn checks(content: &str, passes: &Passes) -> Vec<LintProblem> {
    let mut problems = Vec::new();
    macro_rules! report {
        ($rule:ident) => {
            report!(
                $rule,
                passes
                    .$rule
                    .iter()
                    .flat_map(|cfg| $rule::check(content, cfg))
            )
        };
        ($rule:ident, $hits:expr) => {
            report!($rule, $hits, |hit: $rule::Violation| hit.message)
        };
        ($rule:ident, $hits:expr, $message:expr) => {
            problems.extend($hits.into_iter().map(|hit| LintProblem {
                line: hit.line,
                column: hit.column,
                level: Severity::Error,
                message: $message(hit),
                rule: Some($rule::ID),
            }))
        };
    }
    let platform = new_lines::platform_newline();
    report!(
        new_lines,
        passes
            .new_lines
            .iter()
            .filter_map(|cfg| new_lines::check(content, *cfg, platform))
    );
    report!(comments);
    report!(
        comments_indentation,
        passes
            .comments_indentation
            .iter()
            .flat_map(|cfg| comments_indentation::check(content, cfg)),
        |_| comments_indentation::MESSAGE.to_string()
    );
    report!(commas);
    report!(braces);
    report!(brackets);
    report!(colons);
    report!(hyphens);
    report!(
        new_line_at_end_of_file,
        passes
            .final_newline
            .iter()
            .filter_map(|_| new_line_at_end_of_file::check(content)),
        |_| new_line_at_end_of_file::MESSAGE.to_string()
    );
    report!(quoted_strings);
    report!(
        trailing_spaces,
        passes
            .trailing_spaces
            .then(|| trailing_spaces::check(content))
            .into_iter()
            .flatten(),
        |_| trailing_spaces::MESSAGE.to_string()
    );
    report!(document_start);
    report!(document_end);
    report!(empty_lines);
    report!(
        line_length,
        passes
            .line_length
            .iter()
            .flat_map(|fold| line_length::check(content, &fold.check_config()))
    );
    problems
}

/// One warning per enabled formatting rule whose lint options reject what `ryl format`
/// writes under `cfg`'s `[format]` table.
#[must_use]
pub fn conflicts(cfg: &YamlLintConfig) -> Vec<String> {
    let table = cfg.format();
    let formatted = run_passes(
        CONFLICT_PROBE,
        &Passes::format(cfg, &[]),
        Path::new(""),
        FIX_PIPELINE_MAX_PASSES,
        &mut std::io::sink(),
        &mut Vec::new(),
    );
    let mut rejected: BTreeSet<&str> =
        lint_str(&formatted, Path::new(""), cfg, Path::new(""))
            .into_iter()
            .filter_map(|problem| problem.rule)
            .collect();
    // No fixed probe can match an arbitrary `extra-required` pattern.
    if cfg.rule_level(quoted_strings::ID).is_some()
        && quoted_strings::Config::resolve(cfg).has_extra_required()
    {
        rejected.insert(quoted_strings::ID);
    }
    FORMAT_RULE_IDS
        .into_iter()
        .filter(|rule| rejected.contains(rule))
        .filter_map(|rule| {
            target(rule, table).map(|target| {
                format!(
                    "the {rule} lint rule's options are incompatible with the \
                     formatter's {target}. Disable {rule} when using `ryl format`, or \
                     {}.",
                    remedy(rule, table)
                )
            })
        })
        .collect()
}

fn remedy(rule: &str, table: &FormatTable) -> &'static str {
    match (rule, table.quote_style) {
        (quoted_strings::ID, QuoteStyleTarget::Double) => {
            "set its options `quote-type = \"double\"`, `required = \"only-when-needed\"`, \
             `allow-quoted-quotes = true`"
        }
        (quoted_strings::ID, _) => {
            "set its options `quote-type = \"single\"`, `required = \"only-when-needed\"`, \
             `allow-double-quotes-for-escaping = true`, `allow-quoted-quotes = true`"
        }
        _ => "change its options to accept the formatter's output",
    }
}

/// How a warning names the formatter's target for `rule`, or `None` where `table` leaves
/// that rule's concern alone.
fn target(rule: &str, table: &FormatTable) -> Option<String> {
    let key = match rule {
        quoted_strings::ID if table.quote_style == QuoteStyleTarget::Preserve => None,
        document_start::ID if table.document_start == MarkerTarget::Preserve => None,
        document_end::ID if table.document_end == MarkerTarget::Preserve => None,
        line_length::ID => return None,
        quoted_strings::ID => Some("quote-style"),
        new_lines::ID => Some("line-ending"),
        braces::ID => Some("brace-spacing"),
        document_start::ID | document_end::ID => Some(rule),
        _ => return Some(format!("built-in {rule} style")),
    }?;
    let values = toml::Value::try_from(table).expect("the [format] table serializes");
    Some(format!("`[format] {key} = {}`", values[key]))
}
