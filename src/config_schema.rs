mod serialization;
mod validation;

use std::collections::BTreeMap;

use crate::yaml_dom::{MappingOwned, YamlOwned};
use schemars::{JsonSchema, Schema, schema_for};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub(crate) use serialization::yaml_owned_to_toml_value;
pub use serialization::{
    normalize_toml_config, normalized_config_to_toml_value, toml_config_to_value,
};

/// JSON Schema root for `ryl` TOML configuration.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[schemars(title = "ryl TOML config")]
pub struct TomlConfig {
    /// Glob patterns assigning files to source kinds (`yaml`, `markdown`).
    pub files: Option<FilesTable>,
    /// Behaviour for YAML embedded in markdown (front matter and fenced blocks).
    pub markdown: Option<MarkdownTable>,
    /// Output targets: which format goes to which destination (file/stdout/stderr).
    pub output: Option<OutputTable>,
    /// Ignore patterns, either as one multi-line string or a list of patterns.
    pub ignore: Option<StringOrVec>,
    /// Paths to files that contain ignore patterns.
    #[serde(rename = "ignore-from-file")]
    pub ignore_from_file: Option<StringOrVec>,
    /// Locale identifier used by diagnostics.
    pub locale: Option<String>,
    /// Native fix policy.
    pub fix: Option<FixTable>,
    /// Per-file rule ignores.
    #[serde(rename = "per-file-ignores")]
    pub per_file_ignores: Option<BTreeMap<String, Vec<RuleName>>>,
    /// Per-line rule ignores: suppress rules on lines/files matching a pattern.
    #[serde(rename = "per-line-ignores")]
    pub per_line_ignores: Option<Vec<PerLineIgnore>>,
    /// Rule configuration table.
    pub rules: Option<
        RulesTable<
            TomlQuotedStringsOptions,
            TomlKeyDuplicatesOptions,
            TomlAnchorsOptions,
            CommentsIndentationOptions,
            TomlHyphensOptions,
        >,
    >,
    #[serde(flatten, default)]
    #[schemars(skip)]
    extra: BTreeMap<String, toml::Value>,
}

/// File-to-source-kind glob mapping (ryl-only; TOML). Each kind selects which
/// files are linted as that kind. A file matching more than one kind is an error.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FilesTable {
    /// Glob patterns for files linted directly as YAML.
    pub yaml: Option<Vec<String>>,
    /// Glob patterns for markdown files whose embedded YAML is linted.
    pub markdown: Option<Vec<String>>,
}

/// Markdown embedding behaviour. ryl-only (TOML); yamllint has no equivalent.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MarkdownTable {
    /// Lint the leading YAML front matter block. Defaults to `true`.
    #[serde(rename = "front-matter")]
    pub front_matter: Option<bool>,
    /// Lint fenced `yaml`/`yml` code blocks. Defaults to `true`.
    #[serde(rename = "fenced-blocks")]
    pub fenced_blocks: Option<bool>,
}

/// Output targets (ryl-only; TOML). Each format present is rendered to its destination,
/// so several formats produce several outputs in one run (e.g. console plus a report
/// file). A CLI `--format` overrides this table wholesale. yamllint has no equivalent;
/// see `docs/output-formats.md`.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OutputTable {
    /// Auto-detected console format (GitHub annotations in CI, otherwise colored/plain).
    pub auto: Option<OutputDestination>,
    /// Plain text grouped per file.
    pub standard: Option<OutputDestination>,
    /// Plain text with ANSI colors.
    pub colored: Option<OutputDestination>,
    /// GitHub Actions workflow commands.
    pub github: Option<OutputDestination>,
    /// One `path:line:col: [level] message (rule)` line per diagnostic.
    pub parsable: Option<OutputDestination>,
    /// `JUnit` XML test report.
    pub junit: Option<OutputDestination>,
    /// `GitLab` Code Quality JSON report.
    pub gitlab: Option<OutputDestination>,
}

