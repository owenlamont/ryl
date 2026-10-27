//! `comments-indentation`: a comment must be indented like the content that follows
//! it (else like the content it trails). Mirrors yamllint's `comments-indentation`.
//! Safe `--fix` re-indents the comment; comments carry no YAML structure, so moving
//! one cannot change the parse.
//!
//! The ryl-only, TOML-only `allow-any-open-indent` option (default off) additionally
//! accepts a comment whose indent matches any still-open enclosing block level. The
//! levels come from the parser's parsed block structure (`compute_open_indents`), not a
//! line scan, so multiline scalars, URLs, and flow collections never create a false
//! level; on a parse error the option degrades to the base next/reference rule.
//!
//! Sources: adrienverge/yamllint#141.
use crate::rules::support::span_utils::BytePos;
use crate::text_version::edits::Edit;

use crate::text_version::lexical::Kind as LexicalKind;
use ryl_yaml_parser::{Event, ScalarStyle, Span, SpannedEventReceiver};

use crate::config::YamlLintConfig;
use crate::rules::support::line_syntax::{
    is_magic_first_line, leading_whitespace_width,
};
use crate::rules::support::span_utils::marker_byte_offset;
use crate::text_version::{Analysis, TextVersion};

mod input;
mod target;

pub const ID: &str = "comments-indentation";
pub const MESSAGE: &str = "comment not indented like content";

#[derive(Clone, Copy)]
pub(crate) struct RetainedLine {
    column: Option<usize>,
}

impl RetainedLine {
    pub(crate) const fn column(self) -> Option<usize> {
        self.column
    }
}

pub(crate) fn retained_line(
    line: &crate::format::layout::VirtualLine<'_>,
) -> RetainedLine {
    let text = line.content().trim_start();
    RetainedLine {
        column: text
            .starts_with('#')
            .then_some(line.prefix + line.content().len() - text.len()),
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Config {
    allow_any_open_indent: bool,
}

impl Config {
    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        Self {
            allow_any_open_indent: cfg.rule_option_bool(
                ID,
                "allow-any-open-indent",
                false,
            ),
        }
    }

    #[must_use]
    pub const fn new(allow_any_open_indent: bool) -> Self {
        Self {
            allow_any_open_indent,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
}

#[must_use]
pub fn check(buffer: &str, cfg: &Config) -> Vec<Violation> {
    check_with_analysis(TextVersion::new(buffer).analysis(), *cfg)
}

pub(crate) fn check_with_analysis(input: Analysis<'_>, cfg: Config) -> Vec<Violation> {
    let mut diagnostics: Vec<Violation> = Vec::new();
    if input.text().is_empty() {
        return diagnostics;
    }

    let lines = build_lines(input);

    let prev_content_indents = compute_prev_content_indents(&lines);
    let next_content_indents = compute_next_content_indents(&lines);
    // Only the option's open-block check consults this, so skip the parse entirely
    // when it's off (the common default path).
    let open_indents = if cfg.allow_any_open_indent {
        compute_open_indents(input, lines.len())
    } else {
        Vec::new()
    };

    let mut last_comment_indent: Option<usize> = None;

    for (idx, line) in lines.iter().enumerate() {
        match line.kind {
            LineKind::Comment => {
                let prev_indent = prev_content_indents[idx].unwrap_or(0);
                let next_indent = next_content_indents[idx].unwrap_or(0);
                let reference_indent =
                    last_comment_indent.unwrap_or_else(|| prev_indent.max(next_indent));

                if !comment_is_aligned(
                    line.indent,
                    reference_indent,
                    next_indent,
                    open_indents.get(idx).map_or(&[], Vec::as_slice),
                ) {
                    diagnostics.push(Violation {
                        line: idx + 1,
                        column: line.indent + 1,
                    });
                }

                last_comment_indent = Some(line.indent);
            }
            LineKind::ScalarTrailingComment => last_comment_indent = Some(line.indent),
            LineKind::Other | LineKind::DirectiveComment => {
                last_comment_indent = None;
            }
            LineKind::Empty | LineKind::ScalarBody => {}
        }
    }

    diagnostics
}

#[must_use]
pub fn fix(buffer: &str, cfg: &Config) -> Option<String> {
    fix_with_analysis(TextVersion::new(buffer).analysis(), *cfg)
}

pub(crate) fn fix_with_analysis(input: Analysis<'_>, cfg: Config) -> Option<String> {
    crate::text_version::edits::render(input.text(), edits(input, cfg))
}

pub(crate) fn edits(input: Analysis<'_>, cfg: Config) -> Vec<Edit> {
    let buffer = input.text();
    if buffer.is_empty() {
        return Vec::new();
    }

    let lines = build_lines(input);

    let open_indents = if cfg.allow_any_open_indent {
        compute_open_indents(input, lines.len())
    } else {
        Vec::new()
    };
    let mut planner = target::Planner::new(&lines, &open_indents);
    let mut edits = Vec::new();
    let mut offset = 0;
    for (index, raw, ending) in input.lines() {
        let prefix = lines[index].indent;
        let tabs = matches!(lines[index].kind, LineKind::Comment)
            && raw[..prefix].contains('\t');
        if let Some(indent) = planner.line(index, tabs) {
            edits.push((
                BytePos::new(offset),
                BytePos::new(offset + prefix),
                " ".repeat(indent),
            ));
        }
        offset += raw.len() + ending.len();
    }
    if planner.finish() { edits } else { Vec::new() }
}

fn comment_is_aligned(
    indent: usize,
    reference_indent: usize,
    next_indent: usize,
    open_indents: &[usize],
) -> bool {
    indent == reference_indent
        || indent == next_indent
        || open_indents.contains(&indent)
}

/// Each line's kind, as `check` and `fix` judge it.
struct RetainedLines<'a>(std::borrow::Cow<'a, [LineInfo]>);

impl RetainedLines<'_> {
    fn kind(&self, line: usize) -> Option<LineKind> {
        self.0.get(line).map(|info| info.kind)
    }
}

pub(crate) fn align_projected(
    source: Analysis<'_>,
    config: Option<Config>,
    layout: &crate::rules::indentation::Layout<'_>,
    candidates: &mut crate::rules::indentation::Candidates,
    permission: crate::format::layout::Permission<'_>,
) {
    if config != Some(Config::default()) {
        return;
    }
    let retained = build_lines(source);
    let lines: Vec<_> = layout
        .lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let original = retained[line.origin];
            let kind = if matches!(
                original.kind,
                LineKind::ScalarBody | LineKind::ScalarTrailingComment
            ) {
                original.kind
            } else {
                classify_line_kind(line.content())
            };
            LineInfo {
                kind,
                indent: line.prefix.saturating_add_signed(candidates.deltas[index]),
            }
        })
        .collect();
    let mut planner = target::Planner::new(&lines, &[]);
    for (index, line) in layout.lines.iter().enumerate() {
        let anchor = line.source_anchor(source.text());
        if let Some(indent) = planner.line(index, false)
            && permission.allows(anchor, anchor)
        {
            candidates.deltas[index] = isize::try_from(indent)
                .expect("text fits isize")
                - isize::try_from(line.prefix).expect("text fits isize");
        }
    }
}

