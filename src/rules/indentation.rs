//! `indentation`: yamllint's token-driven rule on granit's scanner. No safe `--fix`;
//! `ryl format` re-indents through [`reindent`], which places each line where the check
//! expects it.

use granit_parser::{ScalarStyle, Scanner, StrInput, TokenType};

use crate::config::YamlLintConfig;
use crate::rules::support::punctuation::{build_line_starts, line_and_column};
use crate::rules::support::span_utils::CharPos;

mod rewrite;

pub use rewrite::{Cause, Refusal, Reindented, fix, moved_lines, reindent};

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
        }
    }

    #[must_use]
    pub const fn with_dash_on_own_line(mut self, value: bool) -> Self {
        self.dash_on_own_line = Some(value);
        self
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
    let chars: Vec<(usize, char)> = buffer.char_indices().collect();
    let line_starts = build_line_starts(&chars);
    let tokens = scan(buffer, &chars, &line_starts);
    let mut analyzer = Analyzer::new(&chars, &line_starts, cfg, Mode::Check);
    analyzer.run(&tokens);
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
    Other,
}

#[derive(Debug, Clone, Copy)]
struct Token {
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

fn scan(buffer: &str, chars: &[(usize, char)], line_starts: &[CharPos]) -> Vec<Token> {
    let mut tokens: Vec<Token> = Vec::new();
    for token in Scanner::new(StrInput::new(buffer)).map_while(Result::ok) {
        let (span, token_type) = token.into_parts();
        let (mut start, mut end) = (span.start.index(), span.end.index());
        let kind = match token_type {
            TokenType::Comment(_) => continue,
            TokenType::FlowMappingStart | TokenType::FlowMappingEnd if start == end => {
                continue;
            }
            TokenType::StreamStart | TokenType::StreamEnd => Kind::StreamBoundary,
            TokenType::BlockMappingStart => Kind::BlockMappingStart,
            TokenType::BlockSequenceStart => Kind::BlockSequenceStart,
            TokenType::BlockEnd => Kind::BlockEnd,
            TokenType::BlockEntry => {
                // granit marks an entry past the dash's trailing blanks and comment.
                start = match tokens.last() {
                    Some(prev) if prev.kind == Kind::BlockSequenceStart => prev.start,
                    _ => {
                        let line_start =
                            line_starts[locate(line_starts, start).0].get();
                        line_start + count_spaces(chars, line_start)
                    }
                };
                end = start + 1;
                Kind::BlockEntry
            }
            TokenType::FlowMappingStart => Kind::FlowMappingStart,
            TokenType::FlowMappingEnd => Kind::FlowMappingEnd,
            TokenType::FlowSequenceStart => Kind::FlowSequenceStart,
            TokenType::FlowSequenceEnd => Kind::FlowSequenceEnd,
            TokenType::Key => Kind::Key {
                explicit: char_at(chars, start) == Some('?')
                    && char_at(chars, start + 1)
                        .is_none_or(|ch| matches!(ch, ' ' | '\t') || is_break(ch)),
            },
            TokenType::Value => Kind::Value,
            TokenType::Anchor(_) | TokenType::Tag(..) => Kind::Property,
            TokenType::Scalar(style, value) => {
                if matches!(style, ScalarStyle::Literal | ScalarStyle::Folded) {
                    // granit starts a block scalar at its content, PyYAML at `|`/`>`.
                    let from = tokens.last().map_or(0, |prev| prev.end);
                    start = block_indicator(chars, from).unwrap_or(start);
                    let end_line_start = line_starts[locate(line_starts, end).0].get();
                    let trailing = end - end_line_start;
                    if count_spaces(chars, end_line_start) >= trailing
                        && trailing <= span.start.col()
                    {
                        end = end_line_start;
                    }
                }
                Kind::Scalar {
                    style,
                    empty: value.is_empty(),
                }
            }
            _ => Kind::Other,
        };
        let (line, column) = locate(line_starts, start);
        let (end_line, end_column) = locate(line_starts, end);
        tokens.push(Token {
            kind,
            start,
            end,
            line,
            column,
            end_line,
            end_column,
        });
    }
    tokens
}

fn locate(line_starts: &[CharPos], idx: usize) -> (usize, usize) {
    let (line, column) = line_and_column(line_starts, CharPos::new(idx));
    (line - 1, column - 1)
}

fn char_at(chars: &[(usize, char)], idx: usize) -> Option<char> {
    chars.get(idx).map(|&(_, ch)| ch)
}

fn count_spaces(chars: &[(usize, char)], from: usize) -> usize {
    chars[from.min(chars.len())..]
        .iter()
        .take_while(|&&(_, ch)| ch == ' ')
        .count()
}

const fn is_break(ch: char) -> bool {
    matches!(ch, '\n' | '\r')
}

fn block_indicator(chars: &[(usize, char)], from: usize) -> Option<usize> {
    let mut in_comment = false;
    let offset = chars[from..].iter().position(|&(_, ch)| {
        in_comment = (in_comment || ch == '#') && !is_break(ch);
        !in_comment && matches!(ch, '|' | '>')
    });
    offset.map(|offset| from + offset)
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
    /// How far a block collection moves, which its block scalars' bodies with an
    /// indentation indicator move with.
    shift: isize,
}

impl Parent {
    const fn new(kind: ParentKind, indent: isize) -> Self {
        Self {
            kind,
            indent,
            line_indent: indent,
            explicit_key: false,
            implicit_block_seq: false,
            shift: 0,
        }
    }
}

struct UnexpectedToken;

struct Analyzer<'a> {
    chars: &'a [(usize, char)],
    line_starts: &'a [CharPos],
    check_multi_line_strings: bool,
    stack: Vec<Parent>,
    cur_line: usize,
    cur_line_indent: isize,
    spaces: Option<isize>,
    indent_sequences: IndentSequencesSetting,
    diagnostics: Vec<Violation>,
    mode: Mode,
    shifts: Vec<Option<Shift>>,
    gaps: Vec<Vec<Gap>>,
}

