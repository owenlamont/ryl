//! `hyphens`: at most `max-spaces-after` spaces after a block-sequence `-` (default
//! 1). Mirrors yamllint's `hyphens`.
//!
//! Sources: yamllint `hyphens`; YAML 1.2.2 block-sequence grammar; the
//! `dash-on-own-line` option originates in adrienverge/yamllint#527 (spec-style
//! "sequence of mappings" layout), which the maintainer welcomed but yamllint has not
//! implemented.
//!
//! The ryl-only, TOML-only `dash-on-own-line` option (default off) additionally
//! requires a block-sequence entry's `-` to sit on its own line when the entry is a
//! block mapping, so a mapping body is indented *below* the dash rather than starting
//! on it. It flags only an entry whose first mapping key shares the dash's line
//! (`- name: web`); the body-below form (`-` then `  name: web`) is accepted, as is a
//! dash line carrying only node properties (`- &a !tag` with keys below) or a comment.
//! The signal is parser-derived, not a char scan: granit's scanner emits `BlockEntry`
//! then, for a block-mapping entry, `BlockMappingStart`; when both land on the same
//! line the mapping opened on the dash line. Non-mapping entries (scalars, aliases,
//! nested sequences, flow values) emit a different value token and a block mapping that
//! is a *mapping* value (no preceding `BlockEntry`) is never reported.
//!
//! No safe `--fix`: collapsing the spaces shifts the indent of any nested block that
//! follows (`max-spaces-after`), and breaking the dash onto its own line re-indents the
//! mapping body (`dash-on-own-line`) — both can change the parsed structure.

use granit_parser::{Scanner, StrInput, TokenType};

use crate::config::YamlLintConfig;
use crate::rules::support::line_syntax::split_lines_preserve_endings;
use crate::rules::support::punctuation::{build_line_starts, line_and_column};
use crate::rules::support::span_utils::CharPos;

pub const ID: &str = "hyphens";
pub const MESSAGE: &str = "too many spaces after hyphen";
pub const MESSAGE_DASH_ON_OWN_LINE: &str =
    "block mapping should start on a new line after the hyphen";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    max_spaces_after: i64,
    dash_on_own_line: bool,
}

impl Config {
    const DEFAULT_MAX: i64 = 1;

    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        Self {
            max_spaces_after: cfg.rule_option_int(
                ID,
                "max-spaces-after",
                Self::DEFAULT_MAX,
            ),
            dash_on_own_line: cfg.rule_option_bool(ID, "dash-on-own-line", false),
        }
    }

    #[must_use]
    pub const fn new_for_tests(max_spaces_after: i64) -> Self {
        Self {
            max_spaces_after,
            dash_on_own_line: false,
        }
    }

    #[must_use]
    pub const fn with_dash_on_own_line(mut self, value: bool) -> Self {
        self.dash_on_own_line = value;
        self
    }

    #[must_use]
    pub const fn max_spaces_after(&self) -> i64 {
        self.max_spaces_after
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

#[must_use]
pub fn check(buffer: &str, cfg: &Config) -> Vec<Violation> {
    let mut violations = collect_max_spaces(buffer, cfg.max_spaces_after);
    if cfg.dash_on_own_line {
        violations.extend(collect_dash_on_own_line(buffer));
        // Two independent passes append out of document order; restore it (no later
        // per-rule sort exists in `lint`, and yamllint reports a file in line/col order).
        violations.sort_by_key(|a| (a.line, a.column));
    }
    violations
}

fn collect_max_spaces(buffer: &str, max_spaces_after: i64) -> Vec<Violation> {
    let mut violations = Vec::new();

    for (idx, line, _ending) in split_lines_preserve_endings(buffer) {
        if line.is_empty() {
            continue;
        }

        let chars = line.char_indices();
        let mut indent_chars = 0usize;
        let mut hyphen_byte = None;

        for (byte_idx, ch) in chars {
            match ch {
                ' ' | '\t' => {
                    indent_chars += 1;
                }
                '-' => {
                    hyphen_byte = Some(byte_idx);
                    break;
                }
                _ => break,
            }
        }

        let Some(hyphen_pos) = hyphen_byte else {
            continue;
        };

        let mut offset = hyphen_pos + 1;
        let mut spaces_after = 0usize;

        while let Some(ch) = line[offset..].chars().next() {
            if matches!(ch, ' ' | '\t') {
                spaces_after += 1;
                offset += ch.len_utf8();
            } else {
                break;
            }
        }

        if offset >= line.len() {
            continue;
        }

        let next_byte = line.as_bytes()[offset];
        if next_byte == b'#' {
            continue;
        }

        let spaces_count = i64::try_from(spaces_after).unwrap_or(i64::MAX);

        if spaces_count > max_spaces_after {
            let column = indent_chars + 1 + spaces_after;
            violations.push(Violation {
                line: idx + 1,
                column,
                message: MESSAGE.to_string(),
            });
        }
    }

    violations
}

/// Flag every block-sequence entry whose block mapping opens on the dash's line.
///
/// granit's scanner emits `BlockEntry` (the `-`), optional node properties
/// (`Anchor`/`Tag`/`Comment`), then the entry's value token. A `BlockMappingStart` on
/// the same line as its `BlockEntry` means the mapping began on the dash line; any
/// other value token (scalar, alias, nested `BlockSequenceStart`, flow `*Start`) is a
/// non-mapping entry and clears the pending dash. Positions go through ryl's CR-aware
/// `line_and_column` so a bare `\r` keeps the reported span in bounds. The scanner is a
/// lexer, so unparsable input simply yields the tokens it can — no panic.
fn collect_dash_on_own_line(buffer: &str) -> Vec<Violation> {
    let char_indices: Vec<(usize, char)> = buffer.char_indices().collect();
    let line_starts = build_line_starts(&char_indices);
    let mut violations = Vec::new();
    let mut dash_line: Option<usize> = None;

    for token in Scanner::new(StrInput::new(buffer)) {
        match token.1 {
            TokenType::BlockEntry => {
                let (line, _) =
                    line_and_column(&line_starts, CharPos::new(token.0.start.index()));
                dash_line = Some(line);
            }
            // Node properties decorate the entry's value without ending the dash.
            TokenType::Anchor(_) | TokenType::Tag(..) | TokenType::Comment(_) => {}
            TokenType::BlockMappingStart => {
                if let Some(dash) = dash_line.take() {
                    let (line, column) = line_and_column(
                        &line_starts,
                        CharPos::new(token.0.start.index()),
                    );
                    if line == dash {
                        violations.push(Violation {
                            line,
                            column,
                            message: MESSAGE_DASH_ON_OWN_LINE.to_string(),
                        });
                    }
                }
            }
            _ => dash_line = None,
        }
    }

    violations
}
