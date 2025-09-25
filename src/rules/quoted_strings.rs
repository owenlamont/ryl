use regex::Regex;
use saphyr::Yaml;
use saphyr_parser::{Event, Parser, ScalarStyle, Span, SpannedEventReceiver, Tag};

use crate::config::YamlLintConfig;

pub const ID: &str = "quoted-strings";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuoteType {
    Any,
    Single,
    Double,
}

impl QuoteType {
    const fn matches(self, style: Option<QuoteStyle>) -> bool {
        match self {
            Self::Any => style.is_some(),
            Self::Single => matches!(style, Some(QuoteStyle::Single)),
            Self::Double => matches!(style, Some(QuoteStyle::Double)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuoteStyle {
    Single,
    Double,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequiredMode {
    Always,
    Never,
    OnlyWhenNeeded,
}

#[derive(Debug, Clone)]
pub struct Config {
    quote_type: QuoteType,
    quote_type_label: &'static str,
    required: RequiredMode,
    extra_required: Vec<Regex>,
    extra_allowed: Vec<Regex>,
    allow_quoted_quotes: bool,
    pub check_keys: bool,
}

impl Config {
    /// Resolve the rule configuration from the parsed yamllint configuration.
    ///
    /// # Panics
    ///
    /// Panics when option types are invalid. Configuration parsing validates
    /// options before resolution, so this only occurs when constructing configs
    /// manually in tests.
    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        let (quote_type, quote_type_label) = match cfg.rule_option_str(ID, "quote-type") {
            Some("single") => (QuoteType::Single, "single"),
            Some("double") => (QuoteType::Double, "double"),
            _ => (QuoteType::Any, "any"),
        };

        let required = cfg
            .rule_option(ID, "required")
            .map_or(RequiredMode::Always, |node| {
                if node.as_bool() == Some(false) {
                    RequiredMode::Never
                } else if node.as_str() == Some("only-when-needed") {
                    RequiredMode::OnlyWhenNeeded
                } else {
                    RequiredMode::Always
                }
            });

        let mut extra_required: Vec<Regex> = Vec::new();
        if let Some(node) = cfg.rule_option(ID, "extra-required")
            && let Some(seq) = node.as_sequence()
        {
            for item in seq {
                let pattern = item
                    .as_str()
                    .expect("quoted-strings extra-required entries should be strings");
                let regex = Regex::new(pattern)
                    .expect("quoted-strings extra-required should contain valid regex");
                extra_required.push(regex);
            }
        }

        let mut extra_allowed: Vec<Regex> = Vec::new();
        if let Some(node) = cfg.rule_option(ID, "extra-allowed")
            && let Some(seq) = node.as_sequence()
        {
            for item in seq {
                let pattern = item
                    .as_str()
                    .expect("quoted-strings extra-allowed entries should be strings");
                let regex = Regex::new(pattern)
                    .expect("quoted-strings extra-allowed should contain valid regex");
                extra_allowed.push(regex);
            }
        }

        let allow_quoted_quotes = cfg
            .rule_option(ID, "allow-quoted-quotes")
            .and_then(saphyr::YamlOwned::as_bool)
            .unwrap_or(false);

        let check_keys = cfg
            .rule_option(ID, "check-keys")
            .and_then(saphyr::YamlOwned::as_bool)
            .unwrap_or(false);

        Self {
            quote_type,
            quote_type_label,
            required,
            extra_required,
            extra_allowed,
            allow_quoted_quotes,
            check_keys,
        }
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
    let mut receiver = QuotedStringsReceiver::new(cfg, buffer);
    let _ = parser.load(&mut receiver, true);
    receiver.diagnostics
}

struct QuotedStringsReceiver<'cfg> {
    state: QuotedStringsState<'cfg>,
    diagnostics: Vec<Violation>,
}

impl<'cfg> QuotedStringsReceiver<'cfg> {
    const fn new(cfg: &'cfg Config, buffer: &'cfg str) -> Self {
        Self {
            state: QuotedStringsState::new(cfg, buffer),
            diagnostics: Vec::new(),
        }
    }
}

impl SpannedEventReceiver<'_> for QuotedStringsReceiver<'_> {
    fn on_event(&mut self, event: Event<'_>, span: Span) {
        match event {
            Event::StreamStart => self.state.reset_stream(),
            Event::DocumentStart(_) => self.state.document_start(),
            Event::DocumentEnd => self.state.document_end(),
            Event::SequenceStart(_, _) => {
                let flow = is_flow_sequence(self.state.buffer, span);
                self.state.enter_sequence(flow);
            }
            Event::SequenceEnd | Event::MappingEnd => self.state.exit_container(),
            Event::MappingStart(_, _) => {
                let flow = is_flow_mapping(self.state.buffer, span);
                self.state.enter_mapping(flow);
            }
            Event::Scalar(value, style, _, tag) => {
                self.state.handle_scalar(
                    style,
                    value.as_ref(),
                    tag.as_deref(),
                    span,
                    &mut self.diagnostics,
                );
            }
            Event::Alias(_) | Event::StreamEnd | Event::Nothing => {}
        }
    }
}

struct QuotedStringsState<'cfg> {
    config: &'cfg Config,
    buffer: &'cfg str,
    containers: Vec<ContainerState>,
    key_depth: usize,
}

impl<'cfg> QuotedStringsState<'cfg> {
    const fn new(config: &'cfg Config, buffer: &'cfg str) -> Self {
        Self {
            config,
            buffer,
            containers: Vec::new(),
            key_depth: 0,
        }
    }

    fn reset_stream(&mut self) {
        self.containers.clear();
        self.key_depth = 0;
    }

    fn document_start(&mut self) {
        self.containers.clear();
        self.key_depth = 0;
    }

    fn document_end(&mut self) {
        self.containers.clear();
        self.key_depth = 0;
    }

    fn enter_mapping(&mut self, flow: bool) {
        let active_key = self.begin_node();
        self.containers.push(ContainerState {
            kind: ContainerKind::Mapping { expect_key: true },
            key_context: active_key,
            flow,
        });
    }

    fn enter_sequence(&mut self, flow: bool) {
        let active_key = self.begin_node();
        self.containers.push(ContainerState {
            kind: ContainerKind::Sequence,
            key_context: active_key,
            flow,
        });
    }

    fn exit_container(&mut self) {
        if let Some(container) = self.containers.pop()
            && container.key_context
            && self.key_depth > 0
        {
            self.key_depth -= 1;
        }
    }

    fn handle_scalar(
        &mut self,
        style: ScalarStyle,
        value: &str,
        tag: Option<&Tag>,
        span: Span,
        diagnostics: &mut Vec<Violation>,
    ) {
        let active_key = self.begin_node();
        let resolves_to_string = matches!(
            Yaml::value_from_str(value),
            Yaml::Value(saphyr::Scalar::String(_))
        );

        if self.should_skip_scalar(style, tag, active_key, resolves_to_string) {
            self.finish_scalar(active_key);
            return;
        }

        if let Some(violation) =
            self.evaluate_scalar(style, value, active_key, resolves_to_string, span)
        {
            diagnostics.push(violation);
        }

        self.finish_scalar(active_key);
    }

    fn in_flow(&self) -> bool {
        self.containers.iter().any(|container| container.flow)
    }

    fn begin_node(&mut self) -> bool {
        let mut is_key_node = false;
        if let Some(ContainerState {
            kind: ContainerKind::Mapping { expect_key },
            ..
        }) = self.containers.last_mut()
        {
            if *expect_key {
                is_key_node = true;
                *expect_key = false;
            } else {
                *expect_key = true;
            }
        }

        let active_key = is_key_node || self.key_depth > 0;
        if active_key {
            self.key_depth += 1;
        }
        active_key
    }

    const fn finish_scalar(&mut self, active_key: bool) {
        if active_key && self.key_depth > 0 {
            self.key_depth -= 1;
        }
    }

    fn should_skip_scalar(
        &self,
        style: ScalarStyle,
        tag: Option<&Tag>,
        active_key: bool,
        resolves_to_string: bool,
    ) -> bool {
        if matches!(style, ScalarStyle::Literal | ScalarStyle::Folded) {
            return true;
        }

        if active_key && !self.config.check_keys {
            return true;
        }

        if let Some(tag) = tag
            && is_core_tag(tag)
        {
            return true;
        }

        matches!(style, ScalarStyle::Plain) && !resolves_to_string
    }

    fn evaluate_scalar(
        &self,
        style: ScalarStyle,
        value: &str,
        active_key: bool,
        resolves_to_string: bool,
        span: Span,
    ) -> Option<Violation> {
        let node_label = if active_key { "key" } else { "value" };
        let quote_style = match style {
            ScalarStyle::SingleQuoted => Some(QuoteStyle::Single),
            ScalarStyle::DoubleQuoted => Some(QuoteStyle::Double),
            ScalarStyle::Plain | ScalarStyle::Literal | ScalarStyle::Folded => None,
        };

        let has_quoted_quotes = match style {
            ScalarStyle::SingleQuoted => value.contains('"'),
            ScalarStyle::DoubleQuoted => value.contains('\''),
            _ => false,
        };

        let extra_required = self
            .config
            .extra_required
            .iter()
            .any(|re| re.is_match(value));
        let extra_allowed = self
            .config
            .extra_allowed
            .iter()
            .any(|re| re.is_match(value));
        let quotes_needed = matches!(style, ScalarStyle::SingleQuoted | ScalarStyle::DoubleQuoted)
            && quotes_are_needed(style, value, self.in_flow(), self.buffer, span);

        let message = match self.config.required {
            RequiredMode::Always => {
                if quote_style.is_none()
                    || quote_style.is_some_and(|style_kind| {
                        self.mismatched_quote(style_kind, has_quoted_quotes)
                    })
                {
                    Some(format!(
                        "string {node_label} is not quoted with {} quotes",
                        self.config.quote_type_label
                    ))
                } else {
                    None
                }
            }
            RequiredMode::Never => quote_style.map_or_else(
                || {
                    if extra_required {
                        Some(format!("string {node_label} is not quoted"))
                    } else {
                        None
                    }
                },
                |style_kind| {
                    if self.mismatched_quote(style_kind, has_quoted_quotes) {
                        Some(format!(
                            "string {node_label} is not quoted with {} quotes",
                            self.config.quote_type_label
                        ))
                    } else {
                        None
                    }
                },
            ),
            RequiredMode::OnlyWhenNeeded => quote_style.map_or_else(
                || {
                    if extra_required {
                        Some(format!("string {node_label} is not quoted"))
                    } else {
                        None
                    }
                },
                |style_kind| {
                    if resolves_to_string && !value.is_empty() && !quotes_needed {
                        if extra_required || extra_allowed {
                            None
                        } else {
                            Some(format!(
                                "string {node_label} is redundantly quoted with {} quotes",
                                self.config.quote_type_label
                            ))
                        }
                    } else if self.mismatched_quote(style_kind, has_quoted_quotes) {
                        Some(format!(
                            "string {node_label} is not quoted with {} quotes",
                            self.config.quote_type_label
                        ))
                    } else {
                        None
                    }
                },
            ),
        }?;

        Some(build_violation(span, message))
    }

    const fn mismatched_quote(&self, style_kind: QuoteStyle, has_quoted_quotes: bool) -> bool {
        !(self.config.quote_type.matches(Some(style_kind))
            || (self.config.allow_quoted_quotes && has_quoted_quotes))
    }
}

struct ContainerState {
    kind: ContainerKind,
    key_context: bool,
    flow: bool,
}

enum ContainerKind {
    Mapping { expect_key: bool },
    Sequence,
}

fn build_violation(span: Span, message: String) -> Violation {
    Violation {
        line: span.start.line(),
        column: span.start.col() + 1,
        message,
    }
}

fn is_flow_sequence(buffer: &str, span: Span) -> bool {
    matches!(char_at(buffer, span.start.index()), Some('['))
}

fn is_flow_mapping(buffer: &str, span: Span) -> bool {
    matches!(char_at(buffer, span.start.index()), Some('{'))
}

fn char_at(text: &str, idx: usize) -> Option<char> {
    text.chars().nth(idx)
}

fn is_core_tag(tag: &Tag) -> bool {
    tag.handle == "tag:yaml.org,2002:"
}

fn quotes_are_needed(
    style: ScalarStyle,
    value: &str,
    is_inside_flow: bool,
    buffer: &str,
    span: Span,
) -> bool {
    if is_inside_flow
        && value
            .chars()
            .any(|c| matches!(c, ',' | '[' | ']' | '{' | '}'))
    {
        return true;
    }

    if matches!(style, ScalarStyle::DoubleQuoted) {
        if contains_non_printable(value) {
            return true;
        }
        if has_backslash_line_ending(buffer, span) {
            return true;
        }
    }

    plain_scalar_equivalent(value).is_none_or(|result| !result)
}

fn plain_scalar_equivalent(value: &str) -> Option<bool> {
    let snippet = format!("key: {value}\n");
    let mut parser = Parser::new_from_str(&snippet);
    let mut checker = PlainScalarChecker::new(value);
    if parser.load(&mut checker, true).is_err() {
        return Some(false);
    }
    checker.result.or(Some(false))
}

struct PlainScalarChecker<'a> {
    expected: &'a str,
    seen_key: bool,
    result: Option<bool>,
}

impl<'a> PlainScalarChecker<'a> {
    const fn new(expected: &'a str) -> Self {
        Self {
            expected,
            seen_key: false,
            result: None,
        }
    }
}

impl SpannedEventReceiver<'_> for PlainScalarChecker<'_> {
    fn on_event(&mut self, event: Event<'_>, _span: Span) {
        if let Event::Scalar(value, style, _, _) = event {
            if !self.seen_key {
                self.seen_key = true;
            } else if self.result.is_none() {
                let is_plain_scalar = matches!(style, ScalarStyle::Plain);
                let matches_expected = value.as_ref() == self.expected;
                self.result = Some(is_plain_scalar && matches_expected);
            }
        }
    }
}

fn contains_non_printable(value: &str) -> bool {
    value.chars().any(|ch| {
        let code = ch as u32;
        !(matches!(ch, '\u{9}' | '\u{A}' | '\u{D}')
            || (0x20..=0x7E).contains(&code)
            || code == 0x85
            || (0xA0..=0xD7FF).contains(&code)
            || (0xE000..=0xFFFD).contains(&code)
            || (0x1_0000..=0x10_FFFF).contains(&code))
    })
}

fn has_backslash_line_ending(buffer: &str, span: Span) -> bool {
    if span.start.line() == span.end.line() {
        return false;
    }

    let slice_start = span.start.index().saturating_add(1).min(buffer.len());
    let mut slice_end = span.end.index().saturating_sub(1);
    slice_end = slice_end.min(buffer.len());
    slice_end = slice_end.max(slice_start);
    let content = &buffer[slice_start..slice_end];
    let has_unix_backslash = content.contains("\\\n");
    let has_windows_backslash = content.contains("\\\r\n");
    has_unix_backslash || has_windows_backslash
}
