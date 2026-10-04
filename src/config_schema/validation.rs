use globset::Glob;
use regex::Regex;

use super::{
    KeyOrderEntry, KeyOrderingOptions, PerLineIgnore, QuotedStringsOptions,
    QuotedStringsRequired, QuotedStringsRequiredMode, RuleEntry, RuleOptions,
    RulesTable, TomlCommentsOptions, TomlKeyOrderingOptions, TomlQuotedStringsOptions,
};
use crate::rules::support::key_path;

/// Validate `per-line-ignores` entries: each needs at least one of `regex`/`path`, a
/// non-empty `rules` list, and valid `regex`/`path` patterns. Validating the patterns here
/// (the single fallible step) lets the runtime matcher build infallibly.
///
/// # Errors
/// Returns an error describing the first invalid entry.
pub fn validate_per_line_ignores(entries: &[PerLineIgnore]) -> Result<(), String> {
    for entry in entries {
        if entry.regex.is_none() && entry.path.is_none() {
            return Err(
                "invalid config: each per-line-ignores entry needs at least one of \
                 `regex` or `path`"
                    .to_string(),
            );
        }
        if entry.rules.is_empty() {
            return Err(
                "invalid config: per-line-ignores entry has an empty `rules` list"
                    .to_string(),
            );
        }
        if let Some(pattern) = entry.regex.as_deref() {
            Regex::new(pattern).map_err(|err| {
                format!(
                    "invalid config: per-line-ignores `regex` '{pattern}' is invalid: {err}"
                )
            })?;
        }
        if let Some(pattern) = entry.path.as_deref() {
            // Validate the same glob the matcher compiles: a leading `!` is a negation
            // marker (per-file-ignores parity), stripped before compilation.
            let glob = pattern.strip_prefix('!').unwrap_or(pattern);
            Glob::new(glob).map_err(|err| {
                format!(
                    "invalid config: per-line-ignores `path` '{pattern}' is invalid: {err}"
                )
            })?;
        }
    }
    Ok(())
}

pub trait QuotedStringsOptionSet {
    fn required(&self) -> Option<&QuotedStringsRequired>;
    fn extra_required(&self) -> Option<&[String]>;
    fn extra_allowed(&self) -> Option<&[String]>;
}

impl QuotedStringsOptionSet for QuotedStringsOptions {
    fn required(&self) -> Option<&QuotedStringsRequired> {
        self.required.as_ref()
    }

    fn extra_required(&self) -> Option<&[String]> {
        self.extra_required.as_deref()
    }

    fn extra_allowed(&self) -> Option<&[String]> {
        self.extra_allowed.as_deref()
    }
}

impl QuotedStringsOptionSet for TomlQuotedStringsOptions {
    fn required(&self) -> Option<&QuotedStringsRequired> {
        self.required.as_ref()
    }

    fn extra_required(&self) -> Option<&[String]> {
        self.extra_required.as_deref()
    }

    fn extra_allowed(&self) -> Option<&[String]> {
        self.extra_allowed.as_deref()
    }
}

pub trait KeyOrderingOptionSet {
    fn ignored_keys(&self) -> Option<&[String]>;
    fn orders(&self) -> &[KeyOrderEntry];
}

impl KeyOrderingOptionSet for KeyOrderingOptions {
    fn ignored_keys(&self) -> Option<&[String]> {
        self.ignored_keys.as_deref()
    }

    fn orders(&self) -> &[KeyOrderEntry] {
        &[]
    }
}

impl KeyOrderingOptionSet for TomlKeyOrderingOptions {
    fn ignored_keys(&self) -> Option<&[String]> {
        self.ignored_keys.as_deref()
    }

    fn orders(&self) -> &[KeyOrderEntry] {
        self.orders.as_deref().unwrap_or_default()
    }
}

impl<Q: QuotedStringsOptionSet, K, A, C, H, M, O: KeyOrderingOptionSet>
    RulesTable<Q, K, A, C, H, M, O>
{
    pub(super) fn validate(&self) -> Result<(), String> {
        validate_key_ordering_rule(self.key_ordering.as_ref())?;
        validate_quoted_strings_rule(self.quoted_strings.as_ref())?;
        Ok(())
    }
}

pub(super) fn validate_comments_rule(
    entry: Option<&RuleEntry<TomlCommentsOptions>>,
) -> Result<(), String> {
    let Some(options) = rule_options(entry) else {
        return Ok(());
    };
    let Some(max) = options.specific.max_spaces_from_content else {
        return Ok(());
    };
    let min = options.specific.min_spaces_from_content.unwrap_or(2);
    if max == 0 {
        return Err("invalid config: comments: \"max-spaces-from-content\" must be at least 1 (a comment needs whitespace before its \"#\")".to_string());
    }
    if max > 0 && min > max {
        return Err(format!(
            "invalid config: comments: \"max-spaces-from-content\" ({max}) is below \"min-spaces-from-content\" ({min}; it defaults to 2 when unset)"
        ));
    }
    Ok(())
}

