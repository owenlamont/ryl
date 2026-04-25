use std::collections::BTreeMap;

use regex::Regex;
use saphyr::{LoadableYamlNode, MappingOwned, ScalarOwned, YamlOwned};
use schemars::{JsonSchema, Schema, schema_for};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON Schema root for `ryl` TOML configuration.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[schemars(title = "ryl TOML config")]
pub struct TomlConfig {
    /// Glob patterns used to identify YAML files while scanning directories.
    #[serde(rename = "yaml-files")]
    pub yaml_files: Option<Vec<String>>,
    /// Ignore patterns, either as one multi-line string or a list of patterns.
    pub ignore: Option<StringOrVec>,
    /// Paths to files that contain ignore patterns.
    #[serde(rename = "ignore-from-file")]
    pub ignore_from_file: Option<StringOrVec>,
    /// Locale identifier used by diagnostics.
    pub locale: Option<String>,
    /// Native fix policy, available only in TOML config.
    pub fix: Option<FixTable>,
    /// Rule configuration table.
    pub rules: Option<RulesTable>,
    #[serde(flatten, default)]
    #[schemars(skip)]
    extra: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct PyProjectToml {
    #[serde(default)]
    tool: PyProjectToolTable,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct PyProjectToolTable {
    ryl: Option<TomlConfig>,
}

/// A TOML field that accepts either one string or a list of strings.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum StringOrVec {
    One(String),
    Many(Vec<String>),
}

/// Rule severity override.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
pub enum RuleLevel {
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "warning")]
    Warning,
}

/// Shorthand rule enable/disable syntax.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
pub enum RuleSwitch {
    #[serde(rename = "enable")]
    Enable,
    #[serde(rename = "disable")]
    Disable,
}

/// Common rule entry shape used by TOML config.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum RuleEntry<T> {
    Bool(bool),
    Switch(RuleSwitch),
    Options(RuleOptions<T>),
}

/// Common rule fields plus rule-specific options.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuleOptions<T> {
    pub level: Option<RuleLevel>,
    pub ignore: Option<StringOrVec>,
    #[serde(rename = "ignore-from-file")]
    pub ignore_from_file: Option<StringOrVec>,
    #[serde(flatten)]
    pub specific: T,
}

/// Empty rule-specific table for rules that only support common fields.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoOptions {}

/// TOML `[fix]` table.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FixTable {
    pub fixable: Option<Vec<FixableRuleSelector>>,
    pub unfixable: Option<Vec<FixRuleName>>,
}

/// A rule selector accepted by `fix.fixable`.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
pub enum FixableRuleSelector {
    #[serde(rename = "ALL")]
    All,
    #[serde(rename = "braces")]
    Braces,
    #[serde(rename = "brackets")]
    Brackets,
    #[serde(rename = "commas")]
    Commas,
    #[serde(rename = "comments")]
    Comments,
    #[serde(rename = "comments-indentation")]
    CommentsIndentation,
    #[serde(rename = "new-line-at-end-of-file")]
    NewLineAtEndOfFile,
    #[serde(rename = "new-lines")]
    NewLines,
}

/// A fixable rule name accepted by `fix.unfixable`.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
pub enum FixRuleName {
    #[serde(rename = "braces")]
    Braces,
    #[serde(rename = "brackets")]
    Brackets,
    #[serde(rename = "commas")]
    Commas,
    #[serde(rename = "comments")]
    Comments,
    #[serde(rename = "comments-indentation")]
    CommentsIndentation,
    #[serde(rename = "new-line-at-end-of-file")]
    NewLineAtEndOfFile,
    #[serde(rename = "new-lines")]
    NewLines,
}