/// The spaces after an indicator that `ryl format` closes to one, with the rule that owns
/// them.
#[derive(Debug, Clone, Copy)]
struct Gap {
    column: usize,
    removed: usize,
    rule: &'static str,
}

#[derive(Debug, Clone, Copy)]
enum Shift {
    /// A line led by a token found at `found`.
    Token { found: isize, delta: isize },
    /// A line inside a multi-line scalar.
    Carried(isize),
}

impl Shift {
    const fn delta(self) -> isize {
        match self {
            Self::Token { delta, .. } | Self::Carried(delta) => delta,
        }
    }
}

impl<'a> Analyzer<'a> {
    fn new(
        chars: &'a [(usize, char)],
        line_starts: &'a [CharPos],
        cfg: &Config,
        mode: Mode,
    ) -> Self {
        Self {
            chars,
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
            shifts: vec![None; line_starts.len()],
            gaps: vec![Vec::new(); line_starts.len()],
        }
    }

    fn delta(&self, line: usize) -> isize {
        self.shifts[line].map_or(0, Shift::delta)
    }

    fn column(&self, token: &Token) -> isize {
        let closed: usize = self.gaps[token.line]
            .iter()
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
        if gap > 1 && count_spaces(self.chars, token.start + 1) == gap {
            self.gaps[token.line].push(Gap {
                column: token.column,
                removed: gap - 1,
                rule,
            });
        }
    }

    fn run(&mut self, tokens: &[Token]) {
        for (idx, token) in tokens.iter().enumerate() {
            let prev = idx.checked_sub(1).and_then(|prev| tokens.get(prev));
            let next = tokens.get(idx + 1);
            if self.step(token, prev, next, tokens.get(idx + 2)).is_err() {
                self.diagnostics.push(Violation {
                    line: token.line + 1,
                    column: token.column + 1,
                    message: "cannot infer indentation: unexpected token".to_string(),
                });
            }
        }
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
            self.shifts[token.line] = Some(Shift::Token {
                found,
                delta: expected - found,
            });
        }
        if self.mode == Mode::Target {
            self.close_gap(token, next, first_in_line);
        } else if first_in_line {
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

    fn expected(&mut self, token: &Token, found: isize) -> isize {
        let top = self.top();
        match token.kind {
            Kind::FlowMappingEnd | Kind::FlowSequenceEnd => top.line_indent,
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
                    shift: column - to_isize(token.column),
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
            Kind::BlockEntry if !next.is(&[Kind::BlockEntry, Kind::BlockEnd]) => {
                if self.top().kind != ParentKind::BlockSequence {
                    self.stack.push(Parent {
                        implicit_block_seq: true,
                        shift: column - to_isize(token.column),
                        ..Parent::new(ParentKind::BlockSequence, column)
                    });
                }
                let indent =
                    if next.line == token.end_line || next.column == token.column {
                        next_column
                    } else {
                        self.detect_indent(column, next_column)
                    };
                self.stack.push(Parent::new(ParentKind::BlockEntry, indent));
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
        let delta = if !matches!(style, ScalarStyle::Literal | ScalarStyle::Folded) {
            self.column(token) - to_isize(token.column)
        } else if let Some((_, indent)) = self.body_indents(token).next()
            && !self.has_indentation_indicator(token)
        {
            let found = to_isize(indent);
            self.expected_scalar_indent(token, style, found) - found
        } else {
            self.stack
                .iter()
                .rev()
                .find(|parent| {
                    matches!(
                        parent.kind,
                        ParentKind::BlockMapping | ParentKind::BlockSequence
                    )
                })
                .map_or(0, |parent| parent.shift)
        };
        for line in token.line + 1..=last_line(token) {
            self.shifts[line] = Some(Shift::Carried(delta));
        }
    }

    fn has_indentation_indicator(&self, token: &Token) -> bool {
        self.chars[token.start + 1..]
            .iter()
            .map(|&(_, ch)| ch)
            .take_while(|&ch| ch.is_ascii_digit() || matches!(ch, '+' | '-'))
            .any(|ch| ch.is_ascii_digit())
    }

    /// Each later line of `token` that is not blank, with its indent.
    fn body_indents(
        &self,
        token: &Token,
    ) -> impl Iterator<Item = (usize, usize)> + use<'a> {
        let (chars, line_starts) = (self.chars, self.line_starts);
        (token.line + 1..=last_line(token)).filter_map(move |line| {
            let line_start = line_starts[line].get();
            let indent = count_spaces(chars, line_start);
            (!char_at(chars, line_start + indent).is_some_and(is_break))
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
        for pos in (token.start.saturating_sub(1)..token.end).rev() {
            let ch = self.chars[pos].1;
            if !matches!(ch, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c') {
                break;
            }
            if ch == '\n' || (ch == '\r' && char_at(self.chars, pos + 1) != Some('\n'))
            {
                end_line -= 1;
            }
        }
        end_line
    }

    fn push(&mut self, line: usize, found: usize, message: String) {
        self.diagnostics.push(Violation {
            line,
            column: found + 1,
            message,
        });
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
