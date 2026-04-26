use regex::Regex;
use toml::Value;

use super::{
    KeyOrderingOptions, QuotedStringsOptions, QuotedStringsRequired,
    QuotedStringsRequiredMode, RuleEntry, RuleOptions, RulesTable,
};

impl RulesTable {
    pub(super) fn validate(&self) -> Result<(), String> {
        validate_key_ordering_rule(self.key_ordering.as_ref())?;
        validate_quoted_strings_rule(self.quoted_strings.as_ref())?;
        validate_extra_rule_filters(&self.extra)?;
        Ok(())
    }
}

fn validate_extra_rule_filters(
    rules: &std::collections::BTreeMap<String, Value>,
) -> Result<(), String> {
    for (rule_name, value) in rules {
        let Value::Table(table) = value else {
            continue;
        };

        if let Some(ignore) = table.get("ignore") {
            validate_string_or_array_of_strings(
                ignore,
                &format!(
                    "invalid config: option \"ignore\" of \"{rule_name}\" should contain file patterns"
                ),
            )?;
        }

        if let Some(ignore_from_file) = table.get("ignore-from-file") {
            validate_string_or_array_of_strings(
                ignore_from_file,
                &format!(
                    "invalid config: option \"ignore-from-file\" of \"{rule_name}\" should contain filename(s), either as a list or string"
                ),
            )?;
        }
    }

    Ok(())
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
    entry: Option<&RuleEntry<QuotedStringsOptions>>,
) -> Result<(), String> {
    let Some(options) = rule_options(entry) else {
        return Ok(());
    };
    let specific = &options.specific;
    validate_quoted_strings_semantics(
        quoted_strings_required_mode(specific.required.as_ref()),
        specific.extra_required.as_deref(),
        specific.extra_allowed.as_deref(),
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

fn validate_string_or_array_of_strings(
    value: &Value,
    error: &str,
) -> Result<(), String> {
    match value {
        Value::String(_) => Ok(()),
        Value::Array(items)
            if items.iter().all(|item| matches!(item, Value::String(_))) =>
        {
            Ok(())
        }
        _ => Err(error.to_string()),
    }
}
