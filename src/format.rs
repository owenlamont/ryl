use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::num::NonZeroU16;
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
/// the shebang and blank-line run, which `conflicts` adds per the `[format]` table.
const CONFLICT_PROBE: &str = "# lead\nkey: value  # note\n'a: b': 'c'\nplain: 'x'\n\
    necessary: 'a: b'\n\
    escape: \"tab\\there\"\napostrophe: \"it's: x\"\nquote: 'say \"hi\": x'\n\
    flow: {a: 1, b: [1, 2]}\nempty: {}\nnone: []\nblock: |\n  text\nlist:\n- item\npairs:\n- k: v\n";

impl Passes<'static> {
    fn format(cfg: &YamlLintConfig, layout: Layout, skip: &[&str]) -> Self {
        let table = cfg.format().targets();
        let on = |rule| !skip.contains(&rule);
        let line_ending = layout.line_ending;
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
            indentation: on(indentation::ID)
                .then(|| indentation_target(cfg, layout.indent)),
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
                    indent: layout.indent,
                }
            }),
            collection_style: Some(collection_style_config(cfg, layout.indent, skip)),
            per_line: Vec::new(),
        }
    }
}

fn collection_style_config(
    cfg: &YamlLintConfig,
    indent: u8,
    skip: &[&str],
) -> collection_style::Config {
    let table = cfg.format().targets();
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
        indent,
        width: line_length(cfg),
    }
}

/// Each collection `ryl format` would restyle under `cfg` but leaves alone, as a notice.
#[must_use]
pub fn refusals(content: &str, cfg: &YamlLintConfig) -> Vec<LintProblem> {
    refusals_at(content, cfg, file_indent_width(cfg, content))
}

fn refusals_at(content: &str, cfg: &YamlLintConfig, indent: u8) -> Vec<LintProblem> {
    collection_style::findings(content, collection_style_config(cfg, indent, &[]))
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
    format_tracked(
        input,
        cfg,
        Layout::of(cfg, input, file_indent_width(cfg, input)),
        path,
        skip,
        &mut Vec::new(),
    )
}

/// The `[format]` table resolved once, with the keys the config set explicitly.
#[derive(Debug, Clone, Default)]
pub struct FormatSettings {
    targets: FormatTable,
    explicit: BTreeSet<String>,
}

impl FormatSettings {
    pub(crate) const fn new(targets: FormatTable, explicit: BTreeSet<String>) -> Self {
        Self { targets, explicit }
    }

    #[must_use]
    pub const fn targets(&self) -> &FormatTable {
        &self.targets
    }

    /// Whether the config set the `[format]` key `key` (kebab-case) itself.
    #[must_use]
    pub fn is_explicit(&self, key: &str) -> bool {
        self.explicit.contains(key)
    }
}

/// The indent width `ryl format` targets without a top-level `indent-width` where a file
/// shows no width of its own.
pub const DEFAULT_INDENT_WIDTH: u8 = 2;

/// The line length `ryl format` targets without a top-level `line-length`.
pub const DEFAULT_LINE_LENGTH: u16 = 80;

/// The widths `ryl format` weighs for a file without a top-level `indent-width`.
const DETECTED_WIDTHS: [u8; 7] = [2, 3, 4, 5, 6, 7, 8];

/// The line length `ryl format` targets: the top-level `line-length`, else
/// [`DEFAULT_LINE_LENGTH`].
#[must_use]
pub fn line_length(cfg: &YamlLintConfig) -> u16 {
    cfg.line_length()
        .map_or(DEFAULT_LINE_LENGTH, NonZeroU16::get)
}

/// The indent width `ryl format` targets in `content`: the top-level `indent-width`, else
/// the one width of [`DETECTED_WIDTHS`] whose re-indent changes the fewest lines, else
/// [`DEFAULT_INDENT_WIDTH`].
#[must_use]
pub fn file_indent_width(cfg: &YamlLintConfig, content: &str) -> u8 {
    if let Some(width) = cfg.indent_width() {
        return width.get();
    }
    let before: Vec<&str> = content.split_inclusive('\n').collect();
    let widths = DETECTED_WIDTHS.map(usize::from);
    let moved: Vec<(u8, usize)> = DETECTED_WIDTHS
        .into_iter()
        .zip(indentation::reindent_widths(
            content,
            &indentation_target(cfg, DEFAULT_INDENT_WIDTH),
            &widths,
        ))
        .map(|(width, reindented)| {
            let text = settled(reindented.text, content, cfg, width);
            let after: Vec<&str> = text.split_inclusive('\n').collect();
            let changed = TextDiff::from_slices(&before, &after)
                .ops()
                .iter()
                .filter(|op| op.tag() != DiffTag::Equal)
                .map(|op| op.old_range().len().max(op.new_range().len()))
                .sum();
            (width, changed)
        })
        .collect();
    let fewest = moved.iter().map(|&(_, count)| count).min();
    match moved
        .iter()
        .filter(|&&(_, count)| Some(count) == fewest)
        .collect::<Vec<_>>()[..]
    {
        [&(width, _)] => width,
        _ => DEFAULT_INDENT_WIDTH,
    }
}