impl OutputTable {
    /// Every format entry as `(format-name, destination?)`, in a fixed order. The single
    /// authoritative enumeration of the table's fields: validation iterates it, and the
    /// CLI resolves each name to a target, so neither hand-maintains a parallel list.
    #[must_use]
    pub fn entries(&self) -> [(&'static str, Option<&OutputDestination>); 7] {
        [
            ("auto", self.auto.as_ref()),
            ("standard", self.standard.as_ref()),
            ("colored", self.colored.as_ref()),
            ("github", self.github.as_ref()),
            ("parsable", self.parsable.as_ref()),
            ("junit", self.junit.as_ref()),
            ("gitlab", self.gitlab.as_ref()),
        ]
    }
}

/// Where one format's output goes. An absent `path` means the format's default stream
/// (stderr for the console formats, stdout for `junit`/`gitlab`); `"-"` means stdout; any
/// other value is a file path.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OutputDestination {
    pub path: Option<String>,
}

/// JSON Schema root for yamllint-compatible YAML configuration.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[schemars(title = "ryl yamllint-compatible YAML config")]
pub struct YamlConfig {
    /// Preset or config file to extend.
    pub extends: Option<String>,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
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
#[schemars(rename = "RuleEntryFor{T}")]
#[serde(untagged)]
pub enum RuleEntry<T> {
    Bool(bool),
    Switch(RuleSwitch),
    Options(RuleOptions<T>),
}

/// Common rule fields plus rule-specific options.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[schemars(rename = "RuleOptionsFor{T}")]
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
    #[serde(rename = "document-end")]
    DocumentEnd,
    #[serde(rename = "document-start")]
    DocumentStart,
    #[serde(rename = "empty-lines")]
    EmptyLines,
    #[serde(rename = "new-line-at-end-of-file")]
    NewLineAtEndOfFile,
    #[serde(rename = "new-lines")]
    NewLines,
    #[serde(rename = "quoted-strings")]
    QuotedStrings,
    #[serde(rename = "trailing-spaces")]
    TrailingSpaces,
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
    #[serde(rename = "document-end")]
    DocumentEnd,
    #[serde(rename = "document-start")]
    DocumentStart,
    #[serde(rename = "empty-lines")]
    EmptyLines,
    #[serde(rename = "new-line-at-end-of-file")]
    NewLineAtEndOfFile,
    #[serde(rename = "new-lines")]
    NewLines,
    #[serde(rename = "quoted-strings")]
    QuotedStrings,
    #[serde(rename = "trailing-spaces")]
    TrailingSpaces,
}

/// A built-in lint rule name.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
pub enum RuleName {
    #[serde(rename = "anchors")]
    Anchors,
    #[serde(rename = "block-scalar-chomping")]
    BlockScalarChomping,
    #[serde(rename = "braces")]
    Braces,
    #[serde(rename = "brackets")]
    Brackets,
    #[serde(rename = "colons")]
    Colons,
    #[serde(rename = "commas")]
    Commas,
    #[serde(rename = "comments")]
    Comments,
    #[serde(rename = "comments-indentation")]
    CommentsIndentation,
    #[serde(rename = "document-end")]
    DocumentEnd,
    #[serde(rename = "document-start")]
    DocumentStart,
    #[serde(rename = "empty-lines")]
    EmptyLines,
    #[serde(rename = "empty-values")]
    EmptyValues,
    #[serde(rename = "float-values")]
    FloatValues,
    #[serde(rename = "hyphens")]
    Hyphens,
    #[serde(rename = "indentation")]
    Indentation,
    #[serde(rename = "key-duplicates")]
    KeyDuplicates,
    #[serde(rename = "key-ordering")]
    KeyOrdering,
    #[serde(rename = "line-length")]
    LineLength,
    #[serde(rename = "merge-keys")]
    MergeKeys,
    #[serde(rename = "new-line-at-end-of-file")]
    NewLineAtEndOfFile,
    #[serde(rename = "new-lines")]
    NewLines,
    #[serde(rename = "octal-values")]
    OctalValues,
    #[serde(rename = "quoted-strings")]
    QuotedStrings,
    #[serde(rename = "tags")]
    Tags,
    #[serde(rename = "trailing-spaces")]
    TrailingSpaces,
    #[serde(rename = "truthy")]
    Truthy,
    #[serde(rename = "unicode-line-breaks")]
    UnicodeLineBreaks,
}

