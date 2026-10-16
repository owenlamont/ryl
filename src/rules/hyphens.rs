//! `hyphens`: at most `max-spaces-after` spaces after a block-sequence `-` (default
//! 1). Mirrors yamllint's `hyphens`. The ryl-only, TOML-only `dash-on-own-line` option
//! (default off) additionally flags a block-mapping entry whose first key shares the
//! dash's line (`- name: web`); the body-below form, a dash line with only node
//! properties (`- &a !tag`), or a comment is accepted.
//!
//! `dash-on-own-line` is parser-derived, not a char scan: granit's scanner emits
//! `BlockEntry` then, for a block-mapping entry, `BlockMappingStart`; both on the same
//! line means the mapping opened on the dash line. Any other value token is a
//! non-mapping entry, and a block mapping that is a *mapping* value (no preceding
//! `BlockEntry`) is never reported.
//!
//! Safe `--fix` collapses the spaces to the tolerance, never below one, except after a
//! dash opening a compact block collection that continues below, whose indentation the
//! spaces set. `dash-on-own-line` has no fix. `ryl format` closes those gaps and joins or
//! breaks dash-line mappings as part of re-indenting (`indentation::reindent`).
//!
//! Sources: YAML 1.2.2 block-sequence grammar; adrienverge/yamllint#527.

use granit_parser::{Scanner, StrInput, TokenType};

use crate::config::YamlLintConfig;
use crate::rules::support::punctuation::{build_line_starts, line_and_column};
use crate::rules::support::span_utils::CharPos;
use crate::rules::support::token_spacing::{self, Fix, Indicator, Mode};

pub const ID: &str = "hyphens";
pub const MESSAGE: &str = "too many spaces after hyphen";
pub const MESSAGE_DASH_ON_OWN_LINE: &str =
    "block mapping should start on a new line after the hyphen";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    max_spaces_after: i64,
    dash_on_own_line: bool,
    mode: Mode,
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
            mode: Mode::Lint,
        }
    }

    #[must_use]
    pub const fn new(max_spaces_after: i64) -> Self {
        Self {
            max_spaces_after,
            dash_on_own_line: false,
            mode: Mode::Lint,
        }
    }

    /// The formatter's target: exactly one space after `-`.
    #[must_use]
    pub const fn format() -> Self {
        Self {
            mode: Mode::Format,
            ..Self::new(1)
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
    let mut violations = violations(buffer, cfg, |fix| {
        cfg.mode == Mode::Lint || fix == Fix::Safe
    });
    if cfg.dash_on_own_line {
        violations.extend(collect_dash_on_own_line(buffer));
        // Two independent passes append out of document order; restore it (no later
        // per-rule sort exists in `lint`, and yamllint reports a file in line/col order).
        violations.sort_by_key(|a| (a.line, a.column));
    }
    violations
}

#[must_use]
pub fn fix(buffer: &str, cfg: &Config) -> Option<String> {
    token_spacing::fix(buffer, cfg.mode, |site| {
        (site.indicator == Indicator::Dash).then_some(cfg.max_spaces_after)
    })
}

/// The violations `fix` leaves because respacing them would re-indent a collection.
#[must_use]
pub fn unfixed(buffer: &str, cfg: &Config) -> Vec<Violation> {
    violations(buffer, cfg, |fix| fix == Fix::Reindents)
}

fn violations(
    buffer: &str,
    cfg: &Config,
    keep: impl Fn(Fix) -> bool,
) -> Vec<Violation> {
    token_spacing::sites(buffer)
        .into_iter()
        .filter(|site| {
            site.indicator == Indicator::Dash
                && keep(site.fix)
                && site.exceeds(cfg.max_spaces_after)
        })
        .map(|site| Violation {
            line: site.line,
            column: site.column,
            message: MESSAGE.to_string(),
        })
        .collect()
}

/// Flag every block-sequence entry whose block mapping opens on the dash's line (see
/// module header for the token mechanics). The scanner is a lexer, so unparsable input
/// just yields the tokens it can, no panic.
fn collect_dash_on_own_line(buffer: &str) -> Vec<Violation> {
    let char_indices: Vec<(usize, char)> = buffer.char_indices().collect();
    let line_starts = build_line_starts(&char_indices);
    let mut violations = Vec::new();
    let mut dash_line: Option<usize> = None;

    for token in Scanner::new(StrInput::new(buffer)).map_while(Result::ok) {
        let (span, token_type) = token.into_parts();
        match token_type {
            TokenType::BlockEntry => {
                let (line, _) =
                    line_and_column(&line_starts, CharPos::new(span.start.index()));
                dash_line = Some(line);
            }
            // Node properties decorate the entry's value without ending the dash.
            TokenType::Anchor(_) | TokenType::Tag(..) | TokenType::Comment(_) => {}
            TokenType::BlockMappingStart => {
                if let Some(dash) = dash_line.take() {
                    let (line, column) =
                        line_and_column(&line_starts, CharPos::new(span.start.index()));
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
