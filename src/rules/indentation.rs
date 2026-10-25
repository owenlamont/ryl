//! `indentation`: yamllint's token-driven rule on the parser's scanner. No safe
//! `--fix`; `ryl format` re-indents through [`reindent`], which places each line where
//! the check expects it.

use crate::text_version::lexical::Kind as LexicalKind;
use crate::text_version::{Analysis, TextVersion};
use ryl_yaml_parser::ScalarStyle;

use crate::config::YamlLintConfig;
use crate::rules::support::punctuation::byte_line_and_column;
use crate::rules::support::span_utils::marker_byte_offset;

mod rewrite;

pub(crate) use rewrite::reindent_with_analysis;
pub use rewrite::{Cause, Refusal, Reindented, fix, reindent};
pub(crate) use rewrite::{Layout, Targets};

pub const ID: &str = "indentation";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    spaces: SpacesSetting,
    indent_sequences: IndentSequencesSetting,
    check_multi_line_strings: bool,
    /// Where [`reindent`] puts a block mapping in a block sequence; `None` leaves it.
    dash_on_own_line: Option<bool>,
    require_consistent: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpacesSetting {
    Fixed(usize),
    Consistent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentSequencesSetting {
    True,
    False,
    Whatever,
    Consistent,
}

impl Config {
    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        let shared = cfg
            .indent_width()
            .map_or(SpacesSetting::Consistent, |width| {
                SpacesSetting::Fixed(usize::from(width.get()))
            });
        let spaces = cfg.rule_option(ID, "spaces").map_or(shared, |node| {
            node.as_integer()
                .map_or(SpacesSetting::Consistent, |value| {
                    let non_negative = value.max(0);
                    let fixed = usize::try_from(non_negative).unwrap_or(usize::MAX);
                    SpacesSetting::Fixed(fixed)
                })
        });

        let indent_sequences = cfg.rule_option(ID, "indent-sequences").map_or(
            IndentSequencesSetting::True,
            |node| {
                if let Some(choice) = node.as_str() {
                    return if choice == "whatever" {
                        IndentSequencesSetting::Whatever
                    } else {
                        IndentSequencesSetting::Consistent
                    };
                }

                if node.as_bool() == Some(false) {
                    IndentSequencesSetting::False
                } else {
                    IndentSequencesSetting::True
                }
            },
        );

        let check_multi_line_strings = cfg
            .rule_option(ID, "check-multi-line-strings")
            .and_then(crate::yaml_dom::YamlOwned::as_bool)
            .unwrap_or(false);

        Self {
            spaces,
            indent_sequences,
            check_multi_line_strings,
            dash_on_own_line: None,
            require_consistent: false,
        }
    }

    #[must_use]
    pub const fn new(
        spaces: SpacesSetting,
        indent_sequences: IndentSequencesSetting,
        check_multi_line_strings: bool,
    ) -> Self {
        Self {
            spaces,
            indent_sequences,
            check_multi_line_strings,
            dash_on_own_line: None,
            require_consistent: false,
        }
    }

    #[must_use]
    pub const fn with_dash_on_own_line(mut self, value: bool) -> Self {
        self.dash_on_own_line = Some(value);
        self
    }

    pub(crate) const fn with_consistent_spaces(mut self) -> Self {
        self.spaces = SpacesSetting::Consistent;
        self
    }

    pub(crate) const fn requiring_consistent(mut self) -> Self {
        self.require_consistent = true;
        self
    }

    pub(crate) const fn requires_consistency(self) -> bool {
        self.require_consistent
    }

    /// Whether a file indented uniformly by `width` spaces satisfies `spaces`.
    #[must_use]
    pub fn admits_width(&self, width: u8) -> bool {
        match self.spaces {
            SpacesSetting::Consistent => true,
            SpacesSetting::Fixed(spaces) => spaces == usize::from(width),
        }
    }
}

#[must_use]
pub fn check(buffer: &str, cfg: &Config) -> Vec<Violation> {
    check_with_analysis(TextVersion::new(buffer).analysis(), cfg)
}

