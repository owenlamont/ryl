use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::num::{NonZeroU8, NonZeroU16};
use std::path::Path;

use similar::{DiffTag, TextDiff};

use crate::config::{SourceKind, YamlLintConfig};
use crate::config_schema::{
    CollectionStyleTarget, FormatTable, LineEndingTarget, MarkerTarget,
    QuoteStyleTarget,
};
use crate::fix::{
    FIX_PIPELINE_MAX_PASSES, NewlinePolicy, Passes, region_prefix, run_passes,
    suppressed_rules,
};
use crate::lint::{LintProblem, Severity, lint_str};
use crate::markdown_embed::markdown_region_problems;
use crate::rules::braces::Forbid;
use crate::rules::support::collection_style;
use crate::rules::{
    braces, brackets, colons, commas, comments, comments_indentation, document_end,
    document_start, empty_lines, hyphens, indentation, line_length,
    new_line_at_end_of_file, new_lines, quoted_strings, trailing_spaces,
};

/// The rules `ryl format` applies, whatever the lint config enables.
pub const FORMAT_RULE_IDS: [&str; 16] = [
    new_lines::ID,
    comments::ID,
    comments_indentation::ID,
    commas::ID,
    braces::ID,
    brackets::ID,
    colons::ID,
    hyphens::ID,
    indentation::ID,
    new_line_at_end_of_file::ID,
    quoted_strings::ID,
    trailing_spaces::ID,
    document_start::ID,
    document_end::ID,
    empty_lines::ID,
    line_length::ID,
];

/// Formatter input for [`conflicts`] to lint: one instance of each target's concern, bar
/// the blank-line run, which `conflicts` sizes to `max-blank-lines`.
const CONFLICT_PROBE: &str = "#!probe\n# lead\nkey: value  # note\n'a: b': 'c'\nplain: 'x'\n\
    necessary: 'a: b'\n\
    escape: \"tab\\there\"\napostrophe: \"it's: x\"\nquote: 'say \"hi\": x'\n\
    flow: {a: 1, b: [1, 2]}\nempty: {}\nnone: []\nblock: |\n  text\nlist:\n- item\npairs:\n- k: v\n";

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
            comments: on(comments::ID).then_some(comments::Config::exact_gap(
                usize::from(table.comment_spacing.get()),
                table.comment_starting_space == MarkerTarget::Add,
            )),
            indentation: on(indentation::ID).then(|| indentation_target(cfg)),
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
            empty_lines: on(empty_lines::ID).then_some(empty_lines::Config::new(
                i64::from(table.max_blank_lines),
                0,
                0,
            )),
            truthy: None,
            key_ordering: None,
            line_length: (table.fold_long_lines && on(line_length::ID)).then(|| {
                line_length::Fold {
                    width: line_length(cfg),
                    indent: indent_width(cfg),
                }
            }),
            collection_style: Some(collection_style_config(cfg, skip)),
            per_line: Vec::new(),
        }
    }
}

fn collection_style_config(
    cfg: &YamlLintConfig,
    skip: &[&str],
) -> collection_style::Config {
    let table = cfg.format();
    let style = |target, rule| {
        if skip.contains(&rule) {
            CollectionStyleTarget::Preserve
        } else {
            target
        }
    };
    collection_style::Config {
        sequences: style(table.sequence_style, brackets::ID),
        mappings: style(table.mapping_style, braces::ID),
        indent: indent_width(cfg),
        width: line_length(cfg),
    }
}

