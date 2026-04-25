use regex::Regex;
use saphyr::{MappingOwned, YamlOwned};
use serde::de::DeserializeOwned;

use super::{
    ForbidSetting, IndentSequencesSetting, KeyOrderingOptions, NewLinesType, QuoteType,
    QuotedStringsOptions, QuotedStringsRequired, QuotedStringsRequiredMode, RuleEntry,
    RuleLevel, RuleOptions, RulesTable, SpacesSetting, TruthyAllowedValue,
    load_ignore_from_files, load_ignore_patterns, parse_string_items,
    yaml_owned_to_toml_value, yaml_value_matches_toml_type,
};

impl RulesTable {
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
    entry: Option<&RuleEntry<QuotedStringsOptions>>,
) -> Result<(), String> {
    let Some(options) = rule_options(entry) else {
        return Ok(());
    };
    let specific = &options.specific;
    let required = quoted_strings_required_mode(specific.required.as_ref());
    validate_quoted_strings_semantics(
        required,
        specific.extra_required.as_deref(),
        specific.extra_allowed.as_deref(),
    )
}

pub(crate) fn validate_yaml_rule_value(
    name: &str,
    value: &YamlOwned,
) -> Result<(), String> {
    if let Some(text) = value.as_str() {
        return match text {
            "enable" | "disable" => Ok(()),
            _ => Err(format!(
                "invalid config: rule '{name}' should be 'enable', 'disable', or a mapping"
            )),
        };
    }

    if value.as_bool().is_some() {
        return Ok(());
    }

    if let Some(map) = value.as_mapping() {
        if name == "quoted-strings" {
            validate_yaml_quoted_strings_rule(map)?;
            return Ok(());
        }

        for (key, val) in map {
            if handle_common_yaml_rule_key(name, key, val)? {
                continue;
            }
            validate_specific_yaml_rule_option(name, key, val)?;
        }
        return Ok(());
    }

    Err(format!(
        "invalid config: rule '{name}' should be 'enable', 'disable', or a mapping"
    ))
}

fn handle_common_yaml_rule_key(
    rule: &str,
    key: &YamlOwned,
    val: &YamlOwned,
) -> Result<bool, String> {
    if key.as_str() == Some("level") {
        if yaml_value_matches_toml_type::<RuleLevel>(val) {
            return Ok(true);
        }
        return Err(format!(
            "invalid config: rule '{rule}' level should be \"error\" or \"warning\""
        ));
    }

    if key.as_str() == Some("ignore") {
        load_ignore_patterns(val)?;
        return Ok(true);
    }

    if key.as_str() == Some("ignore-from-file") {
        load_ignore_from_files(val)?;
        return Ok(true);
    }

    Ok(false)
}