pub(crate) fn align_trailing(
    source: Analysis<'_>,
    config: Option<Config>,
    layout: &crate::rules::indentation::Layout<'_>,
    candidates: &mut crate::rules::indentation::Candidates,
    permission: crate::format::layout::Permission<'_>,
) {
    let Some(admission) = target::TrailingAdmission::new(
        config,
        candidates.last_token,
        input::Projected::new(&layout.lines),
    ) else {
        return;
    };
    let columns = (0..=candidates.last_token.unwrap_or(0))
        .map(|line| layout.column(line, candidates.deltas[line]));
    let mut planner =
        target::Trailing::new(admission, RetainedLines(build_lines(source)), columns);
    for line in planner.first()..layout.lines.len() {
        let column = layout.column(line, candidates.deltas[line]);
        let anchor = column.source_anchor();
        if let Some(delta) = planner.line(line, column)
            && permission.allows(anchor, anchor)
        {
            candidates.deltas[line] = delta;
        }
    }
}

/// Classify every line once, shared by `check` and `fix`. Scalar tokens come from the
/// scanner rather than the parser, as in `comments_scan`, so a parse error degrades less.
fn build_lines(input: Analysis<'_>) -> std::borrow::Cow<'_, [LineInfo]> {
    if let Some(facts) = input.layout_facts() {
        return std::borrow::Cow::Borrowed(&facts.comment_lines);
    }
    let mut lines = line_facts(input);
    for (span, kind) in input.tokens().iter(input.text()) {
        classify_token(&mut lines, span, &kind);
    }
    std::borrow::Cow::Owned(lines)
}

pub(crate) fn line_facts(input: Analysis<'_>) -> Vec<LineInfo> {
    input
        .lines()
        .map(|(idx, line, _)| {
            let indent = leading_whitespace_width(line);
            let kind = if idx == 0 && is_magic_first_line(line) {
                LineKind::DirectiveComment
            } else {
                classify_line_kind(&line[indent..])
            };
            LineInfo { indent, kind }
        })
        .collect()
}

pub(crate) fn classify_token(
    lines: &mut [LineInfo],
    span: Span,
    kind: &LexicalKind<'_>,
) {
    // A token after a quoted scalar's closing quote, such as a flow `]`, makes its
    // line content again, as yamllint indents by each token's start line. yamllint's
    // tokens exclude comments, so a trailing comment leaves the line a scalar body.
    if !matches!(kind, LexicalKind::Comment { .. })
        && let Some(line) = lines.get_mut(span.start.line() - 1)
        && line.kind == LineKind::ScalarBody
    {
        line.kind = LineKind::Other;
    }
    let LexicalKind::Scalar { style, .. } = kind else {
        return;
    };
    // A block scalar's span starts on its first body line, or at its indicator when it
    // has no body, and runs on to the indentation of the line after its body.
    let first = span.start.line()
        + usize::from(
            !matches!(style, ScalarStyle::Literal | ScalarStyle::Folded)
                || lines
                    .get(span.start.line() - 1)
                    .is_some_and(|line| span.start.col() > line.indent),
        );
    let run_on = lines
        .get(span.end.line() - 1)
        .is_some_and(|line| span.end.col() <= line.indent);
    let last = span.end.line() - usize::from(run_on);
    for line in lines.iter_mut().take(last).skip(first - 1) {
        line.kind = LineKind::ScalarBody;
    }
    if let Some(line) = lines.get_mut(last)
        && run_on
        && line.kind == LineKind::Comment
    {
        line.kind = LineKind::ScalarTrailingComment;
    }
}

/// For each line (1-based `L` -> `result[L - 1]`), the start columns of the block
/// collections enclosing it: the levels `allow-any-open-indent` accepts a comment
/// against. On a parse error every set is empty, degrading to the base rule.
fn compute_open_indents(input: Analysis<'_>, line_count: usize) -> Vec<Vec<usize>> {
    struct Collector<'b> {
        buffer: &'b str,
        /// Open collections: `(start column, is block, start line)`.
        stack: Vec<(usize, bool, usize)>,
        /// Closed block collections: `(column, start line, end line)`.
        intervals: Vec<(usize, usize, usize)>,
    }
    impl SpannedEventReceiver<'_> for Collector<'_> {
        fn on_event(&mut self, event: Event<'_>, span: Span) {
            match event {
                Event::MappingStart(..) | Event::SequenceStart(..) => {
                    // A collection carries a block indentation level only when it is
                    // itself block (starts at a key/`-`, not `{`/`[`) and is not nested
                    // inside a flow collection: flow children such as the implicit
                    // mappings in `[a: 1, b: 2]` start at a key but have no block level.
                    let byte = marker_byte_offset(span.start).get();
                    let parent_is_flow =
                        self.stack.last().is_some_and(|&(_, block, _)| !block);
                    let is_block = !parent_is_flow
                        && !buffer_starts_with_flow_indicator(self.buffer, byte);
                    self.stack
                        .push((span.start.col(), is_block, span.start.line()));
                }
                Event::MappingEnd | Event::SequenceEnd => {
                    if let Some((col, is_block, start_line)) = self.stack.pop()
                        && is_block
                    {
                        self.intervals.push((col, start_line, span.start.line()));
                    }
                }
                _ => {}
            }
        }
    }

    let mut collector = Collector {
        buffer: input.text(),
        stack: Vec::new(),
        intervals: Vec::new(),
    };
    if !input.parsed().valid {
        return vec![Vec::new(); line_count];
    }
    for (event, span) in input.event_iter() {
        collector.on_event(event, span);
    }

    let mut result = vec![Vec::new(); line_count];
    for (col, start, end) in collector.intervals {
        // Slice directly to the interval's lines (`take().skip()` would re-walk from 0
        // each time, making this O(sum of end lines), quadratic for deep nesting).
        let hi = end.min(line_count);
        let lo = (start - 1).min(hi);
        for entry in &mut result[lo..hi] {
            entry.push(col);
        }
    }
    result
}

