//! `indentation`: yamllint's token-driven rule on granit's scanner. No safe `--fix`.

use granit_parser::{ScalarStyle, Scanner, StrInput, TokenType};

use crate::config::YamlLintConfig;
use crate::rules::support::punctuation::{build_line_starts, line_and_column};
use crate::rules::support::span_utils::CharPos;

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
        let spaces =
            cfg.rule_option(ID, "spaces")
                .map_or(SpacesSetting::Consistent, |node| {
                    node.as_integer()
                        .map_or(SpacesSetting::Consistent, |value| {
                            let non_negative = value.max(0);
                            let fixed =
                                usize::try_from(non_negative).unwrap_or(usize::MAX);
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
        }
    }

    #[must_use]
    pub const fn new_for_tests(
        spaces: SpacesSetting,
        indent_sequences: IndentSequencesSetting,
        check_multi_line_strings: bool,
    ) -> Self {
        Self {
            spaces,
            indent_sequences,
            check_multi_line_strings,
        }
    }
}

#[must_use]
pub fn check(buffer: &str, cfg: &Config) -> Vec<Violation> {
    let chars: Vec<(usize, char)> = buffer.char_indices().collect();
    let line_starts = build_line_starts(&chars);
    let tokens = scan(buffer, &chars, &line_starts);
    let mut analyzer = Analyzer {
        chars: &chars,
        line_starts: &line_starts,
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
    };
    for (idx, token) in tokens.iter().enumerate() {
        let prev = idx.checked_sub(1).and_then(|prev| tokens.get(prev));
        let next = tokens.get(idx + 1);
        if analyzer
            .step(token, prev, next, tokens.get(idx + 2))
            .is_err()
        {
            analyzer.diagnostics.push(Violation {
                line: token.line + 1,
                column: token.column + 1,
                message: "cannot infer indentation: unexpected token".to_string(),
            });
        }
    }
    analyzer.diagnostics
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
                    if count_spaces(chars, end_line_start) >= end - end_line_start {
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
}

impl Parent {
    const fn new(kind: ParentKind, indent: isize) -> Self {
        Self {
            kind,
            indent,
            line_indent: indent,
            explicit_key: false,
            implicit_block_seq: false,
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
}

impl Analyzer<'_> {
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
        if first_in_line {
            let top = self.top();
            let expected = match token.kind {
                Kind::FlowMappingEnd | Kind::FlowSequenceEnd => top.line_indent,
                Kind::Value => top.indent,
                _ if top.kind == ParentKind::Key && top.explicit_key => {
                    self.detect_indent(top.indent, found)
                }
                _ => top.indent,
            };
            if found != expected {
                let message = if expected < 0 {
                    format!("wrong indentation: expected at least {}", found + 1)
                } else {
                    wrong_indent_message(expected, found)
                };
                self.push(token.line + 1, token.column, message);
            }
        }
        if let Kind::Scalar { style, .. } = token.kind
            && self.check_multi_line_strings
        {
            self.check_scalar(token, style);
        }
        if visible {
            self.cur_line = self.real_end_line(token);
            if first_in_line {
                self.cur_line_indent = found;
            }
        }
        self.update_stack(token, prev, next, nextnext)?;
        self.unwind(token, next)
    }

    fn update_stack(
        &mut self,
        token: &Token,
        prev: Option<&Token>,
        next: Option<&Token>,
        nextnext: Option<&Token>,
    ) -> Result<(), UnexpectedToken> {
        let column = to_isize(token.column);
        let Some(next) = next else {
            return Ok(());
        };
        let next_column = to_isize(next.column);
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
                self.stack.push(Parent::new(kind, column));
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
        let next_column = to_isize(next.column);
        let indent = if key.explicit_key {
            self.detect_indent(key.indent, next_column)
        } else if next.line == prev_line {
            next_column
        } else if next.is(&[Kind::BlockSequenceStart, Kind::BlockEntry]) {
            let flush = next_column == key.indent;
            match self.indent_sequences {
                IndentSequencesSetting::False => key.indent,
                IndentSequencesSetting::True if self.spaces.is_none() && flush => -1,
                IndentSequencesSetting::True => {
                    self.detect_indent(key.indent, next_column)
                }
                setting => {
                    if setting == IndentSequencesSetting::Consistent {
                        self.indent_sequences = if flush {
                            IndentSequencesSetting::False
                        } else {
                            IndentSequencesSetting::True
                        };
                    }
                    if flush {
                        key.indent
                    } else {
                        self.detect_indent(key.indent, next_column)
                    }
                }
            }
        } else {
            self.detect_indent(key.indent, next_column)
        };
        self.stack.push(Parent::new(ParentKind::Value, indent));
        Ok(())
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

    fn check_scalar(&mut self, token: &Token, style: ScalarStyle) {
        let last_line = if token.end_column > 0 {
            token.end_line
        } else {
            token.end_line.saturating_sub(1)
        };
        let mut expected = None;
        for line in token.line + 1..=last_line {
            let line_start = self.line_starts[line].get();
            let indent = count_spaces(self.chars, line_start);
            if char_at(self.chars, line_start + indent).is_some_and(is_break) {
                continue;
            }
            let found = to_isize(indent);
            let expected = *expected.get_or_insert_with(|| {
                self.expected_scalar_indent(token, style, found)
            });
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
        let column = to_isize(token.column);
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

fn wrong_indent_message(expected: isize, found: isize) -> String {
    format!("wrong indentation: expected {expected} but found {found}")
}