fn validate_specific_yaml_rule_option(
    rule: &str,
    key: &YamlOwned,
    val: &YamlOwned,
) -> Result<(), String> {
    match rule {
        "anchors" => validate_scalar_rule_option(
            "anchors",
            key,
            val,
            &[
                "forbid-undeclared-aliases",
                "forbid-duplicated-anchors",
                "forbid-unused-anchors",
            ],
            &[],
        ),
        "braces" => validate_brace_like_option("braces", key, val),
        "brackets" => validate_brace_like_option("brackets", key, val),
        "document-end" => {
            validate_scalar_rule_option("document-end", key, val, &["present"], &[])
        }
        "document-start" => {
            validate_scalar_rule_option("document-start", key, val, &["present"], &[])
        }
        "empty-lines" => validate_scalar_rule_option(
            "empty-lines",
            key,
            val,
            &[],
            &["max", "max-start", "max-end"],
        ),
        "commas" => validate_scalar_rule_option(
            "commas",
            key,
            val,
            &[],
            &["max-spaces-before", "min-spaces-after", "max-spaces-after"],
        ),
        "comments" => validate_scalar_rule_option_allow_non_string(
            "comments",
            key,
            val,
            &["require-starting-space", "ignore-shebangs"],
            &["min-spaces-from-content"],
        ),
        "new-lines" => validate_new_lines_option(key, val),
        "octal-values" => validate_scalar_rule_option(
            "octal-values",
            key,
            val,
            &["forbid-implicit-octal", "forbid-explicit-octal"],
            &[],
        ),
        "float-values" => validate_scalar_rule_option(
            "float-values",
            key,
            val,
            &[
                "require-numeral-before-decimal",
                "forbid-scientific-notation",
                "forbid-nan",
                "forbid-inf",
            ],
            &[],
        ),
        "empty-values" => validate_scalar_rule_option(
            "empty-values",
            key,
            val,
            &[
                "forbid-in-block-mappings",
                "forbid-in-flow-mappings",
                "forbid-in-block-sequences",
            ],
            &[],
        ),
        "key-duplicates" => validate_scalar_rule_option(
            "key-duplicates",
            key,
            val,
            &["forbid-duplicated-merge-keys"],
            &[],
        ),
        "hyphens" => {
            validate_scalar_rule_option("hyphens", key, val, &[], &["max-spaces-after"])
        }
        "truthy" => validate_truthy_option(key, val),
        "key-ordering" => validate_key_ordering_yaml_option(key, val),
        "indentation" => validate_indentation_option(key, val),
        "line-length" => validate_scalar_rule_option(
            "line-length",
            key,
            val,
            &[
                "allow-non-breakable-words",
                "allow-non-breakable-inline-mappings",
            ],
            &["max"],
        ),
        "trailing-spaces" => Err(unknown_rule_option("trailing-spaces", key)),
        "comments-indentation" => Err(unknown_rule_option("comments-indentation", key)),
        _ => Ok(()),
    }
}

fn validate_brace_like_option(
    rule: &str,
    key: &YamlOwned,
    val: &YamlOwned,
) -> Result<(), String> {
    let Some(name) = key.as_str() else {
        return Err(unknown_rule_option(rule, key));
    };

    match name {
        "forbid" => validate_typed_option_value::<ForbidSetting>(
            val,
            format!(
                "invalid config: option \"forbid\" of \"{rule}\" should be bool or \"non-empty\""
            ),
        ),
        "min-spaces-inside" => validate_int_option(val, rule, "min-spaces-inside"),
        "max-spaces-inside" => validate_int_option(val, rule, "max-spaces-inside"),
        "min-spaces-inside-empty" => {
            validate_int_option(val, rule, "min-spaces-inside-empty")
        }
        "max-spaces-inside-empty" => {
            validate_int_option(val, rule, "max-spaces-inside-empty")
        }
        _ => Err(unknown_rule_option(rule, key)),
    }
}

fn validate_scalar_rule_option(
    rule: &str,
    key: &YamlOwned,
    val: &YamlOwned,
    bool_options: &[&str],
    int_options: &[&str],
) -> Result<(), String> {
    let Some(name) = key.as_str() else {
        return Err(unknown_rule_option(rule, key));
    };

    if bool_options.contains(&name) {
        return validate_bool_option(val, rule, name);
    }
    if int_options.contains(&name) {
        return validate_int_option(val, rule, name);
    }

    Err(unknown_rule_option(rule, key))
}

fn validate_scalar_rule_option_allow_non_string(
    rule: &str,
    key: &YamlOwned,
    val: &YamlOwned,
    bool_options: &[&str],
    int_options: &[&str],
) -> Result<(), String> {
    if key.as_str().is_none() {
        return Ok(());
    }

    validate_scalar_rule_option(rule, key, val, bool_options, int_options)
}

fn validate_new_lines_option(key: &YamlOwned, val: &YamlOwned) -> Result<(), String> {
    if key.as_str() != Some("type") {
        return Err(unknown_rule_option("new-lines", key));
    }

    validate_typed_option_value::<NewLinesType>(
        val,
        "invalid config: option \"type\" of \"new-lines\" should be in ('unix', 'dos', 'platform')",
    )
}

