use std::collections::HashMap;

pub(super) use super::input::Initial;
use crate::rules::support::line_syntax::starts_with_document_marker;
use crate::text_version::model::Scalar;
use ryl_yaml_parser::ScalarStyle;

use super::{
    Config, Flag, QuoteStyle, QuoteType, RequiredMode, ScalarQuoteFacts,
    configured_quote_type_matches, contains_non_printable,
    contains_plain_indicator_token, escaped_double_quote_exception, ladder_target,
    plain_scalar_equivalent, quote_style, quote_value,
    quoted_scalar_contains_opposite_quote, value_needs_double_quotes_for_content,
};

#[derive(Clone, Copy)]
pub(super) struct Placement {
    pub(super) context: crate::rules::support::collection_style::RetainedContext,
    pub(super) column: crate::rules::indentation::RetainedColumn,
    pub(super) colon: crate::rules::colons::Context,
}

pub(super) struct Planner<'cfg, 'state> {
    config: &'cfg Config,
    consistent: &'state mut ConsistentStyle,
}

#[derive(Clone, Copy, Default)]
pub(super) struct ConsistentStyle(Option<QuoteStyle>);

impl ConsistentStyle {
    pub(super) fn observe(
        &mut self,
        config: &Config,
        model: &Scalar<'_>,
        initial: Initial<'_>,
    ) {
        if self.0.is_none()
            && !skip_scalar(config, initial.style, model)
            && let Some(style) = quote_style(initial.style)
            && !escaped_double_quote_exception(config, style, initial.any_escape())
        {
            self.0 = Some(style);
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Mode {
    Format,
    Fix,
}

pub(super) struct Eligible<'text>(Scalar<'text>);

pub(super) enum Recipe {
    Text(String),
    SingleRetainedMultiline,
}

pub(super) struct Selected(Recipe);

impl Selected {
    pub(super) fn into_recipe(self) -> Recipe {
        self.0
    }
}

impl<'text> Eligible<'text> {
    pub(super) fn requires_column(&self) -> bool {
        marker_needs_column(self.0.value())
    }

    pub(super) fn new(
        config: &Config,
        model: Scalar<'text>,
        style: ScalarStyle,
        mode: Mode,
    ) -> Option<Self> {
        let keep = matches!(mode, Mode::Format)
            && matches!(
                style,
                ScalarStyle::Plain | ScalarStyle::Literal | ScalarStyle::Folded
            );
        (!keep && !skip_scalar(config, style, &model)).then_some(Self(model))
    }
}

impl<'cfg, 'state> Planner<'cfg, 'state> {
    pub(super) const fn new(
        config: &'cfg Config,
        consistent: &'state mut ConsistentStyle,
    ) -> Self {
        Self { config, consistent }
    }

    pub(super) fn plan(
        &mut self,
        eligible: Eligible<'_>,
        initial: Initial<'_>,
        placement: Placement,
        cache: &mut HashMap<String, Option<bool>>,
    ) -> Option<Selected> {
        let model = eligible.0;
        let value = model.value();
        let mut facts = facts(self.config, value, initial, placement, cache);
        if placement.colon.adjacent() {
            facts.quotes_needed = Flag::new(true);
        }
        let text = match self.config.required {
            RequiredMode::Always => {
                self.fix_required_always(value, facts).map(Recipe::Text)
            }
            RequiredMode::Never => {
                self.fix_required_never(value, facts).map(Recipe::Text)
            }
            RequiredMode::OnlyWhenNeeded => {
                self.fix_only_when_needed(&model, facts, initial)
            }
        };
        text.map(Selected)
    }

    fn fix_required_always(
        &mut self,
        value: &str,
        facts: ScalarQuoteFacts,
    ) -> Option<String> {
        match facts.style {
            None => {
                let target = self.default_quote_style();
                replacement_for_target(value, target)
            }
            Some(style_kind) => {
                if self.mismatched_quote(style_kind, facts) {
                    let target = self.target_quote_style(style_kind);
                    replacement_for_target(value, target)
                } else {
                    None
                }
            }
        }
    }

    fn fix_required_never(
        &mut self,
        value: &str,
        facts: ScalarQuoteFacts,
    ) -> Option<String> {
        match facts.style {
            None => {
                if facts.extra_required.get() {
                    let target = self.default_quote_style();
                    replacement_for_target(value, target)
                } else {
                    None
                }
            }
            Some(style_kind) => {
                if self.mismatched_quote(style_kind, facts) {
                    let target = self.target_quote_style(style_kind);
                    replacement_for_target(value, target)
                } else {
                    None
                }
            }
        }
    }

    fn fix_only_when_needed(
        &mut self,
        model: &Scalar<'_>,
        facts: ScalarQuoteFacts,
        initial: Initial<'_>,
    ) -> Option<Recipe> {
        let value = model.value();
        match facts.style {
            None => {
                if facts.extra_required.get() {
                    let target = self.default_quote_style();
                    replacement_for_target(value, target).map(Recipe::Text)
                } else {
                    None
                }
            }
            Some(style_kind) => {
                if !value.is_empty()
                    && !facts.quotes_needed.get()
                    && !self.redundant_quote_allowed(style_kind, facts)
                    && model.plain_is_string()
                {
                    return Some(Recipe::Text(value.to_owned()));
                }
                if let Some(preferred) = self.config.avoid_escapes {
                    let target = ladder_target(preferred, style_kind, value, facts);
                    if target == QuoteStyle::Single
                        && style_kind == QuoteStyle::Double
                        && initial.multiline()
                    {
                        return Some(Recipe::SingleRetainedMultiline);
                    }
                    return (target != style_kind)
                        .then(|| {
                            replacement_for_target(value, target).map(Recipe::Text)
                        })
                        .flatten();
                }
                if self.mismatched_quote(style_kind, facts) {
                    let target = self.target_quote_style(style_kind);
                    replacement_for_target(value, target).map(Recipe::Text)
                } else {
                    None
                }
            }
        }
    }

    fn redundant_quote_allowed(
        &self,
        style_kind: QuoteStyle,
        facts: ScalarQuoteFacts,
    ) -> bool {
        let has_escape_exception = escaped_double_quote_exception(
            self.config,
            style_kind,
            facts.has_double_quote_escape.get(),
        );
        facts.extra_required.get() || facts.extra_allowed.get() || has_escape_exception
    }

    fn default_quote_style(&mut self) -> QuoteStyle {
        match self.config.quote_type {
            QuoteType::Double => QuoteStyle::Double,
            QuoteType::Consistent => {
                *self.consistent.0.get_or_insert(QuoteStyle::Single)
            }
            QuoteType::Single | QuoteType::Any => QuoteStyle::Single,
        }
    }

    fn target_quote_style(&mut self, current: QuoteStyle) -> QuoteStyle {
        match self.config.quote_type {
            QuoteType::Single | QuoteType::Any => QuoteStyle::Single,
            QuoteType::Double => QuoteStyle::Double,
            QuoteType::Consistent => {
                let expected = self.consistent.0.get_or_insert(current);
                *expected
            }
        }
    }

    fn mismatched_quote(
        &mut self,
        style_kind: QuoteStyle,
        facts: ScalarQuoteFacts,
    ) -> bool {
        !(escaped_double_quote_exception(
            self.config,
            style_kind,
            facts.has_double_quote_escape.get(),
        ) || configured_quote_type_matches(
            self.config,
            &mut self.consistent.0,
            style_kind,
        ) || (self.config.allow_quoted_quotes && facts.has_quoted_quotes.get()))
    }
}

pub(super) fn skip_scalar(
    config: &Config,
    style: ScalarStyle,
    model: &Scalar<'_>,
) -> bool {
    matches!(style, ScalarStyle::Literal | ScalarStyle::Folded)
        || (model.is_key() && !config.check_keys)
        || model.has_core_tag()
        || (style == ScalarStyle::Plain && !model.plain_is_string())
}

fn replacement_for_target(value: &str, target: QuoteStyle) -> Option<String> {
    if target == QuoteStyle::Single && value_needs_double_quotes_for_content(value) {
        return None;
    }
    Some(quote_value(value, target))
}

pub(super) fn facts(
    config: &Config,
    value: &str,
    initial: Initial<'_>,
    placement: Placement,
    cache: &mut HashMap<String, Option<bool>>,
) -> ScalarQuoteFacts {
    ScalarQuoteFacts {
        style: quote_style(initial.style),
        has_quoted_quotes: Flag::new(quoted_scalar_contains_opposite_quote(
            initial.style,
            value,
        )),
        has_double_quote_escape: Flag::new(if config.avoid_escapes.is_some() {
            initial.non_quote_escape()
        } else {
            initial.any_escape()
        }),
        extra_required: Flag::new(
            config.extra_required.iter().any(|re| re.is_match(value)),
        ),
        extra_allowed: Flag::new(
            config.extra_allowed.iter().any(|re| re.is_match(value)),
        ),
        quotes_needed: Flag::new(
            matches!(
                initial.style,
                ScalarStyle::SingleQuoted | ScalarStyle::DoubleQuoted
            ) && quotes_are_needed(value, initial, placement, cache),
        ),
    }
}

pub(super) fn marker_needs_column(value: &str) -> bool {
    starts_with_document_marker(value)
}

pub(super) fn marker_needs_quotes(
    value: &str,
    column: crate::rules::indentation::RetainedColumn,
    colon: crate::rules::colons::Context,
) -> bool {
    column.at_zero()
        && marker_needs_column(value)
        && (value.len() != 3 || !colon.follows())
}

fn quotes_are_needed(
    value: &str,
    initial: Initial<'_>,
    placement: Placement,
    plain_scalar_cache: &mut HashMap<String, Option<bool>>,
) -> bool {
    if value.contains('\u{feff}')
        || marker_needs_quotes(value, placement.column, placement.colon)
    {
        return true;
    }
    if placement.context.in_flow()
        && value
            .chars()
            .any(|c| matches!(c, ',' | '[' | ']' | '{' | '}'))
    {
        return true;
    }

    if contains_plain_indicator_token(value) {
        return true;
    }

    if matches!(initial.style, ScalarStyle::DoubleQuoted) {
        if value.contains('\t') {
            return true;
        }
        if contains_non_printable(value) {
            return true;
        }
        if initial.escaped_line_ending() {
            return true;
        }
    }

    plain_scalar_cache
        .get(value)
        .copied()
        .unwrap_or_else(|| {
            let result = plain_scalar_equivalent(value);
            plain_scalar_cache.insert(value.to_owned(), result);
            result
        })
        .is_none_or(|result| !result)
}
