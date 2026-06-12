use globset::Glob;
use regex::Regex;

use super::{
    KeyOrderingOptions, PerLineIgnore, QuotedStringsOptions, QuotedStringsRequired,
    QuotedStringsRequiredMode, RuleEntry, RuleOptions, RulesTable,
    TomlQuotedStringsOptions,
};

/// Validate `per-line-ignores` entries: each needs at least one of `regex`/`path`
/// and a non-empty `rules` list, and each `regex`/`path` must be a valid pattern. Rule
/// names are already type-checked by deserialization. Validating the patterns here
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

impl<Q: QuotedStringsOptionSet, K, A, C> RulesTable<Q, K, A, C> {
    pub(super) fn validate(&self) -> Result<(), String> {
        validate_key_ordering_rule(self.key_ordering.as_ref())?;
        validate_quoted_strings_rule(self.quoted_strings.as_ref())?;
        Ok(())
    }
}

fn validate_key_ordering_rule(
    entry: Option<&RuleEntry<KeyOrderingOptions>>,
) -> Result<(), String> {
    let Some(options) = rule_options(entry) else {
        return Ok(());
    };
    let Some(patterns) = &options.specific.ignored_keys else {
        return Ok(());
    };

    validate_key_ordering_patterns(patterns)
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