pub(crate) fn check_with_analysis(input: Analysis<'_>, cfg: &Config) -> Vec<Violation> {
    let scanned = input.indentation();
    let mut analyzer =
        Analyzer::new(input.text(), &scanned.line_starts, cfg, Mode::Check);
    analyzer.run(scanned.tokens.iter().copied());
    analyzer.diagnostics
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Check,
    /// Record each line's shift to where the check expects it, and expect what follows
    /// from the shifted columns.
    Target,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    StreamBoundary,
    BlockMappingStart,
    BlockSequenceStart,
    BlockEnd,
    BlockEntry,
    FlowMappingStart,
    FlowMappingEnd,
    FlowSequenceStart,
    FlowSequenceEnd,
    Key { explicit: bool },
    Value,
    Property,
    Scalar { style: ScalarStyle, empty: bool },
    Marker,
    Other,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Token {
    kind: Kind,
    start: usize,
    end: usize,
    line: usize,
    column: usize,
    end_line: usize,
    end_column: usize,
}

impl Token {
    fn is(self, kinds: &[Kind]) -> bool {
        kinds.contains(&self.kind)
    }
}

pub(crate) struct Scanned {
    tokens: Vec<Token>,
    line_starts: Vec<usize>,
}

pub(crate) fn scan_with_analysis(input: Analysis<'_>) -> Scanned {
    let line_starts = normalized_line_starts(input);
    let tokens = normalized_tokens(input, &line_starts).collect();
    Scanned {
        tokens,
        line_starts,
    }
}

pub(crate) fn normalized_line_starts(input: Analysis<'_>) -> Vec<usize> {
    let text = input.text();
    let mut starts = input.byte_line_starts().to_vec();
    if text.contains('\u{feff}') {
        for (span, _) in input.tokens().iter(input.text()) {
            for marker in [span.start, span.end] {
                align_line_start(text, &mut starts[marker.line() - 1], marker);
            }
        }
        for header in input.headers() {
            skip_prefix_bom(text, &mut starts[header.line - 1]);
        }
    }
    starts
}

pub(crate) fn normalized_tokens<'a>(
    input: Analysis<'a>,
    line_starts: &'a [usize],
) -> impl Iterator<Item = Token> {
    normalized_tokens_from(input.text(), line_starts, input.tokens().iter(input.text()))
}

pub(crate) fn normalized_tokens_from<
    'a,
    K: std::borrow::Borrow<LexicalKind<'a>> + 'a,
>(
    text: &'a str,
    line_starts: &'a [usize],
    tokens: impl Iterator<Item = (ryl_yaml_parser::Span, K)> + 'a,
) -> impl Iterator<Item = Token> + 'a {
    crate::format::layout::stats::count(
        crate::format::layout::stats::Counter::Normalize,
    );
    let mut normalizer = Normalizer {
        text,
        line_starts,
        previous: None,
    };
    tokens.filter_map(move |(span, token_type)| {
        normalizer.token(span, token_type.borrow())
    })
}

struct Normalizer<'a> {
    text: &'a str,
    line_starts: &'a [usize],
    previous: Option<Token>,
}