/// `text`, one re-indent of `input` at `width`, re-indented again until it settles, as the
/// format pipeline does, since one pass can enable another.
fn settled(mut text: String, input: &str, cfg: &YamlLintConfig, width: u8) -> String {
    let mut previous = input.to_string();
    for _ in 0..FIX_PIPELINE_MAX_PASSES {
        if text == previous {
            break;
        }
        let next = indentation::reindent(&text, &indentation_target(cfg, width)).text;
        previous = std::mem::replace(&mut text, next);
    }
    text
}

/// What `ryl format` targets in one file where the config can leave it to the file.
#[derive(Debug, Clone, Copy)]
struct Layout {
    indent: u8,
    line_ending: new_lines::Config,
}

impl Layout {
    fn of(cfg: &YamlLintConfig, content: &str, indent: u8) -> Self {
        let kind = match cfg.format().targets().line_ending {
            LineEndingTarget::CrLf => new_lines::LineKind::Dos,
            LineEndingTarget::Native => new_lines::LineKind::Platform,
            LineEndingTarget::Auto
                if 2 * content.matches("\r\n").count()
                    > content.matches('\n').count() =>
            {
                new_lines::LineKind::Dos
            }
            LineEndingTarget::Lf | LineEndingTarget::Auto => new_lines::LineKind::Unix,
        };
        Self {
            indent,
            line_ending: new_lines::Config { kind },
        }
    }
}

fn format_tracked(
    input: &str,
    cfg: &YamlLintConfig,
    layout: Layout,
    path: &Path,
    skip: &[&str],
    edited: &mut Vec<&'static str>,
) -> String {
    run_passes(
        input,
        &Passes::format(cfg, layout, skip),
        path,
        FIX_PIPELINE_MAX_PASSES,
        &mut std::io::stderr(),
        edited,
    )
}

/// Formatter output, edited rules and indent width by input text, so a preview that both
/// diffs and explains a file formats and detects each region once. Only valid for one
/// `cfg`, `path` and `skip`.
#[derive(Default)]
pub(crate) struct FormatCache {
    formatted: HashMap<String, (String, Vec<&'static str>)>,
    widths: HashMap<String, u8>,
}

impl FormatCache {
    pub(crate) fn indent_width(&mut self, cfg: &YamlLintConfig, input: &str) -> u8 {
        *self
            .widths
            .entry(input.to_string())
            .or_insert_with(|| file_indent_width(cfg, input))
    }