impl RuleName {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Anchors => "anchors",
            Self::BlockScalarChomping => "block-scalar-chomping",
            Self::Braces => "braces",
            Self::Brackets => "brackets",
            Self::Colons => "colons",
            Self::Commas => "commas",
            Self::Comments => "comments",
            Self::CommentsIndentation => "comments-indentation",
            Self::DocumentEnd => "document-end",
            Self::DocumentStart => "document-start",
            Self::EmptyLines => "empty-lines",
            Self::EmptyValues => "empty-values",
            Self::FloatValues => "float-values",
            Self::Hyphens => "hyphens",
            Self::Indentation => "indentation",
            Self::KeyDuplicates => "key-duplicates",
            Self::KeyOrdering => "key-ordering",
            Self::LineLength => "line-length",
            Self::MergeKeys => "merge-keys",
            Self::NewLineAtEndOfFile => "new-line-at-end-of-file",
            Self::NewLines => "new-lines",
            Self::OctalValues => "octal-values",
            Self::QuotedStrings => "quoted-strings",
            Self::Tags => "tags",
            Self::TrailingSpaces => "trailing-spaces",
            Self::Truthy => "truthy",
            Self::UnicodeLineBreaks => "unicode-line-breaks",
        }
    }
}

/// A `per-line-ignores` rule selector: a built-in rule name, or `ALL` to suppress
/// every rule on a matching line. Untagged so `"ALL"` and rule names share one list.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum PerLineRule {
    All(AllRulesSelector),
    Named(RuleName),
}

/// The `ALL` keyword accepted in a `per-line-ignores` `rules` list.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
pub enum AllRulesSelector {
    #[serde(rename = "ALL")]
    All,
}

impl PerLineRule {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::All(AllRulesSelector::All) => "ALL",
            Self::Named(rule) => rule.as_str(),
        }
    }
}

/// A single `per-line-ignores` entry. Suppresses `rules` on source lines matching
/// `regex` (the whole physical line, unanchored) within files matching `path` (a
/// glob). All present fields must match (logical AND); at least one of `regex`/`path`
/// is required (validated in `validate_toml_config`), so an entry can't disable a rule
/// globally.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PerLineIgnore {
    pub regex: Option<String>,
    pub path: Option<String>,
    pub rules: Vec<PerLineRule>,
}

/// Built-in rule table for TOML config.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
pub struct RulesTable<
    Q = QuotedStringsOptions,
    K = KeyDuplicatesOptions,
    A = AnchorsOptions,
    C = NoOptions,
    H = HyphensOptions,
