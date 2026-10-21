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
    FIX_PIPELINE_MAX_PASSES, NewlinePolicy, ParsedInput, PassTracking, Passes,
    ReindentCache, region_prefix, rewrite_parse_gate, run_passes,
    run_passes_with_facts, suppressed_rules,
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
use crate::text_version::{Analysis, TextVersion};

pub(crate) mod layout;

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
    fn format(cfg: &YamlLintConfig, content: &str, skip: &[&str]) -> Self {
        let table = cfg.format().targets();
        let on = |rule| !skip.contains(&rule);
        let line_ending = line_ending(cfg, content);
        let indent = indent_width(cfg);
        let brace_padding = i64::from(table.brace_spacing);
        let quote_style = match table.quote_style {
            QuoteStyleTarget::Single => Some(quoted_strings::QuoteStyle::Single),
            QuoteStyleTarget::Double => Some(quoted_strings::QuoteStyle::Double),
            QuoteStyleTarget::Preserve => None,
        };
        let indentation = on(indentation::ID).then(|| {
            let target = indentation_target(cfg, indent);
            if cfg.indent_width().is_none() {
                target.requiring_consistent()
            } else {
                target
            }
        });
        Self {
            preserve_events: true,
            new_lines: on(new_lines::ID).then_some(line_ending),
            comments: on(comments::ID).then_some(comments::Config::exact_gap(
                usize::from(table.comment_spacing.get()),
                table.comment_starting_space == MarkerTarget::Add,
            )),
            indentation,
            repair_indentation: Some(
                indentation.unwrap_or_else(|| indentation_target(cfg, indent)),
            ),
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
                    indent,
                }
            }),
            collection_style: Some(collection_style_config(cfg, indent, skip)).filter(
                |style| {
                    style.sequences != CollectionStyleTarget::Preserve
                        || style.mappings != CollectionStyleTarget::Preserve
                },
            ),
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
        require_consistent_indent: cfg.indent_width().is_none(),
    }
}

/// Each collection `ryl format` would restyle under `cfg` but leaves alone, as a notice.
#[must_use]
pub fn refusals(content: &str, cfg: &YamlLintConfig) -> Vec<LintProblem> {
    refusals_at(content, cfg, indent_width(cfg))
}

fn refusals_at(content: &str, cfg: &YamlLintConfig, indent: u8) -> Vec<LintProblem> {
    refusals_with_analysis(TextVersion::new(content).analysis(), cfg, indent)
}

fn refusals_with_analysis(
    input: Analysis<'_>,
    cfg: &YamlLintConfig,
    indent: u8,
) -> Vec<LintProblem> {
    let content = input.text();
    if let Some(line) =
        crate::rules::block_scalar_chomping::ambiguous_blank_body_line_with_analysis(
            input,
        )
    {
        return vec![LintProblem {
            line,
            column: 1,
            level: Severity::Error,
            message: "cannot format this file safely: block scalar value may change"
                .to_string(),
            rule: Some(indentation::ID),
        }];
    }
    collection_style::findings(
        content,
        collection_style_config(cfg, indent, &[]),
        Some(&indentation_target(cfg, indent)),
    )
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
    let Ok(mut parsed) = rewrite_parse_gate(input, crate::fix::Rewrite::Format) else {
        return input.to_string();
    };
    format_tracked(
        input,
        cfg,
        path,
        skip,
        &mut PassTracking {
            edited: &mut Vec::new(),
            reindented: None,
            strict_input: Some(parsed.strict_error.is_none()),
        },
        Some(&mut parsed),
    )
    .into_text()
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

/// The indent width `ryl format` targets without a top-level `indent-width`.
pub const DEFAULT_INDENT_WIDTH: u8 = 2;

/// The line length `ryl format` targets without a top-level `line-length`.
pub const DEFAULT_LINE_LENGTH: u16 = 80;

/// The line length `ryl format` targets: the top-level `line-length`, else
/// [`DEFAULT_LINE_LENGTH`].
#[must_use]
pub fn line_length(cfg: &YamlLintConfig) -> u16 {
    cfg.line_length()
        .map_or(DEFAULT_LINE_LENGTH, NonZeroU16::get)
}

#[must_use]
pub fn indent_width(cfg: &YamlLintConfig) -> u8 {
    cfg.indent_width()
        .map_or(DEFAULT_INDENT_WIDTH, std::num::NonZeroU8::get)
}

fn line_ending(cfg: &YamlLintConfig, content: &str) -> new_lines::Config {
    let kind = match cfg.format().targets().line_ending {
        LineEndingTarget::CrLf => new_lines::LineKind::Dos,
        LineEndingTarget::Native => new_lines::LineKind::Platform,
        LineEndingTarget::Auto
            if 2 * content.matches("\r\n").count() > content.matches('\n').count() =>
        {
            new_lines::LineKind::Dos
        }
        LineEndingTarget::Lf | LineEndingTarget::Auto => new_lines::LineKind::Unix,
    };
    new_lines::Config { kind }
}

fn format_tracked(
    input: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    skip: &[&str],
    tracking: &mut PassTracking<'_>,
    parsed: Option<&mut ParsedInput>,
) -> TextVersion<'static> {
    let _recording = layout::stats::Recording::start(
        path,
        input.len(),
        parsed.is_some(),
        tracking.strict_input,
    );
    let passes = Passes::format(cfg, input, skip);
    let mut source = TextVersion::new(input);
    if let Some(parsed) = parsed {
        if std::mem::take(&mut parsed.prefetched) {
            layout::stats::prefetched();
        }
        source.analysis().seed_facts(
            parsed
                .facts
                .take()
                .expect("format facts are transferred once"),
        );
        if let Some(tokens) = parsed.tokens.take() {
            source.analysis().seed_tokens(tokens);
        }
    }
    if tracking.strict_input == Some(true) {
        source.mark_strict_valid();
        match layout::format(&mut source, &passes, tracking) {
            Ok(output) => return output,
            Err(reason) => layout::stats::Recording::fallback(&reason),
        }
    } else if tracking.strict_input == Some(false) {
        layout::stats::Recording::fallback(&layout::Fallback::Repair);
    }
    run_passes_with_facts(
        input,
        &passes,
        path,
        FIX_PIPELINE_MAX_PASSES,
        &mut std::io::stderr(),
        tracking,
        source.into_facts(),
    )
}