impl Normalizer<'_> {
    fn token(
        &mut self,
        span: ryl_yaml_parser::Span,
        token_type: &LexicalKind<'_>,
    ) -> Option<Token> {
        let (text, line_starts) = (self.text, self.line_starts);
        let (mut start, mut end) = (
            marker_byte_offset(span.start).get(),
            marker_byte_offset(span.end).get(),
        );
        let (mut line, mut end_line) = (span.start.line() - 1, span.end.line() - 1);
        let (mut column, mut end_column) = (span.start.col(), span.end.col());
        let kind = match token_type {
            LexicalKind::Comment { .. } => {
                return None;
            }
            LexicalKind::FlowMappingStart | LexicalKind::FlowMappingEnd
                if start == end =>
            {
                return None;
            }
            LexicalKind::StreamStart | LexicalKind::StreamEnd => Kind::StreamBoundary,
            LexicalKind::BlockMappingStart => Kind::BlockMappingStart,
            LexicalKind::BlockSequenceStart => Kind::BlockSequenceStart,
            LexicalKind::BlockEnd => Kind::BlockEnd,
            LexicalKind::BlockEntry => {
                // the parser marks an entry past the dash's trailing blanks and
                // comment.
                (start, column) = match self.previous.as_ref() {
                    Some(prev) if prev.kind == Kind::BlockSequenceStart => {
                        (prev.start, prev.column)
                    }
                    _ => {
                        let line_start = line_starts[line];
                        let indent = count_spaces(text, line_start);
                        (line_start + indent, indent)
                    }
                };
                line = line_starts.partition_point(|&offset| offset <= start) - 1;
                end = start + 1;
                end_line = line;
                end_column = column + 1;
                Kind::BlockEntry
            }
            LexicalKind::FlowMappingStart => Kind::FlowMappingStart,
            LexicalKind::FlowMappingEnd => Kind::FlowMappingEnd,
            LexicalKind::FlowSequenceStart => Kind::FlowSequenceStart,
            LexicalKind::FlowSequenceEnd => Kind::FlowSequenceEnd,
            LexicalKind::Key => Kind::Key {
                explicit: char_at(text, start) == Some('?')
                    && char_at(text, start + 1)
                        .is_none_or(|ch| matches!(ch, ' ' | '\t') || is_break(ch)),
            },
            LexicalKind::Value => Kind::Value,
            LexicalKind::DocumentStart | LexicalKind::DocumentEnd => Kind::Marker,
            LexicalKind::Anchor | LexicalKind::Tag => Kind::Property,
            LexicalKind::Scalar { style, empty } => {
                if matches!(style, ScalarStyle::Literal | ScalarStyle::Folded) {
                    // the parser starts a block scalar at its content, PyYAML at
                    // `|`/`>`.
                    let from = self.previous.as_ref().map_or(0, |prev| prev.end);
                    start = block_indicator(text, from).unwrap_or(start);
                    line = line_starts.partition_point(|&offset| offset <= start) - 1;
                    column = text[line_starts[line]..start].chars().count();
                    let end_line_start = line_starts[end_line];
                    let trailing = end - end_line_start;
                    if count_spaces(text, end_line_start) >= trailing
                        && trailing <= span.start.col()
                    {
                        end = end_line_start;
                        end_column = 0;
                    }
                }
                Kind::Scalar {
                    style: *style,
                    empty: *empty && start == end,
                }
            }
            _ => Kind::Other,
        };
        let token = Token {
            kind,
            start,
            end,
            line,
            column,
            end_line,
            end_column,
        };
        self.previous = Some(token);
        Some(token)
    }
}

fn align_line_start(
    text: &str,
    line_start: &mut usize,
    marker: ryl_yaml_parser::Marker,
) {
    if text[*line_start..].starts_with('\u{feff}')
        && byte_line_and_column(
            text.as_bytes(),
            &[*line_start],
            marker_byte_offset(marker).get(),
        )
        .1 - 1
            > marker.col()
    {
        skip_prefix_bom(text, line_start);
    }
}

fn skip_prefix_bom(text: &str, start: &mut usize) {
    if text[*start..].starts_with('\u{feff}') {
        *start += '\u{feff}'.len_utf8();
    }
}

fn char_at(text: &str, idx: usize) -> Option<char> {
    text.get(idx..).and_then(|tail| tail.chars().next())
}

fn count_spaces(text: &str, from: usize) -> usize {
    text.as_bytes()[from.min(text.len())..]
        .iter()
        .take_while(|&&ch| ch == b' ')
        .count()
}

const fn is_break(ch: char) -> bool {
    matches!(ch, '\n' | '\r')
}

fn block_indicator(text: &str, from: usize) -> Option<usize> {
    let mut in_comment = false;
    text[from..].char_indices().find_map(|(offset, ch)| {
        in_comment = (in_comment || ch == '#') && !is_break(ch);
        (!in_comment && matches!(ch, '|' | '>')).then_some(from + offset)
    })
}