fn validate_truthy_option(key: &YamlOwned, val: &YamlOwned) -> Result<(), String> {
    match key.as_str() {
        Some("allowed-values") => {
            validate_typed_option_value::<Vec<TruthyAllowedValue>>(
                val,
                "invalid config: option \"allowed-values\" of \"truthy\" should only contain values in ['YES', 'Yes', 'yes', 'NO', 'No', 'no', 'TRUE', 'True', 'true', 'FALSE', 'False', 'false', 'ON', 'On', 'on', 'OFF', 'Off', 'off']",
            )
        }
        Some("check-keys") => validate_bool_option(val, "truthy", "check-keys"),
        _ => Err(unknown_rule_option("truthy", key)),
    }
}

fn validate_key_ordering_yaml_option(
    key: &YamlOwned,
    val: &YamlOwned,
) -> Result<(), String> {
    match key.as_str() {
        Some("ignored-keys") => {
            let patterns = parse_string_items(
                val,
                "invalid config: option \"ignored-keys\" of \"key-ordering\" should contain regex strings",
                |text| vec![text.to_owned()],
            )?;
            validate_key_ordering_patterns(&patterns)
        }
        _ => Err(unknown_rule_option("key-ordering", key)),
    }
}

fn validate_indentation_option(key: &YamlOwned, val: &YamlOwned) -> Result<(), String> {
    match key.as_str() {
        Some("spaces") => validate_typed_option_value::<SpacesSetting>(
            val,
            "invalid config: option \"spaces\" of \"indentation\" should be in (<class 'int'>, 'consistent')",
        ),
        Some("indent-sequences") => {
            validate_typed_option_value::<IndentSequencesSetting>(
                val,
                "invalid config: option \"indent-sequences\" of \"indentation\" should be in (<class 'bool'>, 'whatever', 'consistent')",
            )
        }
        Some("check-multi-line-strings") => {
            validate_bool_option(val, "indentation", "check-multi-line-strings")
        }
        _ => Err(unknown_rule_option("indentation", key)),
    }
}

fn validate_yaml_quoted_strings_rule(map: &MappingOwned) -> Result<(), String> {
    let mut state = QuotedStringsValidationState::default();
    for (key, val) in map {
        if handle_common_yaml_rule_key("quoted-strings", key, val)? {
            continue;
        }
        validate_quoted_strings_option(key, val, &mut state)?;
    }
    state.finish()
}

#[derive(Default)]
struct QuotedStringsValidationState {
    required: Option<QuotedStringsRequiredModeForValidation>,
    extra_required_count: Option<usize>,
    extra_allowed_count: Option<usize>,
}

impl QuotedStringsValidationState {
    fn finish(&self) -> Result<(), String> {
        let extra_required = self
            .extra_required_count
            .map(|count| vec![String::new(); count]);
        let extra_allowed = self
            .extra_allowed_count
            .map(|count| vec![String::new(); count]);
        validate_quoted_strings_semantics(
            self.required
                .unwrap_or(QuotedStringsRequiredModeForValidation::True),
            extra_required.as_deref(),
            extra_allowed.as_deref(),
        )
    }
}

fn validate_quoted_strings_option(
    key: &YamlOwned,
    val: &YamlOwned,
    state: &mut QuotedStringsValidationState,
) -> Result<(), String> {
    match key.as_str() {
        Some("quote-type") => validate_typed_option_value::<QuoteType>(
            val,
            "invalid config: option \"quote-type\" of \"quoted-strings\" should be in ('any', 'single', 'double')",
        ),
        Some("required") => {
            if let Some(required) = quoted_strings_required_mode_from_yaml_value(val) {
                state.required = Some(required);
                Ok(())
            } else {
                Err(
                    "invalid config: option \"required\" of \"quoted-strings\" should be in (True, False, 'only-when-needed')"
                        .to_string(),
                )
            }
        }
        Some("extra-required") => validate_regex_list_option(
            val,
            "extra-required",
            &mut state.extra_required_count,
        ),
        Some("extra-allowed") => validate_regex_list_option(
            val,
            "extra-allowed",
            &mut state.extra_allowed_count,
        ),
        Some("allow-quoted-quotes") => {
            validate_bool_option(val, "quoted-strings", "allow-quoted-quotes")
        }
        Some("check-keys") => validate_bool_option(val, "quoted-strings", "check-keys"),
        _ => Err(unknown_rule_option("quoted-strings", key)),
    }
}

