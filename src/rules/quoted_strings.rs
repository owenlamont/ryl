use regex::Regex;
use saphyr::Yaml;
use saphyr_parser::{Event, Parser, ScalarStyle, Span, SpannedEventReceiver, Tag};

use crate::config::YamlLintConfig;
use crate::rules::support::mapping_key_walker::Walker;

pub const ID: &str = "quoted-strings";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuoteType {
    Any,
    Single,
    Double,
    Consistent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuoteStyle {
    Single,
    Double,
}

#[derive(Debug, Clone, Copy)]
struct ScalarQuoteFacts {
    style: Option<QuoteStyle>,
    has_quoted_quotes: Flag,
    has_double_quote_escape: Flag,
    extra_required: Flag,
    extra_allowed: Flag,
    quotes_needed: Flag,
}

#[derive(Debug, Clone, Copy)]
struct Flag(bool);

impl Flag {
    const fn new(value: bool) -> Self {
        Self(value)
    }

    const fn get(self) -> bool {
        self.0
    }
}

fn quote_style(style: ScalarStyle) -> Option<QuoteStyle> {
    match style {
        ScalarStyle::SingleQuoted => Some(QuoteStyle::Single),
        ScalarStyle::DoubleQuoted => Some(QuoteStyle::Double),
        ScalarStyle::Plain | ScalarStyle::Literal | ScalarStyle::Folded => None,
    }
}

fn quoted_scalar_contains_opposite_quote(style: ScalarStyle, value: &str) -> bool {
    match style {
        ScalarStyle::SingleQuoted => value.contains('"'),
        ScalarStyle::DoubleQuoted => value.contains('\''),
        _ => false,
    }
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
    allow_double_quotes_for_escaping: bool,
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
        let (quote_type, quote_type_label) = match cfg.rule_option_str(ID, "quote-type")
        {
            Some("single") => (QuoteType::Single, "single"),
            Some("double") => (QuoteType::Double, "double"),
            Some("consistent") => (QuoteType::Consistent, "consistent"),
            _ => (QuoteType::Any, "any"),
        };

        let required =
            cfg.rule_option(ID, "required")
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

        let allow_double_quotes_for_escaping = cfg
            .rule_option(ID, "allow-double-quotes-for-escaping")
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
            allow_double_quotes_for_escaping,
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
    walker: Walker<(), bool>,
    consistent_quote_style: Option<QuoteStyle>,
}

impl<'cfg> QuotedStringsState<'cfg> {
    const fn new(config: &'cfg Config, buffer: &'cfg str) -> Self {
        Self {
            config,
            buffer,
            walker: Walker::new(),
            consistent_quote_style: None,
        }
    }

    fn reset_stream(&mut self) {
        self.walker.reset();
        self.consistent_quote_style = None;
    }

    fn document_start(&mut self) {
        self.walker.reset();
        self.consistent_quote_style = None;
    }

    fn document_end(&mut self) {
        self.walker.reset();
        self.consistent_quote_style = None;
    }

    fn enter_mapping(&mut self, flow: bool) {
        self.walker.enter_mapping((), flow);
    }

    fn enter_sequence(&mut self, flow: bool) {
        self.walker.enter_sequence(flow);
    }

    fn exit_container(&mut self) {
        self.walker.exit_container();
    }

    fn handle_scalar(
        &mut self,
        style: ScalarStyle,
        value: &str,
        tag: Option<&Tag>,
        span: Span,
        diagnostics: &mut Vec<Violation>,
    ) {
        let context = self.walker.begin_node();
        let active_key = context.active();
        let resolves_to_string = matches!(
            Yaml::value_from_str(value),
            Yaml::Value(saphyr::Scalar::String(_))
        );

        if self.should_skip_scalar(style, tag, active_key, resolves_to_string) {
            self.walker.finish_node(context);
            return;
        }

        if let Some(violation) =
            self.evaluate_scalar(style, value, active_key, resolves_to_string, span)
        {
            diagnostics.push(violation);
        }

        self.walker.finish_node(context);
    }

    fn in_flow(&self) -> bool {
        self.walker.any_metadata(|flow| *flow)
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
        &mut self,
        style: ScalarStyle,
        value: &str,
        active_key: bool,
        resolves_to_string: bool,
        span: Span,
    ) -> Option<Violation> {
        let node_label = if active_key { "key" } else { "value" };
        let facts = self.scalar_quote_facts(style, value, span);

        let message = match self.config.required {
            RequiredMode::Always => self.required_always_message(node_label, facts),
            RequiredMode::Never => self.required_never_message(node_label, facts),
            RequiredMode::OnlyWhenNeeded => self.only_when_needed_message(
                node_label,
                value,
                resolves_to_string,
                facts,
            ),
        }?;

        Some(build_violation(span, message))
    }

    fn scalar_quote_facts(
        &self,
        style: ScalarStyle,
        value: &str,
        span: Span,
    ) -> ScalarQuoteFacts {
        ScalarQuoteFacts {
            style: quote_style(style),
            has_quoted_quotes: Flag::new(quoted_scalar_contains_opposite_quote(
                style, value,
            )),
            has_double_quote_escape: Flag::new(
                self.has_escaping_in_double_quotes(style, span),
            ),
            extra_required: Flag::new(
                self.config
                    .extra_required
                    .iter()
                    .any(|re| re.is_match(value)),
            ),
            extra_allowed: Flag::new(
                self.config
                    .extra_allowed
                    .iter()
                    .any(|re| re.is_match(value)),
            ),
            quotes_needed: Flag::new(
                matches!(style, ScalarStyle::SingleQuoted | ScalarStyle::DoubleQuoted)
                    && quotes_are_needed(
                        style,
                        value,
                        self.in_flow(),
                        self.buffer,
                        span,
                    ),
            ),
        }
    }

    fn required_always_message(
        &mut self,
        node_label: &str,
        facts: ScalarQuoteFacts,
    ) -> Option<String> {
        if facts.style.is_none()
            || facts.style.is_some_and(|style_kind| {
                self.mismatched_quote(
                    style_kind,
                    facts.has_quoted_quotes.get(),
                    facts.has_double_quote_escape.get(),
                )
            })
        {
            Some(self.not_quoted_with_message(node_label))
        } else {
            None
        }
    }

    fn required_never_message(
        &mut self,
        node_label: &str,
        facts: ScalarQuoteFacts,
    ) -> Option<String> {
        facts.style.map_or_else(
            || {
                facts
                    .extra_required
                    .get()
                    .then(|| format!("string {node_label} is not quoted"))
            },
            |style_kind| {
                self.mismatched_quote(
                    style_kind,
                    facts.has_quoted_quotes.get(),
                    facts.has_double_quote_escape.get(),
                )
                .then(|| self.not_quoted_with_message(node_label))
            },
        )
    }

    fn only_when_needed_message(
        &mut self,
        node_label: &str,
        value: &str,
        resolves_to_string: bool,
        facts: ScalarQuoteFacts,
    ) -> Option<String> {
        facts.style.map_or_else(
            || {
                facts
                    .extra_required
                    .get()
                    .then(|| format!("string {node_label} is not quoted"))
            },
            |style_kind| {
                if resolves_to_string && !value.is_empty() && !facts.quotes_needed.get()
                {
                    return self.redundant_quote_message(node_label, style_kind, facts);
                }
                self.mismatched_quote(
                    style_kind,
                    facts.has_quoted_quotes.get(),
                    facts.has_double_quote_escape.get(),
                )
                .then(|| self.not_quoted_with_message(node_label))
            },
        )
    }

    fn redundant_quote_message(
        &self,
        node_label: &str,
        style_kind: QuoteStyle,
        facts: ScalarQuoteFacts,
    ) -> Option<String> {
        let has_escape_exception = self.escaped_double_quote_exception(
            style_kind,
            facts.has_double_quote_escape.get(),
        );
        if facts.extra_required.get()
            || facts.extra_allowed.get()
            || has_escape_exception
        {
            None
        } else {
            Some(format!(
                "string {node_label} is redundantly quoted with {} quotes",
                self.config.quote_type_label
            ))
        }
    }

    fn not_quoted_with_message(&self, node_label: &str) -> String {
        format!(
            "string {node_label} is not quoted with {} quotes",
            self.config.quote_type_label
        )
    }

    fn mismatched_quote(
        &mut self,
        style_kind: QuoteStyle,
        has_quoted_quotes: bool,
        has_double_quote_escape: bool,
    ) -> bool {
        !(self.escaped_double_quote_exception(style_kind, has_double_quote_escape)
            || self.configured_quote_type_matches(style_kind)
            || (self.config.allow_quoted_quotes && has_quoted_quotes))
    }

    fn escaped_double_quote_exception(
        &self,
        style_kind: QuoteStyle,
        has_double_quote_escape: bool,
    ) -> bool {
        self.config.allow_double_quotes_for_escaping
            && matches!(style_kind, QuoteStyle::Double)
            && has_double_quote_escape
    }

    fn configured_quote_type_matches(&mut self, style_kind: QuoteStyle) -> bool {
        match self.config.quote_type {
            QuoteType::Any => true,
            QuoteType::Single => matches!(style_kind, QuoteStyle::Single),
            QuoteType::Double => matches!(style_kind, QuoteStyle::Double),
            QuoteType::Consistent => {
                let expected = self.consistent_quote_style.get_or_insert(style_kind);
                *expected == style_kind
            }
        }
    }

    fn has_escaping_in_double_quotes(&self, style: ScalarStyle, span: Span) -> bool {
        if !matches!(style, ScalarStyle::DoubleQuoted) {
            return false;
        }

        let slice_start = span.start.index().saturating_add(1).min(self.buffer.len());
        let mut slice_end = span.end.index().saturating_sub(1);
        slice_end = slice_end.min(self.buffer.len());
        slice_end = slice_end.max(slice_start);
        self.buffer[slice_start..slice_end].contains('\\')
    }
}

fn build_violation(span: Span, message: String) -> Violation {
    Violation {
        line: span.start.line(),
        column: span.start.col() + 1,
        message,
    }
}

fn is_flow_sequence(buffer: &str, span: Span) -> bool {
    matches!(
        next_non_whitespace_char(buffer, span.start.index()),
        Some('[')
    )
}

fn is_flow_mapping(buffer: &str, span: Span) -> bool {
    matches!(
        next_non_whitespace_char(buffer, span.start.index()),
        Some('{')
    )
}

fn next_non_whitespace_char(text: &str, byte_idx: usize) -> Option<char> {
    text.get(byte_idx..)
        .and_then(|tail| tail.chars().find(|ch| !ch.is_whitespace()))
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
                self.result = Some(
                    matches!(style, ScalarStyle::Plain)
                        && value.as_ref() == self.expected,
                );
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
