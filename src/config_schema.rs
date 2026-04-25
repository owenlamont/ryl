use schemars::{JsonSchema, Schema, schema_for};
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

/// Convert a typed TOML config model into a TOML value tree.
///
/// # Panics
/// Panics if serializing the typed config into TOML unexpectedly fails.
#[must_use]
pub fn toml_config_to_value(config: &TomlConfig) -> toml::Value {
    toml::Value::try_from(config.clone())
        .expect("serializing typed TOML config should succeed")
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