fn to_isize(value: usize) -> isize {
    isize::try_from(value).unwrap_or(isize::MAX)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParentKind {
    Root,
    BlockMapping,
    FlowMapping,
    BlockSequence,
    FlowSequence,
    BlockEntry,
    Key,
    Value,
}

#[derive(Debug, Clone, Copy)]
struct Parent {
    kind: ParentKind,
    indent: isize,
    line_indent: isize,
    explicit_key: bool,
    implicit_block_seq: bool,
    /// Where a block collection starts in the source: its block scalars' bodies with an
    /// indentation indicator move as far as it does.
    start: usize,
}

impl Parent {
    const fn new(kind: ParentKind, indent: isize) -> Self {
        Self {
            kind,
            indent,
            line_indent: indent,
            explicit_key: false,
            implicit_block_seq: false,
            start: 0,
        }
    }
}

struct UnexpectedToken;

struct Analyzer<'a> {
    text: &'a str,
    lines: Option<&'a [crate::format::layout::VirtualLine<'a>]>,
    line_starts: &'a [usize],
    check_multi_line_strings: bool,
    stack: Vec<Parent>,
    cur_line: usize,
    cur_line_indent: isize,
    spaces: Option<isize>,
    indent_sequences: IndentSequencesSetting,
    diagnostics: Vec<Violation>,
    mode: Mode,
    shifts: Vec<Option<Shift>>,
    scalar_owners: Option<Vec<(usize, usize)>>,
    block_tails: Vec<BlockTail>,
    gaps: Vec<Gap>,
}

/// The line after a block scalar whose body ends before it, with the body's shift and a
/// lower bound on its content column.
pub(crate) type BlockTail = (usize, isize, Option<usize>);

/// The spaces after an indicator that `ryl format` closes to one, with the rule that owns
/// them.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Gap {
    pub(crate) line: usize,
    pub(crate) column: usize,
    pub(crate) removed: usize,
    pub(crate) rule: &'static str,
}

#[derive(Debug, Clone, Copy)]
enum Shift {
    /// A line led by a token found at `found`.
    Token { found: isize, delta: isize },
    /// A line inside a multi-line scalar.
    Carried(isize),
    /// A document marker's line, which no comment aligns with, since the markers pass
    /// adds and removes them after comments settle.
    Marker(isize),
}

impl Shift {
    const fn delta(self) -> isize {
        match self {
            Self::Token { delta, .. } | Self::Carried(delta) | Self::Marker(delta) => {
                delta
            }
        }
    }
}

impl<'a> Analyzer<'a> {
    fn new(text: &'a str, line_starts: &'a [usize], cfg: &Config, mode: Mode) -> Self {
        Self {
            text,
            lines: None,
            line_starts,
            check_multi_line_strings: cfg.check_multi_line_strings,
            stack: vec![Parent::new(ParentKind::Root, 0)],
            cur_line: 0,
            cur_line_indent: 0,
            spaces: match cfg.spaces {
                SpacesSetting::Fixed(value) => Some(to_isize(value)),
                SpacesSetting::Consistent => None,
            },
            indent_sequences: cfg.indent_sequences,
            diagnostics: Vec::new(),
            mode,
            shifts: if mode == Mode::Target {
                vec![None; line_starts.len()]
            } else {
                Vec::new()
            },
            scalar_owners: None,
            block_tails: Vec::new(),
            gaps: Vec::new(),
        }
    }

    fn delta(&self, line: usize) -> isize {
        self.shifts
            .get(line)
            .copied()
            .flatten()
            .map_or(0, Shift::delta)
    }

    fn column(&self, token: &Token) -> isize {
        let first = self.gaps.partition_point(|gap| gap.line < token.line);
        let closed: usize = self.gaps[first..]
            .iter()
            .take_while(|gap| gap.line == token.line)
            .filter(|gap| gap.column < token.column)
            .map(|gap| gap.removed)
            .sum();
        to_isize(token.column) + self.delta(token.line) - to_isize(closed)
    }

    /// Records the gap after `token` when it is an indicator whose content follows on its
    /// line past more than one space.
    fn close_gap(&mut self, token: &Token, next: Option<&Token>, first_in_line: bool) {
        let rule = match token.kind {
            Kind::BlockEntry => crate::rules::hyphens::ID,
            Kind::Key { explicit: true } => crate::rules::colons::ID,
            Kind::Value if first_in_line => crate::rules::colons::ID,
            _ => return,
        };
        let Some(next) = next.filter(|next| {
            next.line == token.line && !matches!(next.kind, Kind::BlockEnd)
        }) else {
            return;
        };
        let gap = next.column - token.column - 1;
        let spaces = self.lines.expect("target analysis has virtual lines")[token.line]
            .spaces_after(token.column);
        if gap > 1 && spaces == gap {
            self.gaps.push(Gap {
                line: token.line,
                column: token.column,
                removed: gap - 1,
                rule,
            });
        }
    }

