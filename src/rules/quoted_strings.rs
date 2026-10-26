//! `quoted-strings`: enforce a scalar quoting policy: quote type (single/double/any),
//! when quotes are required versus redundant, and extra allow/require patterns. Mirrors
//! yamllint's `quoted-strings`. Safe `--fix` adds or normalises quoting only where the
//! rewrite provably preserves the scalar's value.

use std::collections::HashMap;

mod target;
pub(crate) struct Spelling(String);

impl Spelling {
    fn materialize(selected: target::Selected, retained: &str) -> Self {
        let text = match selected.into_recipe() {
            target::Recipe::Text(text) => text,
            target::Recipe::SingleRetainedMultiline => {
                quote_value(&retained.replace("\\\"", "\""), QuoteStyle::Single)
            }
        };
        Self(text)
    }

    fn as_str(&self) -> &str {
        &self.0
    }
    pub(crate) fn style(&self) -> ScalarStyle {
        match self.0.as_bytes().first() {
            Some(b'\'') => ScalarStyle::SingleQuoted,
            Some(b'"') => ScalarStyle::DoubleQuoted,
            _ => ScalarStyle::Plain,
        }
    }
    pub(crate) fn into_text(self) -> String {
        self.0
    }
}
use crate::text_version::model::{
    Role, Scalar as ScalarModel, resolves_to_string_for_version,
};

use regex::Regex;
use ryl_yaml_parser::{
    Event, Marker, Parser, ScalarStyle, Span, SpannedEventReceiver, StructureStyle, Tag,
};

use crate::config::YamlLintConfig;
use crate::rules::support::mapping_key_walker::Walker;
use crate::rules::support::span_utils::{
    BytePos, apply_replacements, marker_byte_offset,
};
use crate::rules::support::yaml_version::{Version, event_version};
use crate::yaml_dom::is_core_schema;

pub const ID: &str = "quoted-strings";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuoteType {
    Any,
    Single,
    Double,
    Consistent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteStyle {
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
    avoid_escapes: Option<QuoteStyle>,
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
            .and_then(crate::yaml_dom::YamlOwned::as_bool)
            .unwrap_or(false);

        let allow_double_quotes_for_escaping = cfg
            .rule_option(ID, "allow-double-quotes-for-escaping")
            .and_then(crate::yaml_dom::YamlOwned::as_bool)
            .unwrap_or(false);

        let check_keys = cfg
            .rule_option(ID, "check-keys")
            .and_then(crate::yaml_dom::YamlOwned::as_bool)
            .unwrap_or(false);

        Self {
            quote_type,
            quote_type_label,
            required,
            extra_required,
            extra_allowed,
            allow_quoted_quotes,
            allow_double_quotes_for_escaping,
            avoid_escapes: None,
            check_keys,
        }
    }

    #[must_use]
    pub fn has_extra_required(&self) -> bool {
        !self.extra_required.is_empty()
    }

    #[must_use]
    pub fn with_allow_double_quotes_for_escaping(mut self, value: bool) -> Self {
        self.allow_double_quotes_for_escaping = value;
        self
    }

    /// Quotes only where `required = "only-when-needed"` lint needs them, in whichever
    /// quotes avoid an escape, else in `style`; keys included.
    #[must_use]
    pub const fn ladder(style: QuoteStyle) -> Self {
        let (quote_type, quote_type_label) = match style {
            QuoteStyle::Single => (QuoteType::Single, "single"),
            QuoteStyle::Double => (QuoteType::Double, "double"),
        };
        Self {
            quote_type,
            quote_type_label,
            required: RequiredMode::OnlyWhenNeeded,
            extra_required: Vec::new(),
            extra_allowed: Vec::new(),
            allow_quoted_quotes: false,
            allow_double_quotes_for_escaping: true,
            avoid_escapes: Some(style),
            check_keys: true,
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
    check_with_analysis(
        crate::text_version::TextVersion::new(buffer).analysis(),
        cfg,
    )
}

pub(crate) fn check_with_analysis(
    input: crate::text_version::Analysis<'_>,
    cfg: &Config,
) -> Vec<Violation> {
    let buffer = input.text();
    let mut receiver = QuotedStringsReceiver::new(cfg, buffer);
    for (event, span) in input.event_iter() {
        receiver.on_event(event, span);
    }
    receiver.diagnostics
}

struct QuotedStringsReceiver<'cfg> {
    state: QuotedStringsState<'cfg>,
    diagnostics: Vec<Violation>,
}

impl<'cfg> QuotedStringsReceiver<'cfg> {
    fn new(cfg: &'cfg Config, buffer: &'cfg str) -> Self {
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
            Event::DocumentStart(_, version) => {
                self.state.document_start(version.map(event_version));
            }
            Event::DocumentEnd => self.state.document_end(),
            Event::SequenceStart(style, _, _) => {
                self.state.enter_sequence(style == StructureStyle::Flow);
            }
            Event::SequenceEnd | Event::MappingEnd => self.state.exit_container(),
            Event::MappingStart(style, _, _) => {
                self.state.enter_mapping(style == StructureStyle::Flow);
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
            Event::Alias(_) => self.state.skip_node(),
            _ => {}
        }
    }
}

struct QuotedStringsState<'cfg> {
    config: &'cfg Config,
    buffer: &'cfg str,
    walker: Walker<(), bool>,
    consistent_quote_style: Option<QuoteStyle>,
    current_version: Option<Version>,
    plain_scalar_cache: HashMap<String, Option<bool>>,
}

