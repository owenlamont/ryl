use saphyr_parser::{Event, Parser, ScalarStyle, Span, SpannedEventReceiver};

use crate::config::YamlLintConfig;
use crate::rules::span_utils::span_char_index_to_byte;

pub const ID: &str = "float-values";

#[derive(Debug, Clone)]
pub struct Config {
    flags: u8,
}

const REQUIRE_NUMERAL_FLAG: u8 = 1 << 0;
const FORBID_SCIENTIFIC_FLAG: u8 = 1 << 1;
const FORBID_NAN_FLAG: u8 = 1 << 2;
const FORBID_INF_FLAG: u8 = 1 << 3;

impl Config {
    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        let require_numeral_before_decimal = cfg
            .rule_option(ID, "require-numeral-before-decimal")
            .and_then(saphyr::YamlOwned::as_bool)
            .unwrap_or(false);

        let forbid_scientific_notation = cfg
            .rule_option(ID, "forbid-scientific-notation")
            .and_then(saphyr::YamlOwned::as_bool)
            .unwrap_or(false);

        let forbid_nan = cfg
            .rule_option(ID, "forbid-nan")
            .and_then(saphyr::YamlOwned::as_bool)
            .unwrap_or(false);

        let forbid_inf = cfg
            .rule_option(ID, "forbid-inf")
            .and_then(saphyr::YamlOwned::as_bool)
            .unwrap_or(false);

        let mut flags = 0u8;
        if require_numeral_before_decimal {
            flags |= REQUIRE_NUMERAL_FLAG;
        }
        if forbid_scientific_notation {
            flags |= FORBID_SCIENTIFIC_FLAG;
        }
        if forbid_nan {
            flags |= FORBID_NAN_FLAG;
        }
        if forbid_inf {
            flags |= FORBID_INF_FLAG;
        }

        Self { flags }
    }

    const fn require_numeral_before_decimal(&self) -> bool {
        (self.flags & REQUIRE_NUMERAL_FLAG) != 0
    }

    const fn forbid_scientific_notation(&self) -> bool {
        (self.flags & FORBID_SCIENTIFIC_FLAG) != 0
    }

    const fn forbid_nan(&self) -> bool {
        (self.flags & FORBID_NAN_FLAG) != 0
    }

    const fn forbid_inf(&self) -> bool {
        (self.flags & FORBID_INF_FLAG) != 0
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
    let mut parser = Parser::new_from_str(buffer);
    let mut receiver = FloatValuesReceiver::new(cfg, buffer);
    let _ = parser.load(&mut receiver, true);
    receiver.diagnostics
}

struct FloatValuesReceiver<'cfg, 'input> {
    config: &'cfg Config,
    buffer: &'input str,
    chars: Vec<(usize, char)>,
    buffer_len: usize,
    diagnostics: Vec<Violation>,
}

impl<'cfg, 'input> FloatValuesReceiver<'cfg, 'input> {
    fn new(config: &'cfg Config, buffer: &'input str) -> Self {
        Self {
            config,
            buffer,
            chars: buffer.char_indices().collect(),
            buffer_len: buffer.len(),
            diagnostics: Vec::new(),
        }
    }

    fn handle_scalar(&mut self, value: &str, span: Span) {
        let line = span.start.line();
        let column = span.start.col() + 1;

        if self.config.forbid_nan() && is_nan(value) {
            self.diagnostics.push(Violation {
                line,
                column,
                message: format!(
                    "forbidden not a number value \"{}\"",
                    self.original_scalar(span, value)
                ),
            });
        }

        if self.config.forbid_inf() && is_inf(value) {
            self.diagnostics.push(Violation {
                line,
                column,
                message: format!(
                    "forbidden infinite value \"{}\"",
                    self.original_scalar(span, value)
                ),
            });
        }

        if self.config.forbid_scientific_notation() && is_scientific_notation(value) {
            self.diagnostics.push(Violation {
                line,
                column,
                message: format!(
                    "forbidden scientific notation \"{}\"",
                    self.original_scalar(span, value)
                ),
            });
        }

        if self.config.require_numeral_before_decimal() && is_missing_numeral_before_decimal(value)
        {
            self.diagnostics.push(Violation {
                line,
                column,
                message: format!(
                    "forbidden decimal missing 0 prefix \"{}\"",
                    self.original_scalar(span, value)
                ),
            });
        }
    }

    fn original_scalar<'a>(&'a self, span: Span, fallback: &'a str) -> &'a str {
        let start_char = span.start.index();
        let end_char = span.end.index();
        let start = span_char_index_to_byte(&self.chars, start_char, self.buffer_len);
        let end = span_char_index_to_byte(&self.chars, end_char, self.buffer_len);
        let range_start = start.min(end);
        let range_end = start.max(end);
        self.buffer.get(range_start..range_end).unwrap_or(fallback)
    }
}

impl<'input> SpannedEventReceiver<'input> for FloatValuesReceiver<'_, 'input> {
    fn on_event(&mut self, event: Event<'input>, span: Span) {
        if let Event::Scalar(value, style, _, tag) = event {
            if tag.is_some() || !matches!(style, ScalarStyle::Plain) {
                return;
            }
            self.handle_scalar(value.as_ref(), span);
        }
    }
}

fn is_nan(value: &str) -> bool {
    matches!(value, ".nan" | ".NaN" | ".NAN")
}

fn is_inf(value: &str) -> bool {
    let trimmed = without_sign(value);
    matches!(trimmed, ".inf" | ".Inf" | ".INF")
}

fn is_scientific_notation(value: &str) -> bool {
    let trimmed = without_sign(value);
    let Some((mantissa, exponent)) = split_exponent(trimmed) else {
        return false;
    };
    if !is_valid_exponent(exponent) {
        return false;
    }
    if let Some(digits) = mantissa.strip_prefix('.') {
        return !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit());
    }

    if mantissa.is_empty() {
        return false;
    }

    let mut parts = mantissa.splitn(2, '.');
    let int_part = parts.next().unwrap();
    if int_part.is_empty() || !int_part.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    if let Some(frac_part) = parts.next() {
        return frac_part.chars().all(|c| c.is_ascii_digit());
    }
    true
}

fn is_missing_numeral_before_decimal(value: &str) -> bool {
    let trimmed = without_sign(value);
    if !trimmed.starts_with('.') {
        return false;
    }
    let after_dot = &trimmed[1..];
    if after_dot.is_empty() {
        return false;
    }

    let (digits, exponent) = match split_exponent(after_dot) {
        Some((mantissa, exponent)) => (mantissa, Some(exponent)),
        None => (after_dot, None),
    };

    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }

    exponent.is_none_or(is_valid_exponent)
}

fn split_exponent(value: &str) -> Option<(&str, &str)> {
    let idx = value.find(['e', 'E'])?;
    Some(value.split_at(idx))
}

fn is_valid_exponent(exponent: &str) -> bool {
    let rest = exponent
        .strip_prefix('e')
        .or_else(|| exponent.strip_prefix('E'))
        .expect("exponent fragment must start with e/E");
    let rest = without_sign(rest);
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
}

fn without_sign(value: &str) -> &str {
    value
        .strip_prefix('+')
        .or_else(|| value.strip_prefix('-'))
        .unwrap_or(value)
}