> {
    pub anchors: Option<RuleEntry<A>>,
    #[serde(rename = "block-scalar-chomping")]
    pub block_scalar_chomping: Option<RuleEntry<NoOptions>>,
    pub braces: Option<RuleEntry<BraceLikeOptions>>,
    pub brackets: Option<RuleEntry<BraceLikeOptions>>,
    pub colons: Option<RuleEntry<ColonsOptions>>,
    pub commas: Option<RuleEntry<CommasOptions>>,
    pub comments: Option<RuleEntry<CommentsOptions>>,
    #[serde(rename = "comments-indentation")]
    pub comments_indentation: Option<RuleEntry<C>>,
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
    pub hyphens: Option<RuleEntry<H>>,
    pub indentation: Option<RuleEntry<IndentationOptions>>,
    #[serde(rename = "key-duplicates")]
    pub key_duplicates: Option<RuleEntry<K>>,
    #[serde(rename = "key-ordering")]
    pub key_ordering: Option<RuleEntry<KeyOrderingOptions>>,
    #[serde(rename = "line-length")]
    pub line_length: Option<RuleEntry<LineLengthOptions>>,
    #[serde(rename = "merge-keys")]
    pub merge_keys: Option<RuleEntry<NoOptions>>,
    #[serde(rename = "new-line-at-end-of-file")]
    pub new_line_at_end_of_file: Option<RuleEntry<NoOptions>>,
    #[serde(rename = "new-lines")]
    pub new_lines: Option<RuleEntry<NewLinesOptions>>,
    #[serde(rename = "octal-values")]
    pub octal_values: Option<RuleEntry<OctalValuesOptions>>,
    #[serde(rename = "quoted-strings")]
    pub quoted_strings: Option<RuleEntry<Q>>,
    pub tags: Option<RuleEntry<TagsOptions>>,
    #[serde(rename = "trailing-spaces")]
    pub trailing_spaces: Option<RuleEntry<NoOptions>>,
    pub truthy: Option<RuleEntry<TruthyOptions>>,
    #[serde(rename = "unicode-line-breaks")]
    pub unicode_line_breaks: Option<RuleEntry<NoOptions>>,
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

/// TOML-only `anchors` options: the yamllint-compatible set plus ryl's
/// `forbid-ambiguous-anchor-alias-names`, which has no YAML-config equivalent.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TomlAnchorsOptions {
    #[serde(rename = "forbid-undeclared-aliases")]
    pub forbid_undeclared_aliases: Option<bool>,
    #[serde(rename = "forbid-duplicated-anchors")]
    pub forbid_duplicated_anchors: Option<bool>,
    #[serde(rename = "forbid-unused-anchors")]
    pub forbid_unused_anchors: Option<bool>,
    #[serde(rename = "forbid-ambiguous-anchor-alias-names")]
    pub forbid_ambiguous_anchor_alias_names: Option<bool>,
}

/// TOML-only `comments-indentation` options. yamllint's rule has none, so the YAML
/// config path keeps `NoOptions` and ryl's `allow-any-open-indent` is TOML-only.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommentsIndentationOptions {
    #[serde(rename = "allow-any-open-indent")]
    pub allow_any_open_indent: Option<bool>,
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

/// TOML-only `hyphens` options: the yamllint-compatible `max-spaces-after` plus ryl's
/// `dash-on-own-line`, which has no YAML-config equivalent.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TomlHyphensOptions {
    #[serde(rename = "max-spaces-after")]
    pub max_spaces_after: Option<i64>,
    #[serde(rename = "dash-on-own-line")]
    pub dash_on_own_line: Option<bool>,
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