fn validate_key_ordering_rule(
    entry: Option<&RuleEntry<impl KeyOrderingOptionSet>>,
) -> Result<(), String> {
    let Some(options) = rule_options(entry) else {
        return Ok(());
    };
    for (index, order) in options.specific.orders().iter().enumerate() {
        validate_key_order(order).map_err(|problem| {
            format!(
                "invalid config: entry {} of option \"orders\" of \"key-ordering\" {problem}",
                index + 1
            )
        })?;
    }
    options
        .specific
        .ignored_keys()
        .map_or(Ok(()), validate_key_ordering_patterns)
}

fn validate_key_order(order: &KeyOrderEntry) -> Result<(), String> {
    if order.files.is_empty() {
        return Err("has an empty `files` list".to_owned());
    }
    for pattern in &order.files {
        Glob::new(pattern.strip_prefix('!').unwrap_or(pattern))
            .map_err(|err| format!("has an invalid `files` glob '{pattern}': {err}"))?;
    }
    key_path::parse(&order.path)
        .map_err(|err| format!("has an invalid `path` '{}': {err}", order.path))?;
    if order.keys.is_empty() {
        return Err("has an empty `keys` list".to_owned());
    }
    let mut seen = std::collections::HashSet::new();
    match order.keys.iter().find(|key| !seen.insert(*key)) {
        Some(key) => Err(format!("lists key '{key}' twice in `keys`")),
        None => Ok(()),
    }
}

fn validate_quoted_strings_rule(
    entry: Option<&RuleEntry<impl QuotedStringsOptionSet>>,
) -> Result<(), String> {
    let Some(options) = rule_options(entry) else {
        return Ok(());
    };
    let specific = &options.specific;
    validate_quoted_strings_semantics(
        quoted_strings_required_mode(specific.required()),
        specific.extra_required(),
        specific.extra_allowed(),
    )
}

fn rule_options<T>(entry: Option<&RuleEntry<T>>) -> Option<&RuleOptions<T>> {
    match entry {
        Some(RuleEntry::Options(options)) => Some(options),
        Some(RuleEntry::Bool(_) | RuleEntry::Switch(_)) | None => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum QuotedStringsRequiredModeForValidation {
    True,
    False,
    OnlyWhenNeeded,
}

fn quoted_strings_required_mode(
    required: Option<&QuotedStringsRequired>,
) -> QuotedStringsRequiredModeForValidation {
    match required {
        None => QuotedStringsRequiredModeForValidation::True,
        Some(QuotedStringsRequired::Bool(true)) => {
            QuotedStringsRequiredModeForValidation::True
        }
        Some(QuotedStringsRequired::Bool(false)) => {
            QuotedStringsRequiredModeForValidation::False
        }
        Some(QuotedStringsRequired::Mode(
            QuotedStringsRequiredMode::OnlyWhenNeeded,
        )) => QuotedStringsRequiredModeForValidation::OnlyWhenNeeded,
    }
}

fn validate_key_ordering_patterns(patterns: &[String]) -> Result<(), String> {
    validate_regex_list(Some(patterns), |text, err| {
        format!(
            "invalid config: option \"ignored-keys\" of \"key-ordering\" contains invalid regex '{text}': {err}"
        )
    })
}

fn validate_quoted_strings_semantics(
    required: QuotedStringsRequiredModeForValidation,
    extra_required: Option<&[String]>,
    extra_allowed: Option<&[String]>,
) -> Result<(), String> {
    let extra_required_count = extra_required.map_or(0, <[String]>::len);
    let extra_allowed_count = extra_allowed.map_or(0, <[String]>::len);

    if matches!(required, QuotedStringsRequiredModeForValidation::True)
        && extra_allowed_count > 0
    {
        return Err(
            "invalid config: quoted-strings: cannot use both \"required: true\" and \"extra-allowed\""
                .to_string(),
        );
    }
    if matches!(required, QuotedStringsRequiredModeForValidation::True)
        && extra_required_count > 0
    {
        return Err(
            "invalid config: quoted-strings: cannot use both \"required: true\" and \"extra-required\""
                .to_string(),
        );
    }
    if matches!(required, QuotedStringsRequiredModeForValidation::False)
        && extra_allowed_count > 0
    {
        return Err(
            "invalid config: quoted-strings: cannot use both \"required: false\" and \"extra-allowed\""
                .to_string(),
        );
    }

    validate_regex_list(extra_required, |text, err| {
        format!(
            "invalid config: regex \"{text}\" in option \"extra-required\" of \"quoted-strings\" is invalid: {err}"
        )
    })?;
    validate_regex_list(extra_allowed, |text, err| {
        format!(
            "invalid config: regex \"{text}\" in option \"extra-allowed\" of \"quoted-strings\" is invalid: {err}"
        )
    })?;

    Ok(())
}

fn validate_regex_list(
    patterns: Option<&[String]>,
    invalid_regex: impl Fn(&str, regex::Error) -> String,
) -> Result<(), String> {
    let Some(patterns) = patterns else {
        return Ok(());
    };

    for text in patterns {
        Regex::new(text).map_err(|err| invalid_regex(text, err))?;
    }

    Ok(())
}