    pub(crate) fn format(
        &mut self,
        input: &str,
        cfg: &YamlLintConfig,
        path: &Path,
        skip: &[&str],
    ) -> &(String, Vec<&'static str>) {
        let layout = Layout::of(cfg, input, self.indent_width(cfg, input));
        self.formatted.entry(input.to_string()).or_insert_with(|| {
            let mut edited = Vec::new();
            let formatted = format_tracked(input, cfg, layout, path, skip, &mut edited);
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
    let layout = Layout::of(cfg, content, cache.indent_width(cfg, content));
    let (formatted, edited) = cache.format(content, cfg, path, skip);
    if formatted == content {
        return Vec::new();
    }
    let mut changed: BTreeMap<&str, BTreeSet<usize>> = BTreeMap::new();
    let mut problems: Vec<LintProblem> =
        checks(content, &Passes::format(cfg, layout, skip))
            .into_iter()
            .filter(|problem| {
                problem.rule.is_some_and(|rule| {
                    changed
                        .entry(rule)
                        .or_insert_with(|| {
                            lines_changed_by(rule, content, cfg, layout, path)
                        })
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
    layout: Layout,
    path: &Path,
) -> BTreeSet<usize> {
    let others: Vec<&str> = FORMAT_RULE_IDS
        .into_iter()
        .filter(|other| *other != rule)
        .collect();
    let alone = format_tracked(content, cfg, layout, path, &others, &mut Vec::new());
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
fn indentation_target(cfg: &YamlLintConfig, width: u8) -> indentation::Config {
    indentation::Config::new(
        indentation::SpacesSetting::Fixed(usize::from(width)),
        if cfg.format().targets().indent_sequences {
            indentation::IndentSequencesSetting::True
        } else {
            indentation::IndentSequencesSetting::False
        },
        false,
    )
    .with_dash_on_own_line(cfg.format().targets().dash_on_own_line)
}

/// Each document `ryl format` leaves un-re-indented in `content` for a reason other than an
/// inline directive, at its first line, each `-` a comment keeps its mapping beside, and
/// each collection-style refusal.
pub(crate) fn unfixed(
    content: &str,
    cfg: &YamlLintConfig,
    cache: &mut FormatCache,
) -> Vec<LintProblem> {
    let indent = cache.indent_width(cfg, content);
    let reindented = indentation::reindent(content, &indentation_target(cfg, indent));
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
    problems.extend(refusals_at(content, cfg, indent));
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
            crate::lint::emit_problems!(
                problems,
                $rule,
                $hits,
                Severity::Error,
                $message
            )
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
    let table = cfg.format().targets();
    let blanks = "\n".repeat(usize::from(table.max_blank_lines) + 1);
    let shebang = match table.comment_starting_space {
        MarkerTarget::Add => "#!probe\n",
        MarkerTarget::Preserve => "",
    };
    let probe = format!("{shebang}{CONFLICT_PROBE}{blanks}last: 1\n");
    let formatted = run_passes(
        &probe,
        &Passes::format(
            cfg,
            Layout::of(cfg, &probe, file_indent_width(cfg, &probe)),
            &[],
        ),
        Path::new(""),
        FIX_PIPELINE_MAX_PASSES,
        &mut std::io::sink(),
        &mut Vec::new(),
    );
    let preserved_forbid = |problem: &LintProblem| match problem.rule {
        Some(braces::ID) => {
            table.mapping_style == CollectionStyleTarget::Preserve
                && problem.message == braces::FORBID_MESSAGE
        }
        Some(brackets::ID) => {
            table.sequence_style == CollectionStyleTarget::Preserve
                && problem.message == brackets::FORBID_MESSAGE
        }
        _ => false,
    };
    let mut rejected: BTreeSet<&str> =
        lint_str(&formatted, Path::new(""), cfg, Path::new(""))
            .into_iter()
            .filter(|problem| !preserved_forbid(problem))
            .filter_map(|problem| problem.rule)
            .collect();
    // No fixed probe can match an arbitrary `extra-required` pattern.
    if cfg.rule_level(quoted_strings::ID).is_some()
        && quoted_strings::Config::resolve(cfg).has_extra_required()
    {
        rejected.insert(quoted_strings::ID);
    }
    // The probe is one file, so it shows one width and one line ending of the several a
    // detected target can write.
    if cfg.rule_level(indentation::ID).is_some() && !spaces_admit_format(cfg) {
        rejected.insert(indentation::ID);
    }
    if cfg.rule_level(new_lines::ID).is_some()
        && table.line_ending == LineEndingTarget::Auto
    {
        rejected.insert(new_lines::ID);
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
                    remedy(rule, cfg)
                )
            })
        })
        .collect()
}

/// Whether `[lint.rules.indentation] spaces` accepts each width `ryl format` can write.
fn spaces_admit_format(cfg: &YamlLintConfig) -> bool {
    let lint = indentation::Config::resolve(cfg);
    match cfg.indent_width() {
        Some(width) => lint.admits_width(width.get()),
        None => DETECTED_WIDTHS
            .into_iter()
            .all(|width| lint.admits_width(width)),
    }
}

fn remedy(rule: &str, cfg: &YamlLintConfig) -> &'static str {
    match (rule, cfg.format().targets().quote_style) {
        (quoted_strings::ID, QuoteStyleTarget::Double) => {
            "set its options `quote-type = \"double\"`, `required = \"only-when-needed\"`, \
             `allow-quoted-quotes = true`"
        }
        (quoted_strings::ID, _) => {
            "set its options `quote-type = \"single\"`, `required = \"only-when-needed\"`, \
             `allow-double-quotes-for-escaping = true`, `allow-quoted-quotes = true`"
        }
        (indentation::ID, _) if cfg.indent_width().is_none() => {
            "set the top-level `indent-width` to its `spaces`"
        }
        (new_lines::ID, _) => {
            "set `[format] line-ending` to the ending its `type` names"
        }
        _ => "change its options to accept the formatter's output",
    }
}

/// How a warning names the formatter's target for `rule`, or `None` where the `[format]`
/// table leaves that rule's concern alone.
fn target(rule: &str, cfg: &YamlLintConfig) -> Option<String> {
    let table = cfg.format().targets();
    let key = match rule {
        indentation::ID if !spaces_admit_format(cfg) => {
            return Some(cfg.indent_width().map_or_else(
                || "per-file indent width (`indent-width` unset)".to_string(),
                |width| format!("`indent-width = {width}`"),
            ));
        }
        hyphens::ID
            if !table.dash_on_own_line
                && cfg.rule_option_bool(hyphens::ID, "dash-on-own-line", false) =>
        {
            Some("dash-on-own-line")
        }
        comments::ID
            if table.comment_starting_space == MarkerTarget::Add
                && !cfg.rule_option_bool(comments::ID, "ignore-shebangs", true) =>
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