/// Formatter output and edited rules by input text, so a preview that both diffs and
/// explains a file formats each region once; only valid for one
/// `cfg`, `path` and `skip`.
#[derive(Default)]
pub(crate) struct FormatCache {
    formatted: HashMap<String, (String, Vec<&'static str>)>,
    reindented: ReindentCache,
    snapshot: Option<TextVersion<'static>>,
}

impl FormatCache {
    pub(crate) fn format(
        &mut self,
        input: &str,
        cfg: &YamlLintConfig,
        path: &Path,
        skip: &[&str],
    ) -> &(String, Vec<&'static str>) {
        self.format_with_facts(input, cfg, path, skip, None)
    }

    pub(crate) fn format_with_facts(
        &mut self,
        input: &str,
        cfg: &YamlLintConfig,
        path: &Path,
        skip: &[&str],
        initial: Option<&mut ParsedInput>,
    ) -> &(String, Vec<&'static str>) {
        if self.formatted.contains_key(input) {
            return &self.formatted[input];
        }
        let mut parsed = initial
            .is_none()
            .then(|| rewrite_parse_gate(input, crate::fix::Rewrite::Format));
        let parsed =
            initial.or_else(|| parsed.as_mut().and_then(|result| result.as_mut().ok()));
        let strict_input = parsed.as_ref().map(|parsed| parsed.strict_error.is_none());
        self.formatted.entry(input.to_string()).or_insert_with(|| {
            let mut edited = Vec::new();
            let formatted = format_tracked(
                input,
                cfg,
                path,
                skip,
                &mut PassTracking {
                    edited: &mut edited,
                    reindented: Some(&mut self.reindented),
                    strict_input,
                },
                parsed,
            );
            let text = formatted.to_string();
            self.snapshot = Some(formatted);
            (text, edited)
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
    let mut problems: Vec<LintProblem> =
        checks(content, &Passes::format(cfg, content, skip))
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
    let alone = format_tracked(
        content,
        cfg,
        path,
        &others,
        &mut PassTracking {
            edited: &mut Vec::new(),
            reindented: None,
            strict_input: None,
        },
        None,
    );
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
pub(crate) fn indentation_target(
    cfg: &YamlLintConfig,
    width: u8,
) -> indentation::Config {
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

pub(crate) fn unfixed(
    content: &str,
    cfg: &YamlLintConfig,
    cache: &mut FormatCache,
) -> Vec<LintProblem> {
    let local;
    let input = if let Some(snapshot) = cache
        .snapshot
        .as_ref()
        .filter(|snapshot| snapshot.analysis().text() == content)
    {
        snapshot.analysis()
    } else {
        local = TextVersion::new(content);
        local.analysis()
    };
    let indent = indent_width(cfg);
    let mut target = indentation_target(cfg, indent);
    if cfg.indent_width().is_none() {
        target = target.requiring_consistent();
    }
    let cached = cache
        .reindented
        .as_ref()
        .filter(|result| result.matches(input, target));
    let computed;
    let reindented = if let Some(result) = cached {
        result
    } else {
        let result = indentation::reindent_with_analysis(input, &target).0;
        computed = crate::fix::ReindentEvidence::new(
            input,
            target,
            result.refused,
            result.kept_dash_lines,
        );
        &computed
    };
    let inconsistent = if reindented
        .refused
        .iter()
        .any(|refusal| !matches!(refusal.cause, indentation::Cause::Disabled))
    {
        indentation::check_with_analysis(input, &target.with_consistent_spaces())
    } else {
        Vec::new()
    };
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
    let notice = |line, column| LintProblem {
        line,
        column,
        level: Severity::Error,
        message: "cannot re-indent this document safely".to_string(),
        rule: Some(indentation::ID),
    };
    let mut problems: Vec<LintProblem> = reindented
        .refused
        .iter()
        .filter(|refusal| !matches!(refusal.cause, indentation::Cause::Disabled))
        .flat_map(|refusal| {
            let sites: Vec<_> = inconsistent
                .iter()
                .filter(|violation| refusal.lines.contains(&violation.line))
                .map(|violation| notice(violation.line, violation.column))
                .collect();
            if sites.is_empty() {
                vec![notice(*refusal.lines.start(), 1)]
            } else {
                sites
            }
        })
        .chain(kept)
        .collect();
    problems.extend(refused_spacing(input, cfg, &reindented.refused));
    problems.extend(refusals_with_analysis(input, cfg, indent));
    problems.sort_by_key(|problem| (problem.line, problem.column));
    problems.dedup();
    problems
}

fn refused_spacing(
    input: Analysis<'_>,
    cfg: &YamlLintConfig,
    refused: &[indentation::Refusal],
) -> Vec<LintProblem> {
    if refused
        .iter()
        .all(|refusal| matches!(refusal.cause, indentation::Cause::Disabled))
    {
        return Vec::new();
    }
    let refused_line = |line| {
        refused.iter().any(|refusal| {
            !matches!(refusal.cause, indentation::Cause::Disabled)
                && refusal.lines.contains(&line)
        })
    };
    hyphens::check_with_analysis(input, &hyphens::Config::resolve(cfg))
        .into_iter()
        .map(|violation| {
            (
                violation.line,
                violation.column,
                violation.message,
                hyphens::ID,
            )
        })
        .chain(
            colons::check_with_analysis(input, &colons::Config::resolve(cfg))
                .into_iter()
                .map(|violation| {
                    (
                        violation.line,
                        violation.column,
                        violation.message,
                        colons::ID,
                    )
                }),
        )
        .filter(|(line, _, _, _)| refused_line(*line))
        .map(|(line, column, message, rule)| LintProblem {
            line,
            column,
            level: Severity::Error,
            message,
            rule: Some(rule),
        })
        .collect()
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
            .flat_map(|cfg| {
                collection_style::findings(content, *cfg, passes.indentation.as_ref())
            })
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
    if !cfg.enables_any_rule() {
        return Vec::new();
    }
    let table = cfg.format().targets();
    let blanks = "\n".repeat(usize::from(table.max_blank_lines) + 1);
    let shebang = match table.comment_starting_space {
        MarkerTarget::Add => "#!probe\n",
        MarkerTarget::Preserve => "",
    };
    let probe = format!("{shebang}{CONFLICT_PROBE}{blanks}last: 1\n");
    let formatted = run_passes(
        &probe,
        &Passes::format(cfg, &probe, &[]),
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
    lint.admits_width(indent_width(cfg))
}

fn remedy(rule: &str, cfg: &YamlLintConfig) -> String {
    match (rule, cfg.format().targets().quote_style) {
        (quoted_strings::ID, QuoteStyleTarget::Double) => {
            "set its options `quote-type = \"double\"`, `required = \"only-when-needed\"`, \
             `allow-quoted-quotes = true`".to_owned()
        }
        (quoted_strings::ID, _) => {
            "set its options `quote-type = \"single\"`, `required = \"only-when-needed\"`, \
             `allow-double-quotes-for-escaping = true`, `allow-quoted-quotes = true`".to_owned()
        }
        (indentation::ID, _) if cfg.indent_width().is_none() => {
            "set the top-level TOML `indent-width` to its `spaces` (for a YAML config, follow its migration warning first)".to_owned()
        }
        (new_lines::ID, _) => {
            "set `[format] line-ending` to the ending its `type` names".to_owned()
        }
        _ => "change its options to accept the formatter's output".to_owned(),
    }
}

/// How a warning names the formatter's target for `rule`, or `None` where the `[format]`
/// table leaves that rule's concern alone.
fn target(rule: &str, cfg: &YamlLintConfig) -> Option<String> {
    let table = cfg.format().targets();
    let key = match rule {
        indentation::ID if !spaces_admit_format(cfg) => {
            return Some(format!("`indent-width = {}`", indent_width(cfg)));
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