fn buffer_starts_with_flow_indicator(buffer: &str, byte: usize) -> bool {
    buffer[byte..].starts_with(['{', '['])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LineInfo {
    indent: usize,
    kind: LineKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineKind {
    Empty,
    Comment,
    /// yamllint reads a comment on the line a block scalar's span runs on to as
    /// trailing that scalar, and never checks it.
    ScalarTrailingComment,
    DirectiveComment,
    ScalarBody,
    Other,
}

fn classify_line_kind(content: &str) -> LineKind {
    let trimmed = content.trim_start_matches([' ', '\t']);

    if trimmed.is_empty() {
        LineKind::Empty
    } else if trimmed.starts_with("# yamllint ") {
        LineKind::DirectiveComment
    } else if trimmed.starts_with('#') {
        LineKind::Comment
    } else {
        LineKind::Other
    }
}

fn compute_prev_content_indents(lines: &[LineInfo]) -> Vec<Option<usize>> {
    let mut result: Vec<Option<usize>> = Vec::with_capacity(lines.len());
    let mut latest: Option<usize> = None;
    for line in lines {
        if line.kind == LineKind::Other {
            latest = Some(line.indent);
        }
        result.push(latest);
    }
    result
}

fn compute_next_content_indents(lines: &[LineInfo]) -> Vec<Option<usize>> {
    let mut result: Vec<Option<usize>> = vec![None; lines.len()];
    let mut upcoming: Option<usize> = None;
    for (idx, line) in lines.iter().enumerate().rev() {
        if line.kind == LineKind::Other {
            upcoming = Some(line.indent);
        }
        result[idx] = upcoming;
    }
    result
}