/// Each collection `ryl format` would restyle under `cfg` but leaves alone, as a notice.
#[must_use]
pub fn refusals(content: &str, cfg: &YamlLintConfig) -> Vec<LintProblem> {
    collection_style::findings(content, collection_style_config(cfg, &[]))
        .into_iter()
        .filter(|finding| finding.refused.is_some())
        .map(collection_style::Finding::into_problem)
        .collect()
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

/// The indent width `ryl format` targets without a top-level `indent-width`.
pub const DEFAULT_INDENT_WIDTH: u8 = 2;

/// The line length `ryl format` targets without a top-level `line-length`.
pub const DEFAULT_LINE_LENGTH: u16 = 80;

/// The indent width `ryl format` targets: the top-level `indent-width`, else
/// [`DEFAULT_INDENT_WIDTH`].
#[must_use]
pub fn indent_width(cfg: &YamlLintConfig) -> u8 {
    cfg.indent_width()
        .map_or(DEFAULT_INDENT_WIDTH, NonZeroU8::get)
}

/// The line length `ryl format` targets: the top-level `line-length`, else
/// [`DEFAULT_LINE_LENGTH`].
#[must_use]
pub fn line_length(cfg: &YamlLintConfig) -> u16 {
    cfg.line_length()
        .map_or(DEFAULT_LINE_LENGTH, NonZeroU16::get)
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

/// Formatter output and edited rules by input text, so a preview that both diffs and
/// explains a file formats each region once. Only valid for one `cfg`, `path` and `skip`.
#[derive(Default)]
pub(crate) struct FormatCache(HashMap<String, (String, Vec<&'static str>)>);

impl FormatCache {
    pub(crate) fn format(
        &mut self,
        input: &str,
        cfg: &YamlLintConfig,
        path: &Path,
        skip: &[&str],
    ) -> &(String, Vec<&'static str>) {
        self.0.entry(input.to_string()).or_insert_with(|| {
            let mut edited = Vec::new();
            let formatted = format_tracked(input, cfg, path, skip, &mut edited);
            (formatted, edited)
        })
    }
}

/// Why `ryl format` would rewrite `content`: the diagnostics of each formatting rule that
/// would edit it, or a `would reformat` line for a rule that edits what its check misses.
pub(crate) fn problems(
    content: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    kind: SourceKind,
    cache: &mut FormatCache,
) -> Vec<LintProblem> {
    match kind {
        SourceKind::Yaml => region_problems(content, cfg, path, &[], cache),
        SourceKind::Markdown => markdown_region_problems(content, cfg, |region| {
            if region_prefix(content, region).is_none() {
                return Vec::new();
            }
            region_problems(&region.content, cfg, path, suppressed_rules(), cache)
        }),
    }
}

fn region_problems(
    content: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    skip: &[&str],
    cache: &mut FormatCache,
) -> Vec<LintProblem> {
    let (formatted, edited) = cache.format(content, cfg, path, skip);
    if formatted == content {
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
        .iter()
        .copied()
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

/// The `indentation` config `ryl format` re-indents to.
fn indentation_target(cfg: &YamlLintConfig) -> indentation::Config {
    indentation::Config::new(
        indentation::SpacesSetting::Fixed(usize::from(indent_width(cfg))),
        if cfg.format().indent_sequences {
            indentation::IndentSequencesSetting::True
        } else {
            indentation::IndentSequencesSetting::False
        },
        false,
    )
    .with_dash_on_own_line(cfg.format().dash_on_own_line)
}

/// Each document `ryl format` leaves un-re-indented in `content` for a reason other than an
/// inline directive, at its first line, each `-` a comment keeps its mapping beside, and
/// each collection-style refusal.
pub(crate) fn unfixed(content: &str, cfg: &YamlLintConfig) -> Vec<LintProblem> {
    let reindented = indentation::reindent(content, &indentation_target(cfg));
    let kept = reindented
        .kept_dash_lines
        .iter()
        .map(|&(line, column)| LintProblem {
            line,
            column,
            level: Severity::Error,
            message:
                "cannot move this mapping below its `-`: a comment follows the `-`"
                    .to_string(),
            rule: Some(hyphens::ID),
        });
    let mut problems: Vec<LintProblem> = reindented
        .refused
        .iter()
        .filter(|refusal| refusal.cause != indentation::Cause::Disabled)
        .map(|refusal| LintProblem {
            line: *refusal.lines.start(),
            column: 1,
            level: Severity::Error,
            message: "cannot re-indent this document safely".to_string(),
            rule: Some(indentation::ID),
        })
        .chain(kept)
        .collect();
    problems.extend(refusals(content, cfg));
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
    problems.extend(
        passes
            .collection_style
            .iter()
            .flat_map(|cfg| collection_style::findings(content, *cfg))
            .filter(|finding| finding.refused.is_none())
            .map(collection_style::Finding::into_problem),
    );
    report!(braces);
    report!(brackets);
    report!(colons);
    report!(hyphens);
    report!(indentation);
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
    let blanks = "\n".repeat(usize::from(table.max_blank_lines) + 1);
    let formatted = run_passes(
        &format!("{CONFLICT_PROBE}{blanks}last: 1\n"),
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
    // A flush-sequence target nests nothing in the probe for `spaces` to measure.
    if cfg.rule_level(indentation::ID).is_some()
        && !indentation::Config::resolve(cfg).admits_width(indent_width(cfg))
    {
        rejected.insert(indentation::ID);
    }
    FORMAT_RULE_IDS
        .into_iter()
        .filter(|rule| rejected.contains(rule))
        .filter_map(|rule| {
            target(rule, cfg).map(|target| {
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

/// How a warning names the formatter's target for `rule`, or `None` where the `[format]`
/// table leaves that rule's concern alone.
fn target(rule: &str, cfg: &YamlLintConfig) -> Option<String> {
    let table = cfg.format();
    let key = match rule {
        indentation::ID
            if !indentation::Config::resolve(cfg).admits_width(indent_width(cfg)) =>
        {
            return Some(format!("`indent-width = {}`", indent_width(cfg)));
        }
        hyphens::ID
            if !table.dash_on_own_line
                && cfg.rule_option_bool(hyphens::ID, "dash-on-own-line", false) =>
        {
            Some("dash-on-own-line")
        }
        indentation::ID
            if cfg.rule_option_bool(
                indentation::ID,
                "check-multi-line-strings",
                false,
            ) =>
        {
            return Some(format!("built-in {rule} style"));
        }
        comments::ID
            if !cfg.rule_option_bool(comments::ID, "ignore-shebangs", true) =>
        {
            return Some(format!("built-in {rule} style"));
        }
        indentation::ID => Some("indent-sequences"),
        quoted_strings::ID if table.quote_style == QuoteStyleTarget::Preserve => None,
        document_start::ID if table.document_start == MarkerTarget::Preserve => None,
        document_end::ID if table.document_end == MarkerTarget::Preserve => None,
        line_length::ID => return None,
        braces::ID if table.mapping_style == CollectionStyleTarget::Flow => {
            Some("mapping-style")
        }
        brackets::ID if table.sequence_style == CollectionStyleTarget::Flow => {
            Some("sequence-style")
        }
        quoted_strings::ID => Some("quote-style"),
        new_lines::ID => Some("line-ending"),
        braces::ID => Some("brace-spacing"),
        comments::ID => Some("comment-spacing"),
        empty_lines::ID => Some("max-blank-lines"),
        document_start::ID | document_end::ID => Some(rule),
        _ => return Some(format!("built-in {rule} style")),
    }?;
    let values = toml::Value::try_from(table).expect("the [format] table serializes");
    Some(format!("`[format] {key} = {}`", values[key]))
}
