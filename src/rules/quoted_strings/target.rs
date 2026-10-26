use std::collections::HashMap;

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
pub(super) struct Initial {
    pub(super) style: ScalarStyle,
    pub(super) escapes: EscapeFacts,
    pub(super) multiline: bool,
}

#[derive(Clone, Copy)]
pub(super) struct EscapeFacts {
    pub(super) any: bool,
    pub(super) non_quote: bool,
    pub(super) line_ending: bool,
}

#[derive(Clone, Copy)]
pub(super) struct Placement {
    pub(super) in_flow: bool,
    pub(super) at_column_zero: bool,
    pub(super) colon: ColonTarget,
}

#[derive(Clone, Copy)]
pub(super) struct ColonTarget {
    pub(super) follows: bool,
    pub(super) adjacent: bool,
}

pub(super) struct Planner<'cfg, 'state> {
    pub(super) config: &'cfg Config,
    pub(super) consistent_quote_style: &'state mut Option<QuoteStyle>,
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
        (!keep
            && !super::should_skip_scalar(
                config,
                style,
                model.tag(),
                model.is_key(),
                model.plain_is_string(),
            ))
        .then_some(Self(model))
    }
}

impl Planner<'_, '_> {
    pub(super) fn plan(
        &mut self,
        eligible: Eligible<'_>,
        initial: Initial,
        placement: Placement,
        cache: &mut HashMap<String, Option<bool>>,
    ) -> Option<Selected> {
        let model = eligible.0;
        let value = model.value();
        let mut facts = facts(self.config, value, initial, placement, cache);
        if placement.colon.adjacent {
            facts.quotes_needed = Flag::new(true);
        }
        let text = match self.config.required {
            RequiredMode::Always => {
                self.fix_required_always(value, facts).map(Recipe::Text)
            }
            RequiredMode::Never => {
                self.fix_required_never(value, facts).map(Recipe::Text)
            }
            RequiredMode::OnlyWhenNeeded => self.fix_only_when_needed(
                value,
                model.plain_is_string(),
                facts,
                initial.multiline,
            ),
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
        value: &str,
        resolves_to_string: bool,
        facts: ScalarQuoteFacts,
        multiline: bool,
    ) -> Option<Recipe> {
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
                if resolves_to_string
                    && !value.is_empty()
                    && !facts.quotes_needed.get()
                    && !self.redundant_quote_allowed(style_kind, facts)
                {
                    return Some(Recipe::Text(value.to_owned()));
                }
                if let Some(preferred) = self.config.avoid_escapes {
                    let target = ladder_target(preferred, style_kind, value, facts);
                    if target == QuoteStyle::Single
                        && style_kind == QuoteStyle::Double
                        && multiline
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
            QuoteType::Consistent => *self
                .consistent_quote_style
                .get_or_insert(QuoteStyle::Single),
            QuoteType::Single | QuoteType::Any => QuoteStyle::Single,
        }
    }

    fn target_quote_style(&mut self, current: QuoteStyle) -> QuoteStyle {
        match self.config.quote_type {
            QuoteType::Single | QuoteType::Any => QuoteStyle::Single,
            QuoteType::Double => QuoteStyle::Double,
            QuoteType::Consistent => {
                let expected = self.consistent_quote_style.get_or_insert(current);
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
            self.consistent_quote_style,
            style_kind,
        ) || (self.config.allow_quoted_quotes && facts.has_quoted_quotes.get()))
    }
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
    initial: Initial,
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
            initial.escapes.non_quote
        } else {
            initial.escapes.any
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

fn quotes_are_needed(
    value: &str,
    initial: Initial,
    placement: Placement,
    plain_scalar_cache: &mut HashMap<String, Option<bool>>,
) -> bool {
    if value.contains('\u{feff}')
        || (placement.at_column_zero
            && starts_with_document_marker(value)
            && (value.len() != 3 || !placement.colon.follows))
    {
        return true;
    }
    if placement.in_flow
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
        if initial.escapes.line_ending {
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