    fn run(&mut self, mut tokens: impl Iterator<Item = Token>) {
        self.run_with(&mut tokens, |_, _, _, _| {});
    }

    fn run_with(
        &mut self,
        mut tokens: impl Iterator<Item = Token>,
        mut observe: impl FnMut(&Token, Option<&Token>, Option<&Token>, Option<&Token>),
    ) {
        walk(&mut tokens, |token, previous, next, after| {
            observe(token, previous, next, after);
            self.advance(token, previous, next, after);
        });
    }

    fn advance(
        &mut self,
        token: &Token,
        prev: Option<&Token>,
        next: Option<&Token>,
        after: Option<&Token>,
    ) {
        if self.step(token, prev, next, after).is_err() {
            self.push(
                token.line + 1,
                token.column,
                "cannot infer indentation: unexpected token".to_string(),
            );
        }
    }

    fn block_parent(&self) -> Option<&Parent> {
        self.stack.iter().rev().find(|parent| {
            matches!(
                parent.kind,
                ParentKind::BlockMapping | ParentKind::BlockSequence
            )
        })
    }

    fn top(&self) -> Parent {
        self.stack[self.stack.len() - 1]
    }

    fn below_top(&self) -> Parent {
        self.stack[self.stack.len() - 2]
    }

    fn detect_indent(&mut self, base: isize, found: isize) -> isize {
        base.saturating_add(*self.spaces.get_or_insert(found - base))
    }

    fn step(
        &mut self,
        token: &Token,
        prev: Option<&Token>,
        next: Option<&Token>,
        nextnext: Option<&Token>,
    ) -> Result<(), UnexpectedToken> {
        let visible = !matches!(
            token.kind,
            Kind::StreamBoundary | Kind::BlockEnd | Kind::Scalar { empty: true, .. }
        );
        let first_in_line = visible && token.line + 1 > self.cur_line;
        let found = to_isize(token.column);
        if first_in_line && self.mode == Mode::Target {
            let expected = self.expected(token, found);
            let delta = expected - found;
            self.shifts[token.line] = Some(if token.kind == Kind::Marker {
                Shift::Marker(delta)
            } else {
                Shift::Token { found, delta }
            });
        }
        if self.mode == Mode::Target {
            self.close_gap(token, next, first_in_line);
        }
        if first_in_line && self.mode != Mode::Target {
            let expected = self.expected(token, found);
            if found != expected {
                let message = if expected < 0 {
                    format!("wrong indentation: expected at least {}", found + 1)
                } else {
                    wrong_indent_message(expected, found)
                };
                self.push(token.line + 1, token.column, message);
            }
        }
        if let Kind::Scalar { style, .. } = token.kind {
            if self.check_multi_line_strings {
                self.check_scalar(token, style);
            }
            if self.mode == Mode::Target {
                self.carry_scalar_lines(token, style);
                self.record_scalar_owner(token);
            }
        }
        if visible {
            self.cur_line = self.real_end_line(token);
            if first_in_line {
                self.cur_line_indent = self.column(token);
            }
        }
        self.update_stack(token, prev, next, nextnext)?;
        self.unwind(token, next)
    }

    fn record_scalar_owner(&mut self, token: &Token) {
        let column = self.block_parent().map_or(0, |parent| parent.indent);
        if let Some(owners) = &mut self.scalar_owners {
            owners.push((token.start, usize::try_from(column).unwrap_or(0)));
        }
    }

    fn expected(&mut self, token: &Token, found: isize) -> isize {
        let top = self.top();
        match token.kind {
            Kind::FlowMappingEnd | Kind::FlowSequenceEnd => top.line_indent.max(
                self.block_parent()
                    .map_or(0, |parent| parent.indent.saturating_add(1)),
            ),
            Kind::Value => top.indent,
            _ if top.kind == ParentKind::Key && top.explicit_key => {
                self.detect_indent(top.indent, found)
            }
            _ => top.indent,
        }
    }