/// `key-duplicates` options for TOML config. Extends the yamllint-compatible
/// surface with the ryl-only `check-canonical` and `forbid-merge-key-shadowing`
/// knobs (TOML-only, so future yamllint additions cannot clash with the YAML
/// schema; see `crate::config_schema::KeyDuplicatesOptions`).
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TomlKeyDuplicatesOptions {
    #[serde(rename = "forbid-duplicated-merge-keys")]
    pub forbid_duplicated_merge_keys: Option<bool>,
    #[serde(rename = "check-canonical")]
    pub check_canonical: Option<bool>,
    #[serde(rename = "forbid-merge-key-shadowing")]
    pub forbid_merge_key_shadowing: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KeyOrderingOptions {
    #[serde(rename = "ignored-keys")]
    pub ignored_keys: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TagsOptions {
    #[serde(rename = "forbid-unsafe-tags")]
    pub forbid_unsafe_tags: Option<bool>,
    #[serde(rename = "forbid-removed-types")]
    pub forbid_removed_types: Option<bool>,
    #[serde(rename = "allowed-tags")]
    pub allowed_tags: Option<Vec<String>>,
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

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TomlQuotedStringsOptions {
    #[serde(rename = "quote-type")]
    pub quote_type: Option<QuoteType>,
    pub required: Option<QuotedStringsRequired>,
    #[serde(rename = "extra-required")]
    pub extra_required: Option<Vec<String>>,
    #[serde(rename = "extra-allowed")]
    pub extra_allowed: Option<Vec<String>>,
    #[serde(rename = "allow-quoted-quotes")]
    pub allow_quoted_quotes: Option<bool>,
    #[serde(rename = "allow-double-quotes-for-escaping")]
    pub allow_double_quotes_for_escaping: Option<bool>,
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
    #[serde(rename = "consistent")]
    Consistent,
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

/// Build the JSON Schema for `ryl` TOML configuration.
///
/// # Panics
/// Panics if schemars stops emitting an object schema root for `TomlConfig`.
#[must_use]
pub fn schema() -> Schema {
    let mut schema = schema_for!(TomlConfig);
    // The CLI rejects unrecognised top-level keys (`validate_toml_config`), but schemars
    // cannot emit `additionalProperties: false` on the root because the flattened `extra`
    // catch-all is skipped; set it here so editors flag the same typos the CLI does.
    schema
        .as_object_mut()
        .expect("schema root should be an object")
        .insert("additionalProperties".to_string(), Value::Bool(false));
    schema
}

/// Build the JSON Schema for yamllint-compatible YAML configuration.
///
/// # Panics
/// Panics if schemars stops emitting an object schema root for `YamlConfig`.
#[must_use]
pub fn yaml_schema() -> Schema {
    let mut schema = schema_for!(YamlConfig);
    let root = schema
        .as_object_mut()
        .expect("schema root should be an object");
    root.entry("allOf").or_insert_with(|| {
        json!([
            {
                "not": {
                    "required": ["ignore", "ignore-from-file"]
                }
            }
        ])
    });
    prune_ryl_only_rules(root);
    schema
}

/// Remove the ryl-only rule properties (and any `$defs` they leave
/// unreferenced) from the generated YAML schema, so it advertises only the
/// yamllint-compatible rule surface. The rules themselves are still rejected at
/// parse time; see `crate::rules::RYL_ONLY_RULE_IDS`.
fn prune_ryl_only_rules(root: &mut serde_json::Map<String, Value>) {
    // Only the rules-table definition carries rule-id-named properties, so
    // dropping the ryl-only ids from every definition's `properties` targets it
    // without depending on schemars' generated definition name.
    let defs = root
        .get_mut("$defs")
        .and_then(Value::as_object_mut)
        .expect("generated YAML schema always has a $defs table");
    for def in defs.values_mut() {
        if let Some(properties) =
            def.get_mut("properties").and_then(Value::as_object_mut)
        {
            for rule in crate::rules::RYL_ONLY_RULE_IDS {
                properties.remove(rule);
            }
        }
    }
    remove_unreferenced_defs(root);
}

/// Drop `$defs` entries no longer reachable by any `$ref`, iterating until the
/// set is stable so reference chains collapse fully.
fn remove_unreferenced_defs(root: &mut serde_json::Map<String, Value>) {
    loop {
        let mut referenced = std::collections::BTreeSet::new();
        for value in root.values() {
            collect_defs_refs(value, &mut referenced);
        }
        let defs = root
            .get_mut("$defs")
            .and_then(Value::as_object_mut)
            .expect("generated YAML schema always has a $defs table");
        let orphans: Vec<String> = defs
            .keys()
            .filter(|name| !referenced.contains(name.as_str()))
            .cloned()
            .collect();
        if orphans.is_empty() {
            return;
        }
        for orphan in orphans {
            defs.remove(&orphan);
        }
    }
}

fn collect_defs_refs(
    value: &Value,
    referenced: &mut std::collections::BTreeSet<String>,
) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if key == "$ref" {
                    let name = child
                        .as_str()
                        .and_then(|reference| reference.strip_prefix("#/$defs/"))
                        .expect("schema $ref always targets a local $defs entry");
                    referenced.insert(name.to_string());
                } else {
                    collect_defs_refs(child, referenced);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_defs_refs(item, referenced);
            }
        }
        _ => {}
    }
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
    // A config that defines nothing enables no rules; since rules must be explicitly
    // enabled, an empty config configures the linter to do nothing, so reject it. This
    // covers an empty standalone `.ryl.toml` and an empty `[tool.ryl]` table — the
    // latter is a present-but-empty config (an error), distinct from an *absent*
    // `[tool.ryl]` ("ryl is not configured in this file"), which `toml_document_is_empty`
    // reports as non-empty so discovery keeps looking. Checking the raw document keeps
    // this correct as `TomlConfig` gains fields.
    if toml_document_is_empty(input, pyproject) {
        return Err("invalid config: configuration is empty".to_string());
    }

    if pyproject {
        return toml::from_str::<PyProjectToml>(input)
            .map(|doc| doc.tool.ryl)
            .map_err(|err| format!("failed to parse config data: {err}"));
    }

    toml::from_str::<TomlConfig>(input)
        .map(Some)
        .map_err(|err| format!("failed to parse config data: {err}"))
}

/// Whether the relevant TOML configuration table has no entries: the whole
/// document for a standalone config, or the `[tool.ryl]` subtable for
/// `pyproject.toml`. A document that fails to parse is not treated as empty so the
/// real parse error surfaces below; an absent `[tool.ryl]` is likewise not empty.
fn toml_document_is_empty(input: &str, pyproject: bool) -> bool {
    let Ok(table) = input.parse::<toml::Table>() else {
        return false;
    };
    let config = if pyproject {
        table
            .get("tool")
            .and_then(toml::Value::as_table)
            .and_then(|tool| tool.get("ryl"))
            .and_then(toml::Value::as_table)
    } else {
        Some(&table)
    };
    config.is_some_and(toml::map::Map::is_empty)
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

    if config.extra.contains_key("yaml-files") {
        return Err(
            "invalid config: `yaml-files` is not valid in TOML; use `[files]` with \
             `yaml = [...]` instead"
                .to_string(),
        );
    }

    // Top-level unknown keys cannot use `deny_unknown_fields` (serde forbids it
    // alongside the flattened `extra`), so the same strictness the nested tables
    // get is enforced manually here. `extends`/`yaml-files` are handled above.
    if !config.extra.is_empty() {
        let keys = config
            .extra
            .keys()
            .map(|key| format!("`{key}`"))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "invalid config: unrecognised TOML configuration key(s): {keys}"
        ));
    }

    if let Some(entries) = config.per_line_ignores.as_deref() {
        validation::validate_per_line_ignores(entries)?;
    }

    if let Some(output) = config.output.as_ref() {
        validate_output_table(output)?;
    }

    validate_common_config(
        config.ignore.as_ref(),
        config.ignore_from_file.as_ref(),
        config.rules.as_ref(),
    )
}