fn validate_regex_list_option(
    val: &YamlOwned,
    option_name: &str,
    count_slot: &mut Option<usize>,
) -> Result<(), String> {
    let Some(seq) = val.as_sequence() else {
        return Err(quoted_strings_regex_type_error(option_name));
    };
    *count_slot = Some(seq.len());
    validate_regex_strings(
        val,
        &quoted_strings_regex_type_error(option_name),
        |text, err| {
            format!(
                "invalid config: regex \"{text}\" in option \"{option_name}\" of \"quoted-strings\" is invalid: {err}"
            )
        },
    )
}

fn validate_bool_option(
    val: &YamlOwned,
    rule_name: &str,
    option_name: &str,
) -> Result<(), String> {
    validate_option_type::<bool>(val, rule_name, option_name, "bool")
}

fn validate_int_option(
    val: &YamlOwned,
    rule_name: &str,
    option_name: &str,
) -> Result<(), String> {
    validate_option_type::<i64>(val, rule_name, option_name, "int")
}

fn validate_option_type<T: DeserializeOwned>(
    val: &YamlOwned,
    rule_name: &str,
    option_name: &str,
    expected_type: &str,
) -> Result<(), String> {
    if yaml_value_matches_toml_type::<T>(val) {
        Ok(())
    } else {
        Err(format!(
            "invalid config: option \"{option_name}\" of \"{rule_name}\" should be {expected_type}"
        ))
    }
}

fn validate_typed_option_value<T: DeserializeOwned>(
    val: &YamlOwned,
    error_message: impl Into<String>,
) -> Result<(), String> {
    if yaml_value_matches_toml_type::<T>(val) {
        Ok(())
    } else {
        Err(error_message.into())
    }
}

fn unknown_rule_option(rule: &str, key: &YamlOwned) -> String {
    format!(
        "invalid config: unknown option \"{}\" for rule \"{rule}\"",
        describe_rule_option_key(key)
    )
}

fn validate_regex_strings(
    val: &YamlOwned,
    type_error: &str,
    invalid_regex: impl Fn(&str, regex::Error) -> String,
) -> Result<(), String> {
    let values = parse_string_items(val, type_error, |text| vec![text.to_owned()])?;
    for text in values {
        Regex::new(&text).map_err(|err| invalid_regex(&text, err))?;
    }
    Ok(())
}

fn quoted_strings_regex_type_error(option_name: &str) -> String {
    format!(
        "invalid config: option \"{option_name}\" of \"quoted-strings\" should only contain values in [<class 'str'>]"
    )
}

fn describe_rule_option_key(key: &YamlOwned) -> String {
    match (
        key.as_str(),
        key.as_integer(),
        key.as_floating_point(),
        key.as_bool(),
        key.is_null(),
    ) {
        (Some(s), _, _, _, _) => s.to_owned(),
        (_, Some(i), _, _, _) => i.to_string(),
        (_, _, Some(f), _, _) => f.to_string(),
        (_, _, _, Some(b), _) => b.to_string(),
        (_, _, _, _, true) => "None".to_string(),
        _ => format!("{key:?}"),
    }
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

fn quoted_strings_required_mode_from_yaml_value(
    value: &YamlOwned,
) -> Option<QuotedStringsRequiredModeForValidation> {
    let required = yaml_owned_to_toml_value(value)
        .ok()?
        .try_into::<QuotedStringsRequired>()
        .ok()?;
    Some(quoted_strings_required_mode(Some(&required)))
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