    fn update_stack(
        &mut self,
        token: &Token,
        prev: Option<&Token>,
        next: Option<&Token>,
        nextnext: Option<&Token>,
    ) -> Result<(), UnexpectedToken> {
        let column = self.column(token);
        let Some(next) = next else {
            return Ok(());
        };
        let next_column = self.column(next);
        match token.kind {
            Kind::BlockMappingStart | Kind::BlockSequenceStart => {
                let (child, kind) = if token.kind == Kind::BlockMappingStart {
                    (
                        matches!(next.kind, Kind::Key { .. }),
                        ParentKind::BlockMapping,
                    )
                } else {
                    (next.kind == Kind::BlockEntry, ParentKind::BlockSequence)
                };
                if !child || next.line != token.line {
                    return Err(UnexpectedToken);
                }
                self.stack.push(Parent {
                    start: token.start,
                    ..Parent::new(kind, column)
                });
            }
            Kind::FlowMappingStart | Kind::FlowSequenceStart => {
                let indent = if next.line == token.line {
                    next_column
                } else {
                    self.detect_indent(self.cur_line_indent, next_column)
                };
                let kind = if token.kind == Kind::FlowMappingStart {
                    ParentKind::FlowMapping
                } else {
                    ParentKind::FlowSequence
                };
                self.stack.push(Parent {
                    line_indent: self.cur_line_indent,
                    ..Parent::new(kind, indent)
                });
            }
            Kind::BlockEntry => {
                if self.top().kind != ParentKind::BlockSequence {
                    self.stack.push(Parent {
                        implicit_block_seq: true,
                        start: token.start,
                        ..Parent::new(ParentKind::BlockSequence, column)
                    });
                }
                // An entry's content sits deeper than its `-`, so anything else is a sibling
                // or the end of the sequence.
                let empty = next.kind == Kind::BlockEnd
                    || (next.line != token.end_line && next.column <= token.column);
                if !empty {
                    let indent = if next.line == token.end_line {
                        next_column
                    } else {
                        self.detect_indent(column, next_column)
                    };
                    self.stack.push(Parent::new(ParentKind::BlockEntry, indent));
                } else if next.kind != Kind::BlockEntry && self.top().implicit_block_seq
                {
                    self.stack.pop();
                }
            }
            Kind::Key { explicit } => {
                self.stack.push(Parent {
                    explicit_key: explicit,
                    ..Parent::new(ParentKind::Key, self.top().indent)
                });
            }
            Kind::Value => self.push_value(prev, next, nextnext)?,
            _ => {}
        }
        Ok(())
    }

    fn push_value(
        &mut self,
        prev: Option<&Token>,
        next: &Token,
        nextnext: Option<&Token>,
    ) -> Result<(), UnexpectedToken> {
        let key = self.top();
        if key.kind != ParentKind::Key {
            return Err(UnexpectedToken);
        }
        let prev_line = prev.map_or(0, |prev| prev.line);
        let next = match nextnext {
            Some(after)
                if next.kind == Kind::Property
                    && next.line == prev_line
                    && next.line < after.line =>
            {
                after
            }
            _ => next,
        };
        if next.is(&[Kind::BlockEnd, Kind::FlowMappingEnd, Kind::FlowSequenceEnd])
            || matches!(next.kind, Kind::Key { .. })
        {
            return Ok(());
        }
        let next_column = self.column(next);
        let indent = if key.explicit_key {
            self.detect_indent(key.indent, next_column)
        } else if next.line == prev_line {
            next_column
        } else if next.is(&[Kind::BlockSequenceStart, Kind::BlockEntry]) {
            self.sequence_value_indent(key.indent, next_column)
        } else {
            self.detect_indent(key.indent, next_column)
        };
        self.stack.push(Parent::new(ParentKind::Value, indent));
        Ok(())
    }

    fn sequence_value_indent(
        &mut self,
        key_indent: isize,
        next_column: isize,
    ) -> isize {
        let flush = next_column == key_indent;
        match self.indent_sequences {
            IndentSequencesSetting::False => key_indent,
            IndentSequencesSetting::True if self.spaces.is_none() && flush => -1,
            IndentSequencesSetting::True => self.detect_indent(key_indent, next_column),
            setting => {
                if setting == IndentSequencesSetting::Consistent {
                    self.indent_sequences = if flush {
                        IndentSequencesSetting::False
                    } else {
                        IndentSequencesSetting::True
                    };
                }
                if flush {
                    key_indent
                } else {
                    self.detect_indent(key_indent, next_column)
                }
            }
        }
    }