impl<'cfg> QuotedStringsState<'cfg> {
    fn new(config: &'cfg Config, buffer: &'cfg str) -> Self {
        Self {
            config,
            buffer,
            walker: Walker::new(),
            consistent_quote_style: None,
            current_version: None,
            plain_scalar_cache: HashMap::new(),
        }
    }

    fn reset_stream(&mut self) {
        self.walker.reset();
        self.consistent_quote_style = None;
        self.current_version = None;
    }

    fn document_start(&mut self, version: Option<Version>) {
        self.walker.reset();
        self.current_version = version;
    }

    fn document_end(&mut self) {
        self.walker.reset();
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

    fn skip_node(&mut self) {
        self.walker.skip_node();
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
        let resolves_to_string =
            resolves_to_string_for_version(self.current_version, value);

        if should_skip_scalar(self.config, style, tag, active_key, resolves_to_string) {
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

    fn evaluate_scalar(
        &mut self,
        style: ScalarStyle,
        value: &str,
        active_key: bool,
        resolves_to_string: bool,
        span: Span,
    ) -> Option<Violation> {
        let node_label = if active_key { "key" } else { "value" };
        let facts = scalar_quote_facts(
            self.config,
            (self.buffer, self.in_flow()),
            style,
            value,
            (span, colon_follows(self.buffer, span, None)),
            &mut self.plain_scalar_cache,
        );

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

    fn required_always_message(
        &mut self,
        node_label: &str,
        facts: ScalarQuoteFacts,
    ) -> Option<String> {
        if facts.style.is_none_or(|style_kind| {
            self.mismatched_quote(
                style_kind,
                facts.has_quoted_quotes.get(),
                facts.has_double_quote_escape.get(),
            )
        }) {
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
                if let Some(preferred) = self.config.avoid_escapes {
                    return self.ladder_message(
                        node_label, preferred, style_kind, value, facts,
                    );
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

    fn ladder_message(
        &self,
        node_label: &str,
        preferred: QuoteStyle,
        current: QuoteStyle,
        value: &str,
        facts: ScalarQuoteFacts,
    ) -> Option<String> {
        match ladder_target(preferred, current, value, facts) {
            target if target == current => None,
            target if target == preferred => {
                Some(self.not_quoted_with_message(node_label))
            }
            _ => Some("change outer quotes to avoid escaping inner quotes".to_owned()),
        }
    }

    fn redundant_quote_message(
        &self,
        node_label: &str,
        style_kind: QuoteStyle,
        facts: ScalarQuoteFacts,
    ) -> Option<String> {
        let has_escape_exception = escaped_double_quote_exception(
            self.config,
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
        !(escaped_double_quote_exception(
            self.config,
            style_kind,
            has_double_quote_escape,
        ) || configured_quote_type_matches(
            self.config,
            &mut self.consistent_quote_style,
            style_kind,
        ) || (self.config.allow_quoted_quotes && has_quoted_quotes))
    }
}

fn build_violation(span: Span, message: String) -> Violation {
    Violation {
        line: span.start.line(),
        column: span.start.col() + 1,
        message,
    }
}

fn should_skip_scalar(
    config: &Config,
    style: ScalarStyle,
    tag: Option<&Tag>,
    active_key: bool,
    resolves_to_string: bool,
) -> bool {
    if matches!(style, ScalarStyle::Literal | ScalarStyle::Folded) {
        return true;
    }

    if active_key && !config.check_keys {
        return true;
    }

    if let Some(tag) = tag
        && is_core_schema(tag)
    {
        return true;
    }

    matches!(style, ScalarStyle::Plain) && !resolves_to_string
}

fn scalar_quote_facts(
    config: &Config,
    (buffer, in_flow): (&str, bool),
    style: ScalarStyle,
    value: &str,
    (span, colon_follows): (Span, bool),
    plain_scalar_cache: &mut HashMap<String, Option<bool>>,
) -> ScalarQuoteFacts {
    target::facts(
        config,
        value,
        scalar_initial(buffer, style, span),
        target::Placement {
            in_flow,
            at_column_zero: span.start.col() == 0,
            colon: target::ColonTarget {
                follows: colon_follows,
                adjacent: false,
            },
        },
        plain_scalar_cache,
    )
}

fn scalar_initial(buffer: &str, style: ScalarStyle, span: Span) -> target::Initial {
    let mut escapes = false;
    let mut non_quote_escape = false;
    let mut backslash_line_ending = false;
    let mut multiline = false;
    if style == ScalarStyle::DoubleQuoted {
        let (start, end) = scalar_source_bounds(buffer, style, span);
        let content = inner_quoted_content(buffer, start, end);
        for (at, _) in content.match_indices('\\') {
            escapes = true;
            let after = &content[at + 1..];
            non_quote_escape |= !after.starts_with('"');
            backslash_line_ending |=
                after.starts_with('\n') || after.starts_with("\r\n");
        }
        multiline = content.contains(['\n', '\r']);
    }
    target::Initial {
        style,
        escapes: target::EscapeFacts {
            any: escapes,
            non_quote: non_quote_escape,
            line_ending: backslash_line_ending,
        },
        multiline,
    }
}

fn escaped_double_quote_exception(
    config: &Config,
    style_kind: QuoteStyle,
    has_double_quote_escape: bool,
) -> bool {
    config.allow_double_quotes_for_escaping
        && matches!(style_kind, QuoteStyle::Double)
        && has_double_quote_escape
}

fn configured_quote_type_matches(
    config: &Config,
    consistent_quote_style: &mut Option<QuoteStyle>,
    style_kind: QuoteStyle,
) -> bool {
    match config.quote_type {
        QuoteType::Any => true,
        QuoteType::Single => matches!(style_kind, QuoteStyle::Single),
        QuoteType::Double => matches!(style_kind, QuoteStyle::Double),
        QuoteType::Consistent => {
            let expected = consistent_quote_style.get_or_insert(style_kind);
            *expected == style_kind
        }
    }
}

fn has_escaping_in_double_quotes(buffer: &str, style: ScalarStyle, span: Span) -> bool {
    if !matches!(style, ScalarStyle::DoubleQuoted) {
        return false;
    }

    let (scalar_start, scalar_end) = scalar_source_bounds(buffer, style, span);
    inner_quoted_content(buffer, scalar_start, scalar_end).contains('\\')
}

/// The quotes the formatter's ladder keeps a quoted scalar in, after prettier's YAML rule.
fn ladder_target(
    preferred: QuoteStyle,
    current: QuoteStyle,
    value: &str,
    facts: ScalarQuoteFacts,
) -> QuoteStyle {
    if current == QuoteStyle::Double && facts.has_double_quote_escape.get() {
        QuoteStyle::Double
    } else if value.contains('"') {
        QuoteStyle::Single
    } else if value.contains('\'')
        && !value.contains('\\')
        && !value_needs_double_quotes_for_content(value)
    {
        QuoteStyle::Double
    } else {
        preferred
    }
}

fn inner_quoted_content(buffer: &str, start: BytePos, end: BytePos) -> &str {
    let slice_start = start.get().saturating_add(1).min(buffer.len());
    let slice_end = end.get().saturating_sub(1).max(slice_start);
    &buffer[slice_start..slice_end]
}

/// Whether a `:` follows the scalar at `span`, directly or once the colons pass closes
/// the spaces before it, which `spaced_colon` says it does at a byte.
fn colon_follows(
    buffer: &str,
    span: Span,
    spaced_colon: Option<&dyn Fn(usize) -> bool>,
) -> bool {
    span.end.byte_offset().is_some_and(|end| {
        let rest = &buffer[end..];
        let gap = rest.len() - rest.trim_start_matches(' ').len();
        rest[gap..].starts_with(':')
            && (gap == 0 || spaced_colon.is_some_and(|spaced| spaced(end + gap)))
    })
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

fn contains_plain_indicator_token(value: &str) -> bool {
    value.split_whitespace().any(|token| {
        token.len() == 1 && token.chars().next().is_some_and(is_plain_indicator_token)
    })
}

const fn is_plain_indicator_token(ch: char) -> bool {
    matches!(ch, '!' | '&' | '*' | '|' | '>' | '?' | '@' | '%' | '`')
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

fn next_token(mut rest: &str) -> &str {
    loop {
        rest = rest.trim_start_matches([' ', '\t', '\r', '\n']);
        let Some(comment) = rest.strip_prefix('#') else {
            return rest;
        };
        rest = comment.find(['\r', '\n']).map_or("", |eol| &comment[eol..]);
    }
}

fn scalar_source_bounds(
    buffer: &str,
    style: ScalarStyle,
    span: Span,
) -> (BytePos, BytePos) {
    let start = marker_byte_offset(span.start).get().min(buffer.len());
    let span_end = marker_byte_offset(span.end).get().min(buffer.len());
    let end = match style {
        ScalarStyle::SingleQuoted => quoted_scalar_end(buffer, start, b'\''),
        ScalarStyle::DoubleQuoted => double_quoted_scalar_end(buffer, start),
        ScalarStyle::Plain | ScalarStyle::Literal | ScalarStyle::Folded => span_end,
    };
    (BytePos::new(start), BytePos::new(end.max(start)))
}

fn quoted_scalar_end(buffer: &str, start: usize, quote: u8) -> usize {
    let bytes = buffer.as_bytes();
    let mut cursor = start.saturating_add(1);
    let mut closing_quote = None;
    while cursor < bytes.len() {
        if bytes[cursor] != quote {
            cursor += 1;
            continue;
        }

        if quote == b'\'' && bytes.get(cursor.saturating_add(1)) == Some(&quote) {
            cursor += 2;
            continue;
        }

        closing_quote = Some(cursor + 1);
        break;
    }

    closing_quote.expect("quoted scalar event should include a closing quote")
}

fn double_quoted_scalar_end(buffer: &str, start: usize) -> usize {
    let bytes = buffer.as_bytes();
    let mut cursor = start.saturating_add(1);
    let mut escaped = false;
    let mut closing_quote = None;
    while cursor < bytes.len() {
        match bytes[cursor] {
            _ if escaped => escaped = false,
            b'\\' => escaped = true,
            b'"' => {
                closing_quote = Some(cursor + 1);
                break;
            }
            _ => {}
        }
        cursor += 1;
    }

    closing_quote.expect("double-quoted scalar event should include a closing quote")
}

type Replacement = (BytePos, BytePos, Spelling);

#[must_use]
pub fn fix(buffer: &str, cfg: &Config) -> Option<String> {
    let mut parser = Parser::new_from_str(buffer);
    let mut fixer = QuotedStringsFixer::new(cfg, buffer);
    let _ = parser.load(&mut fixer, true);
    fixer.finish()
}

/// Each scalar's settled replacement; `format` leaves plain and block scalars alone, and
/// `spaced_colon` says whether the colons pass leaves a space after the `:` at a byte.
pub(crate) fn format_replacements<'a>(
    input: crate::text_version::Analysis<'a>,
    cfg: &'a Config,
    cache: &mut HashMap<String, Option<bool>>,
    format: bool,
    spaced_colon: &'a dyn Fn(usize) -> bool,
) -> Vec<Replacement> {
    let mut fixer = QuotedStringsFixer::new(cfg, input.text());
    fixer.state.spaced_colon = Some(spaced_colon);
    std::mem::swap(&mut fixer.state.plain_scalar_cache, cache);
    fixer.format = format;
    fixer.settle = true;
    for (event, span) in input.event_iter() {
        fixer.on_event(event, span);
    }
    std::mem::swap(&mut fixer.state.plain_scalar_cache, cache);
    fixer.replacements
}

fn initial_consistent_quote_style(cfg: &Config, buffer: &str) -> Option<QuoteStyle> {
    if !matches!(cfg.quote_type, QuoteType::Consistent) {
        return None;
    }

    let mut parser = Parser::new_from_str(buffer);
    let mut finder = ConsistentQuoteStyleFinder::new(cfg, buffer);
    let _ = parser.load(&mut finder, true);
    finder.finish()
}

struct ConsistentQuoteStyleFinder<'cfg> {
    state: FixState<'cfg>,
}

impl<'cfg> ConsistentQuoteStyleFinder<'cfg> {
    fn new(cfg: &'cfg Config, buffer: &'cfg str) -> Self {
        Self {
            state: FixState::new(cfg, buffer),
        }
    }

    fn finish(self) -> Option<QuoteStyle> {
        self.state.consistent_quote_style
    }
}

impl SpannedEventReceiver<'_> for ConsistentQuoteStyleFinder<'_> {
    fn on_event(&mut self, event: Event<'_>, span: Span) {
        self.state
            .visit_event(event, span, |state, style, value, tag, span| {
                state.collect_consistent_quote_style(style, value, tag, span);
            });
    }
}

struct QuotedStringsFixer<'cfg> {
    state: FixState<'cfg>,
    replacements: Vec<Replacement>,
    format: bool,
    settle: bool,
}

impl<'cfg> QuotedStringsFixer<'cfg> {
    fn new(cfg: &'cfg Config, buffer: &'cfg str) -> Self {
        Self {
            state: FixState::with_consistent_quote_style(
                cfg,
                buffer,
                initial_consistent_quote_style(cfg, buffer),
            ),
            replacements: Vec::new(),
            format: false,
            settle: false,
        }
    }

    fn finish(self) -> Option<String> {
        if self.replacements.is_empty() {
            return None;
        }
        Some(apply_replacements(
            self.state.buffer,
            self.replacements
                .into_iter()
                .map(|(start, end, target)| (start, end, target.into_text()))
                .collect(),
        ))
    }
}

impl SpannedEventReceiver<'_> for QuotedStringsFixer<'_> {
    fn on_event(&mut self, event: Event<'_>, span: Span) {
        let replacements = &mut self.replacements;
        let (format, settle) = (self.format, self.settle);
        self.state
            .visit_event(event, span, |state, style, value, tag, span| {
                if let Some(replacement) =
                    state.fix_scalar(style, value, tag, span, format, settle)
                {
                    replacements.push(replacement);
                }
            });
    }
}

struct FixState<'cfg> {
    config: &'cfg Config,
    buffer: &'cfg str,
    walker: Walker<(), bool>,
    seeded_consistent_quote_style: Option<QuoteStyle>,
    consistent_quote_style: Option<QuoteStyle>,
    current_version: Option<Version>,
    plain_scalar_cache: HashMap<String, Option<bool>>,
    spaced_colon: Option<&'cfg dyn Fn(usize) -> bool>,
}

impl<'cfg> FixState<'cfg> {
    fn settle_scalar(
        &mut self,
        value: &str,
        active_key: bool,
        replacement: Replacement,
    ) -> Replacement {
        let (start, end, spelling) = replacement;
        let suffix = if active_key && self.adjacent_colon(end) {
            ":x"
        } else {
            ""
        };
        let style = spelling.style();
        if style == ScalarStyle::Plain {
            return (start, end, spelling);
        }
        let text = spelling.as_str();
        let buffer = format!("{text}{suffix}");
        let chars = text.chars().count();
        let end_line = 1
            + crate::rules::support::line_syntax::split_lines_preserve_endings(text)
                .filter(|(_, _, ending)| !ending.is_empty())
                .count();
        let end_column = text
            .rsplit(['\n', '\r'])
            .next()
            .unwrap_or("")
            .chars()
            .count();
        let span = Span::new(
            Marker::new(0, 1, 0).with_byte_offset(Some(0)),
            Marker::new(chars, end_line, end_column).with_byte_offset(Some(text.len())),
        );
        let mut local = FixState::new(self.config, &buffer);
        if self.in_flow() {
            local.enter_sequence(true);
        }
        std::mem::swap(&mut local.plain_scalar_cache, &mut self.plain_scalar_cache);
        let next = local.compute_fix(
            scalar_model(value, None, active_key, self.current_version),
            style,
            span,
            target::Mode::Fix,
        );
        std::mem::swap(&mut local.plain_scalar_cache, &mut self.plain_scalar_cache);
        (start, end, next.map_or(spelling, |(_, _, next)| next))
    }

    /// Whether a `:` before a non-space (`{'k':v}`, `{'k' :v}`) follows the key ending at
    /// `end`, even across comments and line breaks: it is a value indicator only because the
    /// key is quoted, unless the colons pass spaces it.
    fn adjacent_colon(&self, end: BytePos) -> bool {
        let rest = &self.buffer[end.get()..];
        let token = next_token(rest);
        token
            .strip_prefix(':')
            .and_then(|after| after.chars().next())
            .is_some_and(|next| {
                !next.is_whitespace()
                    && (matches!(next, ',' | ']' | '}' | '#')
                        || self.spaced_colon.is_none_or(|spaced| {
                            !spaced(end.get() + rest.len() - token.len())
                        }))
            })
    }

    fn visit_event(
        &mut self,
        event: Event<'_>,
        span: Span,
        visit_scalar: impl FnOnce(&mut Self, ScalarStyle, &str, Option<&Tag>, Span),
    ) {
        match event {
            Event::StreamStart => self.reset_stream(),
            Event::DocumentStart(_, version) => {
                self.document_start(version.map(event_version));
            }
            Event::DocumentEnd => self.document_end(),
            Event::SequenceStart(style, _, _) => {
                self.enter_sequence(style == StructureStyle::Flow);
            }
            Event::SequenceEnd | Event::MappingEnd => self.exit_container(),
            Event::MappingStart(style, _, _) => {
                self.enter_mapping(style == StructureStyle::Flow);
            }
            Event::Scalar(value, style, _, tag) => {
                visit_scalar(self, style, value.as_ref(), tag.as_deref(), span);
            }
            Event::Alias(_) => self.skip_node(),
            _ => {}
        }
    }

    fn new(config: &'cfg Config, buffer: &'cfg str) -> Self {
        Self::with_consistent_quote_style(config, buffer, None)
    }

    fn with_consistent_quote_style(
        config: &'cfg Config,
        buffer: &'cfg str,
        consistent_quote_style: Option<QuoteStyle>,
    ) -> Self {
        Self {
            config,
            buffer,
            walker: Walker::new(),
            seeded_consistent_quote_style: consistent_quote_style,
            consistent_quote_style,
            current_version: None,
            plain_scalar_cache: HashMap::new(),
            spaced_colon: None,
        }
    }

    fn reset_stream(&mut self) {
        self.walker.reset();
        self.consistent_quote_style = self.seeded_consistent_quote_style;
        self.current_version = None;
    }

    fn document_start(&mut self, version: Option<Version>) {
        self.walker.reset();
        self.current_version = version;
    }

    fn document_end(&mut self) {
        self.walker.reset();
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

    fn skip_node(&mut self) {
        self.walker.skip_node();
    }

    fn in_flow(&self) -> bool {
        self.walker.any_metadata(|flow| *flow)
    }

    fn fix_scalar(
        &mut self,
        style: ScalarStyle,
        value: &str,
        tag: Option<&Tag>,
        span: Span,
        format: bool,
        settle: bool,
    ) -> Option<Replacement> {
        let context = self.walker.begin_node();
        let active_key = context.active();
        let model = scalar_model(value, tag, active_key, self.current_version);
        let mode = if format {
            target::Mode::Format
        } else {
            target::Mode::Fix
        };
        let replacement = self.compute_fix(model, style, span, mode).map(|first| {
            if settle {
                self.settle_scalar(value, active_key, first)
            } else {
                first
            }
        });
        self.walker.finish_node(context);
        replacement
    }

    fn collect_consistent_quote_style(
        &mut self,
        style: ScalarStyle,
        value: &str,
        tag: Option<&Tag>,
        span: Span,
    ) {
        let context = self.walker.begin_node();
        let active_key = context.active();
        let resolves_to_string =
            resolves_to_string_for_version(self.current_version, value);

        if should_skip_scalar(self.config, style, tag, active_key, resolves_to_string) {
            self.walker.finish_node(context);
            return;
        }

        if self.consistent_quote_style.is_none()
            && let Some(style_kind) = quote_style(style)
            && !escaped_double_quote_exception(
                self.config,
                style_kind,
                has_escaping_in_double_quotes(self.buffer, style, span),
            )
        {
            self.consistent_quote_style = Some(style_kind);
        }

        self.walker.finish_node(context);
    }

    fn compute_fix(
        &mut self,
        model: ScalarModel<'_>,
        style: ScalarStyle,
        span: Span,
        mode: target::Mode,
    ) -> Option<Replacement> {
        let active_key = model.is_key();
        let eligible = target::Eligible::new(self.config, model, style, mode)?;
        let (start, end) = scalar_source_bounds(self.buffer, style, span);
        let initial = scalar_initial(self.buffer, style, span);
        let placement = target::Placement {
            in_flow: self.in_flow(),
            at_column_zero: span.start.col() == 0,
            colon: target::ColonTarget {
                follows: colon_follows(self.buffer, span, self.spaced_colon),
                adjacent: active_key && self.adjacent_colon(end),
            },
        };
        target::Planner {
            config: self.config,
            consistent_quote_style: &mut self.consistent_quote_style,
        }
        .plan(eligible, initial, placement, &mut self.plain_scalar_cache)
        .map(|selected| {
            let retained = if initial.multiline {
                inner_quoted_content(self.buffer, start, end)
            } else {
                ""
            };
            (start, end, Spelling::materialize(selected, retained))
        })
    }
}

fn scalar_model<'a>(
    value: &'a str,
    tag: Option<&'a Tag>,
    active_key: bool,
    version: Option<Version>,
) -> ScalarModel<'a> {
    let role = if active_key { Role::Key } else { Role::Value };
    ScalarModel::new(value, tag, role, version)
}

fn value_needs_double_quotes_for_content(value: &str) -> bool {
    value.contains('\n') || value.contains('\r') || contains_non_printable(value)
}

const DOUBLE_QUOTE_ESCAPES: &[(char, &str)] = &[
    ('\\', "\\\\"),
    ('"', "\\\""),
    ('\0', "\\0"),
    ('\u{7}', "\\a"),
    ('\u{8}', "\\b"),
    ('\t', "\\t"),
    ('\n', "\\n"),
    ('\u{b}', "\\v"),
    ('\u{c}', "\\f"),
    ('\r', "\\r"),
    ('\u{1b}', "\\e"),
];

fn double_quote_escape(ch: char) -> Option<&'static str> {
    DOUBLE_QUOTE_ESCAPES
        .iter()
        .find_map(|(candidate, escape)| (*candidate == ch).then_some(*escape))
}

fn quote_value(value: &str, style: QuoteStyle) -> String {
    match style {
        QuoteStyle::Single => {
            let mut result = String::with_capacity(value.len().saturating_add(2));
            result.push('\'');
            for ch in value.chars() {
                if ch == '\'' {
                    result.push_str("''");
                } else {
                    result.push(ch);
                }
            }
            result.push('\'');
            result
        }
        QuoteStyle::Double => {
            let mut result = String::with_capacity(value.len().saturating_add(2));
            result.push('"');
            for ch in value.chars() {
                if let Some(escape) = double_quote_escape(ch) {
                    result.push_str(escape);
                } else {
                    result.push(ch);
                }
            }
            result.push('"');
            result
        }
    }
}