/// Reject an empty `path` in an `[output.<format>]` table. An empty path can neither be a
/// file (`File::create("")` fails) nor mean stdout (`"-"`), and the CLI's lexical path
/// normalization rejects empty paths, so it is a config error rather than a silent no-op.
fn validate_output_table(output: &OutputTable) -> Result<(), String> {
    for (name, destination) in output.entries() {
        if let Some(destination) = destination
            && destination.path.as_deref() == Some("")
        {
            return Err(format!(
                "invalid config: output.{name}.path must not be empty (omit it for the \
                 default stream, or use \"-\" for stdout)"
            ));
        }
    }
    Ok(())
}

/// Validate semantic constraints for a typed YAML config model.
///
/// # Errors
/// Returns an error if the typed YAML config violates semantic rules that are
/// not fully captured by deserialization alone.
pub fn validate_yaml_config(config: &YamlConfig) -> Result<(), String> {
    validate_common_config(
        config.ignore.as_ref(),
        config.ignore_from_file.as_ref(),
        config.rules.as_ref(),
    )
}

fn validate_common_config<Q: validation::QuotedStringsOptionSet, K, A, C, H>(
    ignore: Option<&StringOrVec>,
    ignore_from_file: Option<&StringOrVec>,
    rules: Option<&RulesTable<Q, K, A, C, H>>,
) -> Result<(), String> {
    if ignore.is_some() && ignore_from_file.is_some() {
        return Err(
            "invalid config: ignore and ignore-from-file keys cannot be used together"
                .to_string(),
        );
    }

    if let Some(rules) = rules {
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
    pub per_file_ignores: BTreeMap<String, Vec<String>>,
    pub per_line_ignores: Vec<NormalizedPerLineIgnore>,
    pub yaml_file_patterns: Option<Vec<String>>,
    pub markdown_file_patterns: Option<Vec<String>>,
    pub markdown: Option<NormalizedMarkdown>,
    /// Output targets, passed through unchanged (no transformation needed; the CLI
    /// resolves each format/path into a destination at render time).
    pub output: Option<OutputTable>,
    pub locale: Option<String>,
    pub fix: Option<NormalizedFixConfig>,
    pub rules: BTreeMap<String, YamlOwned>,
}

#[derive(Debug, Clone, Default)]
pub struct NormalizedMarkdown {
    pub front_matter: Option<bool>,
    pub fenced_blocks: Option<bool>,
}

/// Post-parse form of one `per-line-ignores` entry. `rules` holds rule ids, or the
/// single literal `"ALL"`; the regex/glob are compiled later in `config.rs`.
#[derive(Debug, Clone, Default)]
pub struct NormalizedPerLineIgnore {
    pub regex: Option<String>,
    pub path: Option<String>,
    pub rules: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ParsedYamlConfig {
    pub extends: Vec<String>,
    pub normalized: NormalizedConfig,
}

#[must_use]
fn load_ignore_patterns(node: &YamlOwned) -> Vec<String> {
    parse_string_items(node, patterns_from_scalar)
}

#[must_use]
fn load_ignore_from_files(node: &YamlOwned) -> Vec<String> {
    parse_string_items(node, |value| vec![value.to_owned()])
}

pub(super) fn patterns_from_scalar(value: &str) -> Vec<String> {
    value
        .lines()
        .map(|line| line.trim_end_matches(['\r']))
        .filter(|line| !line.trim().is_empty())
        .map(std::string::ToString::to_string)
        .collect()
}

fn parse_string_items(
    node: &YamlOwned,
    map: impl Fn(&str) -> Vec<String>,
) -> Vec<String> {
    if let Some(seq) = node.as_sequence() {
        let mut values = Vec::with_capacity(seq.len());
        for item in seq {
            let text = item
                .as_str()
                .expect("typed config validation should guarantee string items");
            values.extend(map(text));
        }
        return values;
    }

    let text = node
        .as_str()
        .expect("typed config validation should guarantee string or sequence values");
    map(text)
}

pub(crate) fn parse_yaml_config(doc: &YamlOwned) -> Result<ParsedYamlConfig, String> {
    if doc.as_mapping().is_none() {
        return Err("invalid config: not a mapping".to_string());
    }

    if doc.as_mapping_get("fix").is_some() {
        return Err(
            "invalid config: fix is only supported in TOML configuration".to_string(),
        );
    }

    if doc.as_mapping_get("per-line-ignores").is_some() {
        return Err(
            "invalid config: per-line-ignores is only supported in TOML configuration"
                .to_string(),
        );
    }

    if doc.as_mapping_get("output").is_some() {
        return Err(
            "invalid config: output is only supported in TOML configuration"
                .to_string(),
        );
    }

    let typed = parse_typed_yaml_config(doc)?;
    validate_yaml_config(&typed)?;

    let normalized = normalize_typed_yaml_config(doc, &typed);
    if let Some(rule) = normalized
        .rules
        .keys()
        .find(|name| crate::rules::RYL_ONLY_RULE_IDS.contains(&name.as_str()))
    {
        return Err(format!(
            "invalid config: `{rule}` is a ryl-only rule and is not available in \
             yamllint-compatible YAML config; configure it in TOML (`[rules.{rule}]`)"
        ));
    }

    Ok(ParsedYamlConfig {
        extends: typed.extends.iter().cloned().collect(),
        normalized,
    })
}

fn parse_typed_yaml_config(doc: &YamlOwned) -> Result<YamlConfig, String> {
    let mut map = MappingOwned::new();
    for (key, value) in doc.as_mapping().expect(
        "parse_yaml_config should only call parse_typed_yaml_config for mappings",
    ) {
        map.insert(key.clone(), value.clone());
    }
    let value = yaml_owned_to_toml_value(&YamlOwned::Mapping(map))
        .map_err(|err| format!("failed to parse config data: {err}"))?;
    value
        .try_into::<YamlConfig>()
        .map_err(|err| format!("failed to parse config data: {err}"))
}

fn normalize_typed_yaml_config(
    doc: &YamlOwned,
    config: &YamlConfig,
) -> NormalizedConfig {
    let mut normalized = serialization::normalize_yaml_config(config);
    normalized.ignore_patterns = doc.as_mapping_get("ignore").map(load_ignore_patterns);
    normalized
}

#[must_use]
pub(crate) fn yaml_rule_level(node: &YamlOwned) -> Option<RuleLevel> {
    if let Some(text) = node.as_str() {
        return if text == "disable" {
            None
        } else {
            Some(RuleLevel::Error)
        };
    }

    if let Some(flag) = node.as_bool() {
        return flag.then_some(RuleLevel::Error);
    }

    node.as_mapping()
        .and_then(|map| {
            map.iter().find_map(|(key, value)| {
                if key.as_str() != Some("level") {
                    return None;
                }
                Some(if value.as_str() == Some("warning") {
                    RuleLevel::Warning
                } else {
                    RuleLevel::Error
                })
            })
        })
        .or(Some(RuleLevel::Error))
}

#[must_use]
pub(crate) fn yaml_rule_filter_patterns(
    node: &YamlOwned,
) -> Option<(Vec<String>, Vec<String>)> {
    let map = node.as_mapping()?;
    let ignore = map
        .iter()
        .find_map(|(key, value)| (key.as_str() == Some("ignore")).then_some(value))
        .map(load_ignore_patterns)
        .unwrap_or_default();
    let ignore_from_files = map
        .iter()
        .find_map(|(key, value)| {
            (key.as_str() == Some("ignore-from-file")).then_some(value)
        })
        .map(load_ignore_from_files)
        .unwrap_or_default();
    Some((ignore, ignore_from_files))
}

/// Serialize the generated schema to a JSON value.
///
/// # Panics
/// Panics if serializing the generated schema unexpectedly fails.
#[must_use]
pub fn schema_value() -> Value {
    serialized_schema_value(schema())
}

#[must_use]
/// Serialize the generated YAML schema to a JSON value.
///
/// # Panics
/// Panics if serializing the generated schema unexpectedly fails.
pub fn yaml_schema_value() -> Value {
    serialized_schema_value(yaml_schema())
}

/// Serialize the generated schema to a pretty-printed JSON string.
///
/// # Panics
/// Panics if serializing the generated schema unexpectedly fails.
#[must_use]
pub fn schema_string_pretty() -> String {
    serialized_schema_string_pretty(&schema())
}

#[must_use]
/// Serialize the generated YAML schema to a pretty-printed JSON string.
///
/// # Panics
/// Panics if serializing the generated schema unexpectedly fails.
pub fn yaml_schema_string_pretty() -> String {
    serialized_schema_string_pretty(&yaml_schema())
}

fn serialized_schema_value(schema: Schema) -> Value {
    serde_json::to_value(schema).expect("serializing generated schema should succeed")
}

fn serialized_schema_string_pretty(schema: &Schema) -> String {
    serde_json::to_string_pretty(schema)
        .expect("serializing generated schema should succeed")
}