    fn unwind(
        &mut self,
        token: &Token,
        next: Option<&Token>,
    ) -> Result<(), UnexpectedToken> {
        let next_is = |kinds: &[Kind]| next.is_some_and(|next| next.is(kinds));
        let mut consumed = false;
        loop {
            let top = self.top();
            let pop = match top.kind {
                ParentKind::FlowSequence => {
                    !consumed && token.kind == Kind::FlowSequenceEnd
                }
                ParentKind::FlowMapping => {
                    !consumed && token.kind == Kind::FlowMappingEnd
                }
                ParentKind::BlockMapping | ParentKind::BlockSequence => {
                    !consumed && token.kind == Kind::BlockEnd && !top.implicit_block_seq
                }
                ParentKind::BlockEntry => {
                    if token.kind != Kind::BlockEntry
                        && token.kind != Kind::Property
                        && self.below_top().implicit_block_seq
                        && !next_is(&[Kind::BlockEntry])
                    {
                        self.stack.pop();
                        true
                    } else {
                        next_is(&[Kind::BlockEntry, Kind::BlockEnd])
                    }
                }
                ParentKind::Value => {
                    if token.kind == Kind::Value || token.kind == Kind::Property {
                        false
                    } else {
                        self.stack.pop();
                        true
                    }
                }
                ParentKind::Key => {
                    next_is(&[
                        Kind::BlockEnd,
                        Kind::FlowMappingEnd,
                        Kind::FlowSequenceEnd,
                    ]) || next.is_some_and(|next| matches!(next.kind, Kind::Key { .. }))
                }
                ParentKind::Root => false,
            };
            if !pop {
                return Ok(());
            }
            consumed |= matches!(
                top.kind,
                ParentKind::FlowSequence
                    | ParentKind::FlowMapping
                    | ParentKind::BlockMapping
                    | ParentKind::BlockSequence
            );
            self.stack.pop();
        }
    }

    /// Moves a scalar's later lines: a block body to where [`Self::check_scalar`] expects
    /// it, or with the collection it is indented against where an indentation indicator
    /// fixes its column, and a continuation with the line it starts on.
    fn carry_scalar_lines(&mut self, token: &Token, style: ScalarStyle) {
        let block = matches!(style, ScalarStyle::Literal | ScalarStyle::Folded);
        let delta = if !block {
            let lines = self.lines.expect("target analysis has virtual lines");
            // A continuation follows its token, but one deeper than its collection stays so.
            (token.line + 1..=last_line(token))
                .map(|line| &lines[line])
                .filter(|line| !line.body.trim_start().is_empty())
                .map(|line| {
                    to_isize(
                        line.prefix + line.body.len() - line.body.trim_start().len(),
                    )
                })
                .fold(
                    self.column(token) - to_isize(token.column),
                    |delta, column| match self.block_parent() {
                        Some(parent) if column > self.source_column(parent.start) => {
                            delta.max(parent.indent + 1 - column)
                        }
                        _ => delta,
                    },
                )
        } else if let Some((_, indent)) = self.body_indents(token).next()
            && !self.has_indentation_indicator(token)
        {
            let found = to_isize(indent);
            self.expected_scalar_indent(token, style, found) - found
        } else {
            self.block_parent()
                .map_or(0, |parent| parent.indent - self.source_column(parent.start))
        };
        if block && token.end_column == 0 {
            // A blank-only body has no content column to bound; the refusal then treats
            // any move right of the comment's own column as entering it.
            let content = self.body_indents(token).next().map(|(_, indent)| indent);
            self.block_tails.push((token.end_line, delta, content));
        }
        let lines = self.lines.expect("target analysis has virtual lines");
        let last = last_line(token);
        for line in token.line + 1..=last {
            self.shifts[line] = Some(Shift::Carried(delta));
        }
        if block
            && lines.len() == last + 2
            && let Some(tail) = lines.last()
            && tail.ending.is_empty()
            && tail.body.is_empty()
            && !tail.bom
        {
            // Keep one space so a keep-chomped scalar retains its unterminated last line.
            let delta = delta.max(1 - to_isize(tail.prefix));
            self.shifts[last + 1] = Some(Shift::Carried(delta));
        }
    }