/// Built-in rule table for TOML config.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
pub struct RulesTable {
    pub anchors: Option<RuleEntry<AnchorsOptions>>,
    pub braces: Option<RuleEntry<BraceLikeOptions>>,
    pub brackets: Option<RuleEntry<BraceLikeOptions>>,
    pub colons: Option<RuleEntry<ColonsOptions>>,
    pub commas: Option<RuleEntry<CommasOptions>>,
    pub comments: Option<RuleEntry<CommentsOptions>>,
    #[serde(rename = "comments-indentation")]
    pub comments_indentation: Option<RuleEntry<NoOptions>>,
    #[serde(rename = "document-end")]
    pub document_end: Option<RuleEntry<DocumentPresenceOptions>>,
    #[serde(rename = "document-start")]
    pub document_start: Option<RuleEntry<DocumentPresenceOptions>>,
    #[serde(rename = "empty-lines")]
    pub empty_lines: Option<RuleEntry<EmptyLinesOptions>>,
    #[serde(rename = "empty-values")]
    pub empty_values: Option<RuleEntry<EmptyValuesOptions>>,
    #[serde(rename = "float-values")]
    pub float_values: Option<RuleEntry<FloatValuesOptions>>,
    pub hyphens: Option<RuleEntry<HyphensOptions>>,
    pub indentation: Option<RuleEntry<IndentationOptions>>,
    #[serde(rename = "key-duplicates")]
    pub key_duplicates: Option<RuleEntry<KeyDuplicatesOptions>>,
    #[serde(rename = "key-ordering")]
    pub key_ordering: Option<RuleEntry<KeyOrderingOptions>>,
    #[serde(rename = "line-length")]
    pub line_length: Option<RuleEntry<LineLengthOptions>>,
    #[serde(rename = "new-line-at-end-of-file")]
    pub new_line_at_end_of_file: Option<RuleEntry<NoOptions>>,
    #[serde(rename = "new-lines")]
    pub new_lines: Option<RuleEntry<NewLinesOptions>>,
    #[serde(rename = "octal-values")]
    pub octal_values: Option<RuleEntry<OctalValuesOptions>>,
    #[serde(rename = "quoted-strings")]
    pub quoted_strings: Option<RuleEntry<QuotedStringsOptions>>,
    #[serde(rename = "trailing-spaces")]
    pub trailing_spaces: Option<RuleEntry<NoOptions>>,
    pub truthy: Option<RuleEntry<TruthyOptions>>,
    #[serde(flatten, default)]
    #[schemars(skip)]
    extra: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnchorsOptions {
    #[serde(rename = "forbid-undeclared-aliases")]
    pub forbid_undeclared_aliases: Option<bool>,
    #[serde(rename = "forbid-duplicated-anchors")]
    pub forbid_duplicated_anchors: Option<bool>,
    #[serde(rename = "forbid-unused-anchors")]
    pub forbid_unused_anchors: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BraceLikeOptions {
    pub forbid: Option<ForbidSetting>,
    #[serde(rename = "min-spaces-inside")]
    pub min_spaces_inside: Option<i64>,
    #[serde(rename = "max-spaces-inside")]
    pub max_spaces_inside: Option<i64>,
    #[serde(rename = "min-spaces-inside-empty")]
    pub min_spaces_inside_empty: Option<i64>,
    #[serde(rename = "max-spaces-inside-empty")]
    pub max_spaces_inside_empty: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ForbidSetting {
    Bool(bool),
    Mode(ForbidMode),
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
pub enum ForbidMode {
    #[serde(rename = "non-empty")]
    NonEmpty,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ColonsOptions {
    #[serde(rename = "max-spaces-before")]
    pub max_spaces_before: Option<i64>,
    #[serde(rename = "max-spaces-after")]
    pub max_spaces_after: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommasOptions {
    #[serde(rename = "max-spaces-before")]
    pub max_spaces_before: Option<i64>,
    #[serde(rename = "min-spaces-after")]
    pub min_spaces_after: Option<i64>,
    #[serde(rename = "max-spaces-after")]
    pub max_spaces_after: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommentsOptions {
    #[serde(rename = "require-starting-space")]
    pub require_starting_space: Option<bool>,
    #[serde(rename = "ignore-shebangs")]
    pub ignore_shebangs: Option<bool>,
    #[serde(rename = "min-spaces-from-content")]
    pub min_spaces_from_content: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocumentPresenceOptions {
    pub present: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmptyLinesOptions {
    pub max: Option<i64>,
    #[serde(rename = "max-start")]
    pub max_start: Option<i64>,
    #[serde(rename = "max-end")]
    pub max_end: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmptyValuesOptions {
    #[serde(rename = "forbid-in-block-mappings")]
    pub forbid_in_block_mappings: Option<bool>,
    #[serde(rename = "forbid-in-flow-mappings")]
    pub forbid_in_flow_mappings: Option<bool>,
    #[serde(rename = "forbid-in-block-sequences")]
    pub forbid_in_block_sequences: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FloatValuesOptions {
    #[serde(rename = "require-numeral-before-decimal")]
    pub require_numeral_before_decimal: Option<bool>,
    #[serde(rename = "forbid-scientific-notation")]
    pub forbid_scientific_notation: Option<bool>,
    #[serde(rename = "forbid-nan")]
    pub forbid_nan: Option<bool>,
    #[serde(rename = "forbid-inf")]
    pub forbid_inf: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HyphensOptions {
    #[serde(rename = "max-spaces-after")]
    pub max_spaces_after: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IndentationOptions {
    pub spaces: Option<SpacesSetting>,
    #[serde(rename = "indent-sequences")]
    pub indent_sequences: Option<IndentSequencesSetting>,
    #[serde(rename = "check-multi-line-strings")]
    pub check_multi_line_strings: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum SpacesSetting {
    Int(i64),
    Mode(SpacesMode),
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
pub enum SpacesMode {
    #[serde(rename = "consistent")]
    Consistent,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IndentSequencesSetting {
    Bool(bool),
    Mode(IndentSequencesMode),
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
pub enum IndentSequencesMode {
    #[serde(rename = "whatever")]
    Whatever,
    #[serde(rename = "consistent")]
    Consistent,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KeyDuplicatesOptions {
    #[serde(rename = "forbid-duplicated-merge-keys")]
    pub forbid_duplicated_merge_keys: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KeyOrderingOptions {
    #[serde(rename = "ignored-keys")]
    pub ignored_keys: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LineLengthOptions {
    pub max: Option<i64>,
    #[serde(rename = "allow-non-breakable-words")]
    pub allow_non_breakable_words: Option<bool>,
    #[serde(rename = "allow-non-breakable-inline-mappings")]
    pub allow_non_breakable_inline_mappings: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NewLinesOptions {
    #[serde(rename = "type")]
    pub line_ending: Option<NewLinesType>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
pub enum NewLinesType {
    #[serde(rename = "unix")]
    Unix,
    #[serde(rename = "dos")]
    Dos,
    #[serde(rename = "platform")]
    Platform,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OctalValuesOptions {
    #[serde(rename = "forbid-implicit-octal")]
    pub forbid_implicit_octal: Option<bool>,
    #[serde(rename = "forbid-explicit-octal")]
    pub forbid_explicit_octal: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuotedStringsOptions {
    #[serde(rename = "quote-type")]
    pub quote_type: Option<QuoteType>,
    pub required: Option<QuotedStringsRequired>,
    #[serde(rename = "extra-required")]
    pub extra_required: Option<Vec<String>>,
    #[serde(rename = "extra-allowed")]
    pub extra_allowed: Option<Vec<String>>,
    #[serde(rename = "allow-quoted-quotes")]
    pub allow_quoted_quotes: Option<bool>,
    #[serde(rename = "check-keys")]
    pub check_keys: Option<bool>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
pub enum QuoteType {
    #[serde(rename = "any")]
    Any,
    #[serde(rename = "single")]
    Single,
    #[serde(rename = "double")]
    Double,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum QuotedStringsRequired {
    Bool(bool),
    Mode(QuotedStringsRequiredMode),
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
pub enum QuotedStringsRequiredMode {
    #[serde(rename = "only-when-needed")]
    OnlyWhenNeeded,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TruthyOptions {
    #[serde(rename = "allowed-values")]
    pub allowed_values: Option<Vec<TruthyAllowedValue>>,
    #[serde(rename = "check-keys")]
    pub check_keys: Option<bool>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
pub enum TruthyAllowedValue {
    #[serde(rename = "YES")]
    YesUpper,
    #[serde(rename = "Yes")]
    YesTitle,
    #[serde(rename = "yes")]
    YesLower,
    #[serde(rename = "NO")]
    NoUpper,
    #[serde(rename = "No")]
    NoTitle,
    #[serde(rename = "no")]
    NoLower,
    #[serde(rename = "TRUE")]
    TrueUpper,
    #[serde(rename = "True")]
    TrueTitle,
    #[serde(rename = "true")]
    TrueLower,
    #[serde(rename = "FALSE")]
    FalseUpper,
    #[serde(rename = "False")]
    FalseTitle,
    #[serde(rename = "false")]
    FalseLower,
    #[serde(rename = "ON")]
    OnUpper,
    #[serde(rename = "On")]
    OnTitle,
    #[serde(rename = "on")]
    OnLower,
    #[serde(rename = "OFF")]
    OffUpper,
    #[serde(rename = "Off")]
    OffTitle,
    #[serde(rename = "off")]
    OffLower,
}

#[must_use]
pub fn schema() -> Schema {
    schema_for!(TomlConfig)
}

/// Deserialize TOML configuration text into the typed schema model.
///
/// When `pyproject` is true, this extracts `[tool.ryl]` and returns `Ok(None)`
/// when the section is absent.
///
/// # Errors
/// Returns an error if the TOML cannot be parsed into the typed config model.
pub fn parse_toml_config_str(
    input: &str,
    pyproject: bool,
) -> Result<Option<TomlConfig>, String> {
    if pyproject {
        return toml::from_str::<PyProjectToml>(input)
            .map(|doc| doc.tool.ryl)
            .map_err(|err| format!("failed to parse config data: {err}"));
    }

    toml::from_str::<TomlConfig>(input)
        .map(Some)
        .map_err(|err| format!("failed to parse config data: {err}"))
}

/// Validate semantic constraints for a typed TOML config model.
///
/// # Errors
/// Returns an error if the typed TOML config violates semantic rules that are
/// not fully captured by deserialization alone.
pub fn validate_toml_config(config: &TomlConfig) -> Result<(), String> {
    if config.extra.contains_key("extends") {
        return Err(
            "invalid config: extends is not supported in TOML configuration"
                .to_string(),
        );
    }

    if config.ignore.is_some() && config.ignore_from_file.is_some() {
        return Err(
            "invalid config: ignore and ignore-from-file keys cannot be used together"
                .to_string(),
        );
    }

    if let Some(rules) = &config.rules {
        rules.validate()?;
    }

    Ok(())
}

#[derive(Debug, Clone, Default)]
pub struct NormalizedFixConfig {
    pub fixable: Vec<FixableRuleSelector>,
    pub unfixable: Vec<FixRuleName>,
}

#[derive(Debug, Clone, Default)]
pub struct NormalizedConfig {
    pub ignore_patterns: Option<Vec<String>>,
    pub ignore_from_files: Option<Vec<String>>,
    pub yaml_file_patterns: Option<Vec<String>>,
    pub locale: Option<String>,
    pub fix: Option<NormalizedFixConfig>,
    pub rules: BTreeMap<String, YamlOwned>,
}

fn string_or_vec_items(value: &StringOrVec) -> Vec<String> {
    match value {
        StringOrVec::One(item) => vec![item.clone()],
        StringOrVec::Many(items) => items.clone(),
    }
}

fn normalize_fix_table(fix: &FixTable) -> NormalizedFixConfig {
    NormalizedFixConfig {
        fixable: fix
            .fixable
            .clone()
            .unwrap_or_else(|| vec![FixableRuleSelector::All]),
        unfixable: fix.unfixable.clone().unwrap_or_default(),
    }
}

#[must_use]
pub(crate) fn toml_value_to_yaml_owned(value: &toml::Value) -> YamlOwned {
    match value {
        toml::Value::String(text) => {
            YamlOwned::Value(ScalarOwned::String(text.clone()))
        }
        toml::Value::Integer(num) => YamlOwned::Value(ScalarOwned::Integer(*num)),
        toml::Value::Float(num) => {
            let rendered = num.to_string();
            YamlOwned::load_from_str(&rendered)
                .ok()
                .and_then(|docs| docs.into_iter().next())
                .unwrap_or(YamlOwned::Value(ScalarOwned::String(rendered)))
        }
        toml::Value::Boolean(flag) => YamlOwned::Value(ScalarOwned::Boolean(*flag)),
        toml::Value::Datetime(dt) => {
            YamlOwned::Value(ScalarOwned::String(dt.to_string()))
        }
        toml::Value::Array(items) => {
            YamlOwned::Sequence(items.iter().map(toml_value_to_yaml_owned).collect())
        }
        toml::Value::Table(table) => {
            let mut map = MappingOwned::new();
            for (key, val) in table {
                map.insert(
                    YamlOwned::Value(ScalarOwned::String(key.clone())),
                    toml_value_to_yaml_owned(val),
                );
            }
            YamlOwned::Mapping(map)
        }
    }
}

pub(crate) fn yaml_owned_to_toml_value(
    value: &YamlOwned,
) -> Result<toml::Value, String> {
    if let Some(text) = value.as_str() {
        return Ok(toml::Value::String(text.to_string()));
    }
    if let Some(flag) = value.as_bool() {
        return Ok(toml::Value::Boolean(flag));
    }
    if let Some(num) = value.as_integer() {
        return Ok(toml::Value::Integer(num));
    }
    if let Some(num) = value.as_floating_point() {
        return Ok(toml::Value::Float(num));
    }
    if value.is_null() {
        return Err(
            "cannot convert null values to TOML (TOML has no null type)".to_string()
        );
    }
    if let Some(items) = value.as_sequence() {
        let out: Result<Vec<_>, _> =
            items.iter().map(yaml_owned_to_toml_value).collect();
        return out.map(toml::Value::Array);
    }
    if let Some(map) = value.as_mapping() {
        let mut out = toml::map::Map::new();
        for (key, val) in map {
            let Some(key_text) = key.as_str() else {
                return Err(format!("cannot convert non-string TOML key: {key:?}"));
            };
            out.insert(key_text.to_string(), yaml_owned_to_toml_value(val)?);
        }
        return Ok(toml::Value::Table(out));
    }
    Err("cannot convert this YAML node to TOML".to_string())
}

pub(crate) fn yaml_value_matches_toml_type<T>(value: &YamlOwned) -> bool
where
    T: DeserializeOwned,
{
    yaml_owned_to_toml_value(value)
        .ok()
        .and_then(|value| value.try_into::<T>().ok())
        .is_some()
}

/// Normalize a typed TOML config into a shared post-parse representation.
///
/// # Panics
/// Panics if serializing already-validated typed TOML rules unexpectedly stops
/// producing a TOML table.
pub fn normalize_toml_config(config: &TomlConfig) -> NormalizedConfig {
    let mut normalized = NormalizedConfig {
        ignore_patterns: config.ignore.as_ref().map(string_or_vec_items),
        ignore_from_files: config.ignore_from_file.as_ref().map(string_or_vec_items),
        yaml_file_patterns: config.yaml_files.clone(),
        locale: config.locale.clone(),
        fix: config.fix.as_ref().map(normalize_fix_table),
        ..NormalizedConfig::default()
    };

    if let Some(rules) = config.rules.as_ref() {
        let rules = rules_table_to_value(rules);
        normalized.rules = rules
            .as_table()
            .expect("serializing typed TOML rules should yield a table")
            .clone()
            .into_iter()
            .map(|(name, value)| (name, toml_value_to_yaml_owned(&value)))
            .collect();
    }

    normalized
}

fn insert_serialized<T: Serialize>(
    table: &mut toml::map::Map<String, toml::Value>,
    key: &str,
    value: Option<&T>,
) {
    if let Some(value) = value {
        table.insert(
            key.to_string(),
            toml::Value::try_from(value)
                .expect("serializing typed TOML value should succeed"),
        );
    }
}

fn rules_table_to_value(rules: &RulesTable) -> toml::Value {
    let mut table = toml::map::Map::new();
    insert_serialized(&mut table, "anchors", rules.anchors.as_ref());
    insert_serialized(&mut table, "braces", rules.braces.as_ref());
    insert_serialized(&mut table, "brackets", rules.brackets.as_ref());
    insert_serialized(&mut table, "colons", rules.colons.as_ref());
    insert_serialized(&mut table, "commas", rules.commas.as_ref());
    insert_serialized(&mut table, "comments", rules.comments.as_ref());
    insert_serialized(
        &mut table,
        "comments-indentation",
        rules.comments_indentation.as_ref(),
    );
    insert_serialized(&mut table, "document-end", rules.document_end.as_ref());
    insert_serialized(&mut table, "document-start", rules.document_start.as_ref());
    insert_serialized(&mut table, "empty-lines", rules.empty_lines.as_ref());
    insert_serialized(&mut table, "empty-values", rules.empty_values.as_ref());
    insert_serialized(&mut table, "float-values", rules.float_values.as_ref());
    insert_serialized(&mut table, "hyphens", rules.hyphens.as_ref());
    insert_serialized(&mut table, "indentation", rules.indentation.as_ref());
    insert_serialized(&mut table, "key-duplicates", rules.key_duplicates.as_ref());
    insert_serialized(&mut table, "key-ordering", rules.key_ordering.as_ref());
    insert_serialized(&mut table, "line-length", rules.line_length.as_ref());
    insert_serialized(
        &mut table,
        "new-line-at-end-of-file",
        rules.new_line_at_end_of_file.as_ref(),
    );
    insert_serialized(&mut table, "new-lines", rules.new_lines.as_ref());
    insert_serialized(&mut table, "octal-values", rules.octal_values.as_ref());
    insert_serialized(&mut table, "quoted-strings", rules.quoted_strings.as_ref());
    insert_serialized(
        &mut table,
        "trailing-spaces",
        rules.trailing_spaces.as_ref(),
    );
    insert_serialized(&mut table, "truthy", rules.truthy.as_ref());
    table.extend(rules.extra.clone());
    toml::Value::Table(table)
}

/// Convert a typed TOML config model into a TOML value tree.
///
/// # Panics
/// Panics if serializing the typed config into TOML unexpectedly fails.
#[must_use]
pub fn toml_config_to_value(config: &TomlConfig) -> toml::Value {
    let mut table = toml::map::Map::new();
    insert_serialized(&mut table, "yaml-files", config.yaml_files.as_ref());
    insert_serialized(&mut table, "ignore", config.ignore.as_ref());
    insert_serialized(
        &mut table,
        "ignore-from-file",
        config.ignore_from_file.as_ref(),
    );
    insert_serialized(&mut table, "locale", config.locale.as_ref());
    insert_serialized(&mut table, "fix", config.fix.as_ref());
    if let Some(rules) = config.rules.as_ref() {
        table.insert("rules".to_string(), rules_table_to_value(rules));
    }
    table.extend(config.extra.clone());
    toml::Value::Table(table)
}

/// Serialize the generated schema to a JSON value.
///
/// # Panics
/// Panics if serializing the generated schema unexpectedly fails.
#[must_use]
pub fn schema_value() -> Value {
    serde_json::to_value(schema()).expect("serializing generated schema should succeed")
}

/// Serialize the generated schema to a pretty-printed JSON string.
///
/// # Panics
/// Panics if serializing the generated schema unexpectedly fails.
#[must_use]
pub fn schema_string_pretty() -> String {
    serde_json::to_string_pretty(&schema())
        .expect("serializing generated schema should succeed")
}

impl RulesTable {
    fn validate(&self) -> Result<(), String> {
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

pub(crate) fn validate_regex_list(
    patterns: Option<&[String]>,
    _option_name: &str,
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

fn rule_options<T>(entry: Option<&RuleEntry<T>>) -> Option<&RuleOptions<T>> {
    match entry {
        Some(RuleEntry::Options(options)) => Some(options),
        Some(RuleEntry::Bool(_) | RuleEntry::Switch(_)) | None => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuotedStringsRequiredModeForValidation {
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

pub(crate) fn quoted_strings_required_mode_from_yaml_value(
    value: &YamlOwned,
) -> Option<QuotedStringsRequiredModeForValidation> {
    let required = yaml_owned_to_toml_value(value)
        .ok()?
        .try_into::<QuotedStringsRequired>()
        .ok()?;
    Some(quoted_strings_required_mode(Some(&required)))
}

pub(crate) fn validate_key_ordering_patterns(
    patterns: &[String],
) -> Result<(), String> {
    validate_regex_list(Some(patterns), "ignored-keys", |text, err| {
        format!(
            "invalid config: option \"ignored-keys\" of \"key-ordering\" contains invalid regex '{text}': {err}"
        )
    })
}

pub(crate) fn validate_quoted_strings_semantics(
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

    validate_regex_list(extra_required, "extra-required", |text, err| {
        format!(
            "invalid config: regex \"{text}\" in option \"extra-required\" of \"quoted-strings\" is invalid: {err}"
        )
    })?;
    validate_regex_list(extra_allowed, "extra-allowed", |text, err| {
        format!(
            "invalid config: regex \"{text}\" in option \"extra-allowed\" of \"quoted-strings\" is invalid: {err}"
        )
    })?;

    Ok(())
}