    /// The source column of byte `at`, before same-line edits project it.
    fn source_column(&self, at: usize) -> isize {
        let start = self.text[..at].rfind(['\n', '\r']).map_or(0, |at| at + 1);
        to_isize(
            self.text[start..at]
                .chars()
                .filter(|&ch| ch != '\u{feff}')
                .count(),
        )
    }

    fn has_indentation_indicator(&self, token: &Token) -> bool {
        self.text[token.start + 1..]
            .chars()
            .take_while(|&ch| ch.is_ascii_digit() || matches!(ch, '+' | '-'))
            .any(|ch| ch.is_ascii_digit())
    }

    /// Each later line of `token` that is not blank, with its indent.
    fn body_indents(
        &self,
        token: &Token,
    ) -> impl Iterator<Item = (usize, usize)> + use<'a> {
        let (text, line_starts) = (self.text, self.line_starts);
        (token.line + 1..=last_line(token)).filter_map(move |line| {
            let line_start = line_starts[line];
            let indent = count_spaces(text, line_start);
            (!char_at(text, line_start + indent).is_some_and(is_break))
                .then_some((line, indent))
        })
    }

    fn check_scalar(&mut self, token: &Token, style: ScalarStyle) {
        let lines: Vec<(usize, usize)> = self.body_indents(token).collect();
        let Some(&(_, first)) = lines.first() else {
            return;
        };
        let expected = self.expected_scalar_indent(token, style, to_isize(first));
        for (line, indent) in lines {
            let found = to_isize(indent);
            if found != expected {
                self.push(line + 1, indent, wrong_indent_message(expected, found));
            }
        }
    }

    fn expected_scalar_indent(
        &mut self,
        token: &Token,
        style: ScalarStyle,
        found: isize,
    ) -> isize {
        let column = self.column(token);
        let top = self.top();
        match style {
            ScalarStyle::Plain => column,
            ScalarStyle::SingleQuoted | ScalarStyle::DoubleQuoted => column + 1,
            _ => match top.kind {
                ParentKind::BlockEntry | ParentKind::Key => {
                    self.detect_indent(column, found)
                }
                ParentKind::Value if token.line + 1 > self.cur_line => {
                    self.detect_indent(top.indent, found)
                }
                ParentKind::Value if self.below_top().explicit_key => {
                    self.detect_indent(column, found)
                }
                ParentKind::Value => self.detect_indent(self.below_top().indent, found),
                _ => self.detect_indent(top.indent, found),
            },
        }
    }

    fn real_end_line(&self, token: &Token) -> usize {
        let mut end_line = token.end_line + 1;
        if !matches!(token.kind, Kind::Scalar { .. }) {
            return end_line;
        }
        let from = self.text[..token.start]
            .char_indices()
            .next_back()
            .map_or(0, |(byte, _)| byte);
        for (offset, ch) in self.text[from..token.end].char_indices().rev() {
            let pos = from + offset;
            if !matches!(ch, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c') {
                break;
            }
            if ch == '\n' || (ch == '\r' && char_at(self.text, pos + 1) != Some('\n')) {
                end_line -= 1;
            }
        }
        end_line
    }

    fn push(&mut self, line: usize, found: usize, message: String) {
        let start = self.line_starts[line - 1];
        let bom = usize::from(self.text[..start].ends_with('\u{feff}'));
        self.diagnostics.push(Violation {
            line,
            column: found + bom + 1,
            message,
        });
    }
}

fn walk<T: Copy>(
    mut tokens: impl Iterator<Item = T>,
    mut visit: impl FnMut(&T, Option<&T>, Option<&T>, Option<&T>),
) {
    let mut previous = None;
    let mut current = tokens.next();
    let mut next = tokens.next();
    let mut after = tokens.next();
    while let Some(token) = current {
        visit(&token, previous.as_ref(), next.as_ref(), after.as_ref());
        previous = current;
        current = next;
        next = after;
        after = tokens.next();
    }
}

/// The last line holding a character of `token`.
const fn last_line(token: &Token) -> usize {
    if token.end_column > 0 {
        token.end_line
    } else {
        token.end_line.saturating_sub(1)
    }
}

fn wrong_indent_message(expected: isize, found: isize) -> String {
    format!("wrong indentation: expected {expected} but found {found}")
}
