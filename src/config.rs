use std::borrow::Cow;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::directives::PerLineRuleApply;
use crate::yaml_dom::{ScalarOwned, YamlOwned};
use globset::{Glob, GlobMatcher, escape as glob_escape};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use regex::Regex;

use crate::config_schema::{
    FixRuleName as TomlFixRuleName, FixableRuleSelector as TomlFixableRuleSelector,
    NormalizedConfig, NormalizedFixConfig, NormalizedMarkdown, NormalizedPerLineIgnore,
    OutputTable, TomlConfig, normalize_toml_config, normalized_config_to_toml_value,
    parse_toml_config_str, parse_yaml_config, validate_toml_config,
    yaml_rule_filter_patterns, yaml_rule_level,
};
use crate::{conf, decoder};

pub use crate::config_schema::RuleLevel;

/// Maximum depth of `extends` resolution. A cyclic `extends` (a config that
/// extends itself, directly or via a chain) would otherwise recurse until the
/// stack overflows; real chains are only a level or two deep, so this bounds the
/// recursion far above any legitimate use and turns a cycle into a clean error.
const MAX_EXTENDS_DEPTH: usize = 32;

/// How a file should be linted, as resolved from the `[files]` globs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// The whole file is one YAML document.
    Yaml,
    /// A markdown file whose embedded YAML (front matter, fenced blocks) is linted.
    Markdown,
}

/// Abstraction over environment/filesystem to enable full test coverage.
/// Minimal environment abstraction used by tests to cover file system and env-var behavior.
pub trait Env {
    /// Current working directory.
    fn current_dir(&self) -> PathBuf;
    /// Platform configuration directory (e.g., XDG config dir).
    fn config_dir(&self) -> Option<PathBuf>;
    /// Home directory for tilde expansion.
    fn home_dir(&self) -> Option<PathBuf>;
    /// Read file contents.
    ///
    /// # Errors
    /// Returns an error string when the file cannot be read.
    fn read_to_string(&self, p: &Path) -> Result<String, String>;
    fn path_exists(&self, p: &Path) -> bool;
    fn env_var(&self, key: &str) -> Option<String>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemEnv;

impl Env for SystemEnv {
    fn current_dir(&self) -> PathBuf {
        PathBuf::from(".")
    }
    fn config_dir(&self) -> Option<PathBuf> {
        // Check XDG_CONFIG_HOME first (for cross-platform compatibility)
        env::var("XDG_CONFIG_HOME")
            .ok()
            .map(PathBuf::from)
            .or_else(dirs_next::config_dir)
    }
    fn home_dir(&self) -> Option<PathBuf> {
        dirs_next::home_dir()
    }
    fn read_to_string(&self, p: &Path) -> Result<String, String> {
        let bytes = match fs::read(p) {
            Ok(data) => data,
            Err(err) => {
                return Err(format!(
                    "failed to read config file {}: {err}",
                    p.display()
                ));
            }
        };
        match decoder::decode_bytes(&bytes) {
            Ok(text) => Ok(text),
            Err(err) => {
                Err(format!("failed to read config file {}: {err}", p.display()))
            }
        }
    }
    fn path_exists(&self, p: &Path) -> bool {
        p.exists()
    }
    fn env_var(&self, key: &str) -> Option<String> {
        env::var(key).ok()
    }
}

struct ClosureEnv<'a> {
    get: &'a dyn Fn(&str) -> Option<String>,
}

impl Env for ClosureEnv<'_> {
    fn current_dir(&self) -> PathBuf {
        SystemEnv.current_dir()
    }

    fn config_dir(&self) -> Option<PathBuf> {
        // Resolve config_dir purely from the injected XDG_CONFIG_HOME so this injection env
        // stays hermetic and never reads the real config dir; an un-injected value yields
        // None (production uses SystemEnv, which adds the platform-native fallback).
        (self.get)("XDG_CONFIG_HOME").map(PathBuf::from)
    }

    fn home_dir(&self) -> Option<PathBuf> {
        (self.get)("HOME")
            .or_else(|| (self.get)("USERPROFILE"))
            .map(PathBuf::from)
            .or_else(|| SystemEnv.home_dir())
    }

    fn read_to_string(&self, p: &Path) -> Result<String, String> {
        SystemEnv.read_to_string(p)
    }

    fn path_exists(&self, p: &Path) -> bool {
        SystemEnv.path_exists(p)
    }

    fn env_var(&self, key: &str) -> Option<String> {
        (self.get)(key)
    }
}

/// Minimal configuration model compatible with yamllint discovery precedence.
#[derive(Debug, Clone)]
pub struct YamlLintConfig {
    ignore_patterns: Vec<String>,
    ignore_from_files: Vec<String>,
    #[allow(clippy::struct_field_names)]
    ignore_matcher: Option<Gitignore>,
    per_file_ignores: BTreeMap<String, Vec<String>>,
    per_file_ignore_matchers: Vec<PerFileIgnore>,
    /// Resolved `per-line-ignores` spec, kept (alongside the compiled matchers) so the
    /// runtime config can serialize back to TOML for `--migrate-configs`.
    per_line_ignores: Vec<NormalizedPerLineIgnore>,
    per_line_ignore_matchers: Vec<PerLineIgnoreMatcher>,
    rule_names: Vec<String>,
    rules: std::collections::BTreeMap<String, RuleConfig>,
    yaml_file_patterns: Vec<String>,
    yaml_matcher: Option<Gitignore>,
    markdown_file_patterns: Vec<String>,
    markdown_matcher: Option<Gitignore>,
    /// Set when the `--markdown` flag injected the default markdown globs; lets
    /// markdown win over yaml for an overlapping file instead of hard-erroring, so
    /// the flag can't break a run whose yaml globs happen to match `.md`.
    markdown_from_flag: bool,
    lint_markdown_front_matter: bool,
    lint_markdown_fenced_blocks: bool,
    /// Output targets from a TOML `[output]` table (ryl-only). Run-level: read once from
    /// the config governing the invocation, then resolved into destinations by the CLI.
    output: Option<OutputTable>,
    locale: Option<String>,
    fix: FixConfig,
}

const DEFAULT_YAML_FILE_PATTERNS: [&str; 3] = ["*.yaml", "*.yml", ".yamllint"];
const DEFAULT_MARKDOWN_FILE_PATTERNS: [&str; 5] =
    ["*.md", "*.markdown", "*.mdx", "*.qmd", "*.Rmd"];

#[derive(Debug, Clone, Default)]
struct RuleFilter {
    patterns: Vec<String>,
    from_files: Vec<String>,
    matcher: Option<Gitignore>,
}

#[derive(Debug, Clone)]
struct RuleConfig {
    value: YamlOwned,
    filter: Option<RuleFilter>,
}

#[derive(Debug, Clone)]
struct PerFileIgnore {
    basename_matcher: GlobMatcher,
    absolute_matcher: GlobMatcher,
    negated: bool,
    rules: Vec<String>,
}

impl PerFileIgnore {
    fn new(pattern: &str, rules: Vec<String>, base_dir: &Path) -> Result<Self, String> {
        let (negated, pattern) = split_negation(pattern);
        let absolute_pattern = absolute_glob_pattern(pattern, base_dir);
        let basename_matcher = Glob::new(pattern)
            .map_err(|err| {
                format!(
                    "invalid config: per-file-ignores pattern '{pattern}' is invalid: {err}"
                )
            })?
            .compile_matcher();
        let absolute_matcher = Glob::new(&absolute_pattern)
            .expect("absolute per-file ignore pattern should compile after validation")
            .compile_matcher();
        Ok(Self {
            basename_matcher,
            absolute_matcher,
            negated,
            rules,
        })
    }

    fn matches(&self, path: &Path, base_dir: &Path) -> bool {
        let matched = glob_path_matches(
            &self.basename_matcher,
            &self.absolute_matcher,
            path,
            base_dir,
        );
        // Negation inverts the whole match: `!(filename || absolute)`.
        matched != self.negated
    }
}

/// Split a leading `!` negation marker off a glob pattern, returning `(negated, rest)`.
/// Shared by the per-file and per-line ignore matchers so both honour `!` identically.
fn split_negation(pattern: &str) -> (bool, &str) {
    pattern
        .strip_prefix('!')
        .map_or((false, pattern), |rest| (true, rest))
}

/// Whether `path` matches either glob: its basename against `basename`, or its
/// (base-dir-resolved) absolute form against `absolute`. Shared by the per-file and
/// per-line ignore matchers so both interpret a path glob identically.
fn glob_path_matches(
    basename: &GlobMatcher,
    absolute: &GlobMatcher,
    path: &Path,
    base_dir: &Path,
) -> bool {
    let filename_matches = path
        .file_name()
        .is_some_and(|file_name| basename.is_match(Path::new(file_name)));
    let absolute_path = if path.is_absolute() {
        Cow::Borrowed(path)
    } else {
        Cow::Owned(base_dir.join(path))
    };
    filename_matches || absolute.is_match(absolute_path.as_ref())
}

fn absolute_glob_pattern(pattern: &str, base_dir: &Path) -> String {
    if Path::new(pattern).is_absolute() {
        pattern.to_owned()
    } else {
        let mut pattern_with_base = glob_escape(&base_dir.to_string_lossy());
        if !pattern_with_base.is_empty()
            && !pattern_with_base.ends_with(std::path::MAIN_SEPARATOR)
        {
            pattern_with_base.push(std::path::MAIN_SEPARATOR);
        }
        pattern_with_base.push_str(pattern);
        pattern_with_base
    }
}

/// The rules a `per-line-ignores` entry suppresses, resolved to `&'static str` ids;
/// `None` means every rule (the `ALL` selector). This mirrors
/// `directives::insert_rules`'s `None`-means-all convention, so the directives builder
/// expands `ALL` in one place rather than here.
fn resolve_per_line_rules(rules: &[String]) -> Option<Vec<&'static str>> {
    if rules.iter().any(|rule| rule == "ALL") {
        return None;
    }
    Some(
        rules
            .iter()
            .map(|rule| {
                crate::rules::ALL_RULE_IDS
                    .iter()
                    .copied()
                    .find(|id| *id == rule)
                    .expect("per-line-ignores rule names are validated rule ids")
            })
            .collect(),
    )
}

/// Compiled `per-line-ignores` entry: an optional path glob (the basename + absolute
/// matcher pair, as per-file-ignores), an optional line regex, and the rules to
/// suppress (`None` = all). No path glob applies to every file; line matching happens
/// in the directives builder. At least one of path/regex is guaranteed by validation.
#[derive(Debug, Clone)]
struct PerLineIgnoreMatcher {
    path_glob: Option<(GlobMatcher, GlobMatcher)>,
    /// Whether the path glob was `!`-negated (matches files *not* matching it), as
    /// per-file-ignores. Only meaningful when `path_glob` is `Some`.
    path_negated: bool,
    regex: Option<Regex>,
    rules: Option<Vec<&'static str>>,
}

impl PerLineIgnoreMatcher {
    /// Build a compiled matcher. Infallible: config validation
    /// (`validate_per_line_ignores`) has already proven every regex/glob compiles, so
    /// the only failure path is unreachable and is documented with `expect`.
    fn new(entry: &NormalizedPerLineIgnore, base_dir: &Path) -> Self {
        let (path_negated, path_glob) = match entry.path.as_deref() {
            Some(raw) => {
                let (negated, pattern) = split_negation(raw);
                let basename = Glob::new(pattern)
                    .expect("per-line-ignores `path` compiles after config validation")
                    .compile_matcher();
                let absolute = Glob::new(&absolute_glob_pattern(pattern, base_dir))
                    .expect(
                        "absolute per-line ignore pattern compiles after validation",
                    )
                    .compile_matcher();
                (negated, Some((basename, absolute)))
            }
            None => (false, None),
        };
        let regex = entry.regex.as_deref().map(|pattern| {
            Regex::new(pattern)
                .expect("per-line-ignores `regex` compiles after config validation")
        });
        Self {
            path_glob,
            path_negated,
            regex,
            rules: resolve_per_line_rules(&entry.rules),
        }
    }

    /// Whether this entry applies to `path` (true when it has no path constraint).
    fn path_matches(&self, path: &Path, base_dir: &Path) -> bool {
        self.path_glob.as_ref().is_none_or(|(basename, absolute)| {
            glob_path_matches(basename, absolute, path, base_dir) != self.path_negated
        })
    }

    fn as_apply(&self) -> PerLineRuleApply<'_> {
        PerLineRuleApply {
            regex: self.regex.as_ref(),
            rules: self.rules.as_deref(),
        }
    }
}

fn build_per_line_ignores(
    entries: &[NormalizedPerLineIgnore],
    base_dir: &Path,
) -> Vec<PerLineIgnoreMatcher> {
    entries
        .iter()
        .map(|entry| PerLineIgnoreMatcher::new(entry, base_dir))
        .collect()
}

impl RuleConfig {
    fn new(value: YamlOwned) -> Self {
        Self {
            filter: rule_filter_from_node(&value),
            value,
        }
    }

    fn merge(&mut self, value: &YamlOwned) {
        deep_merge_yaml_owned(&mut self.value, value);
        self.filter = rule_filter_from_node(&self.value);
    }

    fn level(&self) -> Option<RuleLevel> {
        yaml_rule_level(&self.value)
    }

    fn option(&self, option: &str) -> Option<&YamlOwned> {
        self.value.as_mapping_get(option)
    }

    fn build_filter(&mut self, envx: &dyn Env, base_dir: &Path) -> Result<(), String> {
        let Some(filter) = &mut self.filter else {
            return Ok(());
        };
        build_rule_filter(filter, envx, base_dir)
    }

    fn is_ignored(&self, path: &Path, base_dir: &Path) -> bool {
        self.filter
            .as_ref()
            .and_then(|filter| filter.matcher.as_ref())
            .is_some_and(|matcher| path_matches_ignore(matcher, path, base_dir))
    }
}

fn rule_filter_from_node(node: &YamlOwned) -> Option<RuleFilter> {
    yaml_rule_filter_patterns(node).map(|(patterns, from_files)| RuleFilter {
        patterns,
        from_files,
        matcher: None,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixConfig {
    fixable: Vec<FixRuleSelector>,
    unfixable: Vec<FixRule>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixRule {
    Braces,
    Brackets,
    Commas,
    Comments,
    CommentsIndentation,
    DocumentEnd,
    DocumentStart,
    EmptyLines,
    NewLineAtEndOfFile,
    NewLines,
    QuotedStrings,
    TrailingSpaces,
}

impl FixRule {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "braces" => Some(Self::Braces),
            "brackets" => Some(Self::Brackets),
            "commas" => Some(Self::Commas),
            "comments" => Some(Self::Comments),
            "comments-indentation" => Some(Self::CommentsIndentation),
            "document-end" => Some(Self::DocumentEnd),
            "document-start" => Some(Self::DocumentStart),
            "empty-lines" => Some(Self::EmptyLines),
            "new-line-at-end-of-file" => Some(Self::NewLineAtEndOfFile),
            "new-lines" => Some(Self::NewLines),
            "quoted-strings" => Some(Self::QuotedStrings),
            "trailing-spaces" => Some(Self::TrailingSpaces),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixRuleSelector {
    All,
    Rule(FixRule),
}

impl Default for FixConfig {
    fn default() -> Self {
        Self {
            fixable: vec![FixRuleSelector::All],
            unfixable: Vec::new(),
        }
    }
}

impl FixConfig {
    #[must_use]
    pub fn fixable(&self) -> &[FixRuleSelector] {
        &self.fixable
    }

    #[must_use]
    pub fn unfixable(&self) -> &[FixRule] {
        &self.unfixable
    }

    #[must_use]
    pub fn allows_rule(&self, rule: &str) -> bool {
        let Some(rule) = FixRule::parse(rule) else {
            return false;
        };
        if self.unfixable.contains(&rule) {
            return false;
        }

        self.fixable.iter().any(|entry| match entry {
            FixRuleSelector::All => true,
            FixRuleSelector::Rule(candidate) => *candidate == rule,
        })
    }

    #[must_use]
    fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

impl Default for YamlLintConfig {
    fn default() -> Self {
        Self {
            ignore_patterns: Vec::new(),
            ignore_from_files: Vec::new(),
            ignore_matcher: None,
            per_file_ignores: BTreeMap::new(),
            per_file_ignore_matchers: Vec::new(),
            per_line_ignores: Vec::new(),
            per_line_ignore_matchers: Vec::new(),
            rule_names: Vec::new(),
            rules: std::collections::BTreeMap::new(),
            yaml_file_patterns: DEFAULT_YAML_FILE_PATTERNS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            yaml_matcher: None,
            markdown_file_patterns: Vec::new(),
            markdown_matcher: None,
            markdown_from_flag: false,
            lint_markdown_front_matter: true,
            lint_markdown_fenced_blocks: true,
            output: None,
            locale: None,
            fix: FixConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub config_file: Option<PathBuf>,
    pub config_data: Option<String>,
}

impl YamlLintConfig {
    /// Parse configuration data without filesystem access.
    ///
    /// # Errors
    /// Returns an error when `extends` is used and the config requires filesystem access.
    pub fn from_yaml_str(s: &str) -> Result<Self, String> {
        Self::from_yaml_str_with_env(s, None, None)
    }

    /// Build a config from standalone TOML configuration text, without filesystem
    /// access (like [`Self::from_yaml_str`]). Parsing is separated from discovery: this
    /// does not run [`Self::finalize`], so path-based matchers (`per-file-ignores`,
    /// `per-line-ignores`, per-rule `ignore`) are not built here &mdash; the lint-ready
    /// config comes from `discover_config`, which owns I/O and finalization.
    ///
    /// # Errors
    /// Returns an error when the TOML is empty or cannot be parsed into a valid
    /// config.
    ///
    /// # Panics
    /// Cannot panic in practice: the `None` result is reserved for an absent
    /// `[tool.ryl]` table in `pyproject.toml`, which standalone parsing
    /// (`pyproject = false`) never produces &mdash; an empty config is an error.
    pub fn from_toml_str(s: &str) -> Result<Self, String> {
        Self::from_toml_str_with_env(s, None, None, false)
            .map(|config| config.expect("standalone TOML config is never absent"))
    }

    fn extend_from_entry(
        &mut self,
        entry: &str,
        envx: Option<&dyn Env>,
        base_dir: &Path,
        depth: usize,
    ) -> Result<(), String> {
        if let Some(builtin) = conf::builtin(entry) {
            let base = Self::from_yaml_str(builtin).expect("builtin preset must parse");
            self.merge_from(base);
            return Ok(());
        }

        let Some(envx) = envx else {
            return Err(format!(
                "invalid config: extends '{entry}' requires filesystem access for resolution"
            ));
        };

        let resolved = resolve_extend_path(entry, envx, Some(base_dir));
        if is_toml_path(&resolved) {
            return Err(format!(
                "invalid config: extends cannot reference TOML configuration {}",
                resolved.display()
            ));
        }
        let data = match envx.read_to_string(&resolved) {
            Ok(text) => text,
            Err(err) => {
                return Err(format!(
                    "failed to read extended config {}: {err}",
                    resolved.display()
                ));
            }
        };
        let parent_dir = resolved
            .parent()
            .map_or_else(|| base_dir.to_path_buf(), Path::to_path_buf);
        let base = Self::from_yaml_str_with_env_depth(
            &data,
            Some(envx),
            Some(&parent_dir),
            depth + 1,
        )?;
        self.merge_from(base);
        Ok(())
    }
    #[must_use]
    pub fn ignore_patterns(&self) -> &[String] {
        &self.ignore_patterns
    }

    /// Drop `ignore-from-file` so the serialized config emits the patterns `finalize`
    /// already resolved into `ignore`. User-global migration calls this so the converted
    /// config is self-contained and keeps working after it moves to ryl's config directory
    /// (the original relative path would otherwise dangle). Call only after `finalize`.
    pub fn inline_resolved_ignore_from_file(&mut self) {
        self.ignore_from_files.clear();
    }

    /// Whether any rule sets a *relative* rule-level `ignore-from-file`. User-global
    /// migration refuses these: the rule config is serialized verbatim, so a relative path
    /// cannot be relocated to ryl's config directory without rewriting it (the top-level
    /// case is inlined instead). An absolute rule-level path is left as-is — moving the
    /// config cannot invalidate it. Call only after `finalize`, which populates rule
    /// filters.
    #[must_use]
    pub fn has_relative_rule_level_ignore_from_file(&self) -> bool {
        self.rules
            .values()
            .filter_map(|rule| rule.filter.as_ref())
            .flat_map(|filter| filter.from_files.iter())
            .any(|path| !Path::new(path).is_absolute())
    }

    #[must_use]
    pub fn rule_names(&self) -> &[String] {
        &self.rule_names
    }

    /// Whether the configuration enables at least one rule (one with a severity
    /// level). A configuration that enables none would lint nothing; the lint CLI
    /// rejects that, though config resolution itself (e.g. for migration) does not.
    #[must_use]
    pub fn enables_any_rule(&self) -> bool {
        self.rules.values().any(|rule| rule.level().is_some())
    }

    #[must_use]
    pub fn rule_level(&self, rule: &str) -> Option<RuleLevel> {
        self.rules.get(rule)?.level()
    }

    #[must_use]
    pub fn rule_option_str(&self, rule: &str, option: &str) -> Option<&str> {
        self.rule_option(rule, option).and_then(YamlOwned::as_str)
    }

    #[must_use]
    pub fn rule_option(&self, rule: &str, option: &str) -> Option<&YamlOwned> {
        self.rules.get(rule)?.option(option)
    }

    #[must_use]
    pub fn rule_option_bool(&self, rule: &str, option: &str, default: bool) -> bool {
        self.rule_option(rule, option)
            .and_then(YamlOwned::as_bool)
            .unwrap_or(default)
    }

    #[must_use]
    pub fn rule_option_int(&self, rule: &str, option: &str, default: i64) -> i64 {
        self.rule_option(rule, option)
            .and_then(YamlOwned::as_integer)
            .unwrap_or(default)
    }

    #[must_use]
    pub fn locale(&self) -> Option<&str> {
        self.locale.as_deref()
    }

    #[must_use]
    pub const fn fix(&self) -> &FixConfig {
        &self.fix
    }

    fn build_file_kind_matchers(&mut self, base_dir: &Path) {
        self.yaml_matcher = build_glob_matcher(base_dir, &self.yaml_file_patterns);
        self.markdown_matcher =
            build_glob_matcher(base_dir, &self.markdown_file_patterns);
    }

    /// Returns true when `path` should be ignored according to config patterns.
    /// Matching is performed on the path relative to `base_dir`.
    #[must_use]
    pub fn is_file_ignored(&self, path: &Path, base_dir: &Path) -> bool {
        self.ignore_matcher
            .as_ref()
            .is_some_and(|matcher| path_matches_ignore(matcher, path, base_dir))
    }

    /// Disable filename-based rule ignores so every enabled rule runs.
    ///
    /// Use when linting content that has no real path (e.g. stdin without
    /// `--stdin-filename`), so per-file-ignores and per-rule `ignore` patterns
    /// cannot accidentally match the synthetic label.
    pub fn disable_path_based_rule_ignores(&mut self) {
        self.per_file_ignore_matchers.clear();
        // A per-line entry with a path constraint can't match a synthetic label, so
        // drop it; pure-regex entries are content-based and still apply to stdin.
        self.per_line_ignore_matchers
            .retain(|matcher| matcher.path_glob.is_none());
        for rule in self.rules.values_mut() {
            if let Some(filter) = rule.filter.as_mut() {
                filter.matcher = None;
            }
        }
    }

    #[must_use]
    pub fn is_rule_ignored(&self, rule: &str, path: &Path, base_dir: &Path) -> bool {
        self.rules
            .get(rule)
            .is_some_and(|config| config.is_ignored(path, base_dir))
            || self
                .per_file_ignore_matchers
                .iter()
                .filter(|entry| entry.matches(path, base_dir))
                .any(|entry| entry.rules.iter().any(|candidate| candidate == rule))
    }

    /// The `per-line-ignores` entries applying to `path` (path glob matches, or none),
    /// as virtual-disable-line applies for the directives builder. Empty when none are
    /// configured, so directive-less linting pays nothing.
    #[must_use]
    pub(crate) fn per_line_applies(
        &self,
        path: &Path,
        base_dir: &Path,
    ) -> Vec<PerLineRuleApply<'_>> {
        self.per_line_ignore_matchers
            .iter()
            .filter(|matcher| matcher.path_matches(path, base_dir))
            .map(PerLineIgnoreMatcher::as_apply)
            .collect()
    }

    #[must_use]
    pub fn is_yaml_candidate(&self, path: &Path, base_dir: &Path) -> bool {
        if let Some(matcher) = &self.yaml_matcher {
            let rel: Cow<'_, Path> = path.strip_prefix(base_dir).map_or_else(
                |_| Cow::Owned(path.file_name().map(PathBuf::from).unwrap_or_default()),
                Cow::Borrowed,
            );
            let matched =
                matcher.matched_path_or_any_parents(rel.as_ref(), path.is_dir());
            return matched.is_ignore();
        }
        crate::discover::is_yaml_path(path)
    }

    /// Returns true when `path` is a markdown file to scan for embedded YAML.
    /// Always false when markdown linting is disabled (no `[files].markdown`).
    #[must_use]
    pub fn is_markdown_candidate(&self, path: &Path, base_dir: &Path) -> bool {
        let Some(matcher) = &self.markdown_matcher else {
            return false;
        };
        let rel: Cow<'_, Path> = path.strip_prefix(base_dir).map_or_else(
            |_| Cow::Owned(path.file_name().map(PathBuf::from).unwrap_or_default()),
            Cow::Borrowed,
        );
        matcher
            .matched_path_or_any_parents(rel.as_ref(), path.is_dir())
            .is_ignore()
    }

    /// Resolve which source kind `path` should be linted as, per the `[files]`
    /// globs. Returns `Ok(None)` when no kind matches, and `Err` when a file
    /// matches more than one kind (an unresolvable configuration).
    ///
    /// # Errors
    /// Returns `Err` if `path` matches both the `yaml` and `markdown` globs.
    pub fn source_kind(
        &self,
        path: &Path,
        base_dir: &Path,
    ) -> Result<Option<SourceKind>, String> {
        match (
            self.is_yaml_candidate(path, base_dir),
            self.is_markdown_candidate(path, base_dir),
        ) {
            // `--markdown` injected the markdown globs, so honour the explicit
            // request: treat an overlapping file as markdown rather than aborting.
            (true, true) if self.markdown_from_flag => Ok(Some(SourceKind::Markdown)),
            (true, true) => Err(format!(
                "{}: matches both `yaml` and `markdown` in [files]; a file may \
                 belong to only one source kind",
                path.display()
            )),
            (true, false) => Ok(Some(SourceKind::Yaml)),
            (false, true) => Ok(Some(SourceKind::Markdown)),
            (false, false) => Ok(None),
        }
    }

    /// Enable markdown linting with default globs (`*.md`, `*.markdown`, `*.mdx`,
    /// `*.qmd`, `*.Rmd`) when none are configured. Backs the `--markdown` CLI flag
    /// so embedded YAML can be linted without editing config. A no-op when
    /// `[files].markdown` already lists globs.
    pub fn enable_default_markdown(&mut self, base_dir: &Path) {
        if self.markdown_file_patterns.is_empty() {
            self.markdown_file_patterns = DEFAULT_MARKDOWN_FILE_PATTERNS
                .iter()
                .map(|pattern| (*pattern).to_string())
                .collect();
            self.markdown_matcher =
                build_glob_matcher(base_dir, &self.markdown_file_patterns);
            self.markdown_from_flag = true;
        }
    }

    #[must_use]
    pub fn markdown_front_matter(&self) -> bool {
        self.lint_markdown_front_matter
    }

    #[must_use]
    pub fn markdown_fenced_blocks(&self) -> bool {
        self.lint_markdown_fenced_blocks
    }

    /// The TOML `[output]` table, if the config declared one (ryl-only). The CLI resolves
    /// it into output destinations; a CLI `--format` overrides it wholesale.
    #[must_use]
    pub fn output(&self) -> Option<&OutputTable> {
        self.output.as_ref()
    }

    fn from_yaml_str_with_env(
        s: &str,
        envx: Option<&dyn Env>,
        base_dir: Option<&Path>,
    ) -> Result<Self, String> {
        Self::from_yaml_str_with_env_depth(s, envx, base_dir, 0)
    }

    /// `depth` tracks `extends` recursion so a cycle is rejected (see
    /// [`MAX_EXTENDS_DEPTH`]) rather than overflowing the stack.
    fn from_yaml_str_with_env_depth(
        s: &str,
        envx: Option<&dyn Env>,
        base_dir: Option<&Path>,
        depth: usize,
    ) -> Result<Self, String> {
        if depth > MAX_EXTENDS_DEPTH {
            return Err(
                "invalid config: extends nested too deeply (possible cyclic extends)"
                    .to_string(),
            );
        }
        let docs = YamlOwned::load_from_str(s)
            .map_err(|e| format!("failed to parse config data: {e}"))?;
        // An empty document stream (empty/whitespace/comment-only config) yields no
        // docs; treat it as a non-mapping so it reports "invalid config: not a
        // mapping" (matching yamllint) instead of panicking on `docs[0]`.
        Self::from_doc_with_env(
            docs.first().unwrap_or(&YamlOwned::BadValue),
            envx,
            base_dir,
            depth,
        )
    }

    fn from_toml_str_with_env(
        s: &str,
        envx: Option<&dyn Env>,
        base_dir: Option<&Path>,
        pyproject: bool,
    ) -> Result<Option<Self>, String> {
        let Some(typed) = parse_toml_config_str(s, pyproject)? else {
            return Ok(None);
        };
        validate_toml_config(&typed)?;
        let _ = (envx, base_dir);
        Ok(Some(Self::from_typed_toml_config_with_env(&typed)))
    }

    fn from_typed_toml_config_with_env(config: &TomlConfig) -> Self {
        let normalized = normalize_toml_config(config);
        let mut cfg = Self::default();
        cfg.apply_normalized_config(normalized);
        cfg
    }

    fn from_doc_with_env(
        doc: &YamlOwned,
        envx: Option<&dyn Env>,
        base_dir: Option<&Path>,
        depth: usize,
    ) -> Result<Self, String> {
        let parsed = parse_yaml_config(doc)?;
        let mut cfg = Self::default();
        let base_path = base_dir.unwrap_or_else(|| Path::new(""));
        for entry in &parsed.extends {
            cfg.extend_from_entry(entry, envx, base_path, depth)?;
        }
        cfg.apply_normalized_config(parsed.normalized);

        Ok(cfg)
    }

    fn merge_from(&mut self, mut other: Self) {
        // Merge ignore patterns (append, then dedup later during matcher build)
        self.ignore_patterns.append(&mut other.ignore_patterns);
        self.ignore_from_files.append(&mut other.ignore_from_files);
        // Merge rules deeply and accumulate names
        for (name, rule) in other.rules {
            self.merge_rule(&name, &rule.value);
        }
        if !other.yaml_file_patterns.is_empty() {
            self.yaml_file_patterns = other.yaml_file_patterns;
        }
        self.per_file_ignores = other.per_file_ignores;
        self.per_line_ignores = other.per_line_ignores;
        self.locale = self.locale.take().or(other.locale);
    }

    fn apply_normalized_config(&mut self, normalized: NormalizedConfig) {
        if let Some(ignore) = normalized.ignore_patterns {
            self.ignore_patterns.clear();
            self.ignore_from_files.clear();
            self.ignore_patterns = ignore;
        }

        if let Some(ignore_from_file) = normalized.ignore_from_files {
            self.ignore_patterns.clear();
            self.ignore_from_files = ignore_from_file;
        }

        if let Some(yaml_files) = normalized.yaml_file_patterns {
            self.yaml_file_patterns.clear();
            self.yaml_file_patterns = yaml_files;
        }

        if let Some(markdown_files) = normalized.markdown_file_patterns {
            self.markdown_file_patterns = markdown_files;
        }

        if let Some(markdown) = normalized.markdown {
            if let Some(front_matter) = markdown.front_matter {
                self.lint_markdown_front_matter = front_matter;
            }
            if let Some(fenced_blocks) = markdown.fenced_blocks {
                self.lint_markdown_fenced_blocks = fenced_blocks;
            }
        }

        // A child config's `[output]` replaces an `extends:` base's wholesale (last wins);
        // omitting it preserves the base, mirroring the other top-level tables here.
        if normalized.output.is_some() {
            self.output = normalized.output;
        }

        if !normalized.per_file_ignores.is_empty() {
            self.per_file_ignores = normalized.per_file_ignores;
        }

        if !normalized.per_line_ignores.is_empty() {
            self.per_line_ignores = normalized.per_line_ignores;
        }

        if let Some(locale) = normalized.locale {
            self.locale = Some(locale);
        }

        if let Some(fix) = normalized.fix.as_ref() {
            self.fix = typed_fix_config(fix);
        }

        for (name, value) in &normalized.rules {
            self.merge_rule(name, value);
        }
    }

    /// Render the effective configuration as TOML.
    ///
    /// # Panics
    /// Panics if serializing a validated config as TOML fails unexpectedly.
    #[must_use]
    pub fn to_toml_string(&self) -> String {
        let normalized = normalized_config_from_runtime(self);
        toml::to_string_pretty(&normalized_config_to_toml_value(&normalized))
            .expect("serializing TOML Value should not fail")
    }

    fn finalize(&mut self, envx: &dyn Env, base_dir: &Path) -> Result<(), String> {
        // Reject unknown/misspelled rule names (matching yamllint's "no such rule").
        // ryl does not support custom/unrecognised rules: an unknown rule is never
        // dispatched by `lint_str`, so without this a typo lints nothing and a config
        // whose only entries are unknown would also slip past the "no rules enabled"
        // guard.
        if let Some(unknown) = self
            .rule_names
            .iter()
            .find(|name| !crate::rules::ALL_RULE_IDS.contains(&name.as_str()))
        {
            return Err(format!("invalid config: no such rule: \"{unknown}\""));
        }

        let (matcher, extra_patterns) = build_ignore_matcher(
            &self.ignore_patterns,
            &self.ignore_from_files,
            envx,
            base_dir,
        )?;
        if !extra_patterns.is_empty() {
            self.ignore_patterns.extend(extra_patterns);
        }
        self.ignore_matcher = matcher;
        self.per_file_ignore_matchers =
            build_per_file_ignores(&self.per_file_ignores, base_dir)?;
        self.per_line_ignore_matchers =
            build_per_line_ignores(&self.per_line_ignores, base_dir);

        self.build_file_kind_matchers(base_dir);

        for rule in self.rules.values_mut() {
            rule.build_filter(envx, base_dir)?;
        }
        Ok(())
    }

    fn merge_rule(&mut self, name: &str, value: &YamlOwned) {
        if let Some(dst) = self.rules.get_mut(name) {
            dst.merge(value);
        } else {
            self.rules
                .insert(name.to_owned(), RuleConfig::new(value.clone()));
        }
        if !self.rule_names.iter().any(|entry| entry == name) {
            self.rule_names.push(name.to_owned());
        }
    }
}

fn build_rule_filter(
    filter: &mut RuleFilter,
    envx: &dyn Env,
    base_dir: &Path,
) -> Result<(), String> {
    let (matcher, extra_patterns) =
        build_ignore_matcher(&filter.patterns, &filter.from_files, envx, base_dir)?;
    if !extra_patterns.is_empty() {
        filter.patterns.extend(extra_patterns);
    }
    filter.matcher = matcher;
    Ok(())
}

fn build_glob_matcher(base_dir: &Path, patterns: &[String]) -> Option<Gitignore> {
    if patterns.is_empty() {
        return None;
    }
    let mut builder = GitignoreBuilder::new(base_dir);
    builder.allow_unclosed_class(false);
    for pat in patterns {
        let normalized = pat.trim_end_matches(['\r']);
        let _ = builder.add_line(None, normalized);
    }
    builder.build().ok()
}

fn build_ignore_matcher(
    patterns: &[String],
    from_files: &[String],
    envx: &dyn Env,
    base_dir: &Path,
) -> Result<(Option<Gitignore>, Vec<String>), String> {
    if patterns.is_empty() && from_files.is_empty() {
        return Ok((None, Vec::new()));
    }

    let mut builder = GitignoreBuilder::new(base_dir);
    builder.allow_unclosed_class(false);
    let mut any_pattern = false;

    for pat in patterns {
        let normalized = pat.trim_end_matches(['\r']);
        if let Err(err) = builder.add_line(None, normalized) {
            return Err(format!(
                "invalid config: ignore pattern '{normalized}' is invalid: {err}"
            ));
        }
        any_pattern = true;
    }

    let mut extra_patterns = Vec::new();
    for source in from_files {
        let source_path = Path::new(source);
        let resolved = if source_path.is_absolute() {
            source_path.to_path_buf()
        } else {
            base_dir.join(source_path)
        };
        let data = match envx.read_to_string(&resolved) {
            Ok(text) => text,
            Err(err) => {
                return Err(format!(
                    "failed to read ignore-from-file {}: {err}",
                    resolved.display()
                ));
            }
        };
        for line in data.lines() {
            let normalized = line.trim_end_matches(['\r']);
            if normalized.trim().is_empty() {
                continue;
            }
            if let Err(err) = builder.add_line(Some(resolved.clone()), normalized) {
                return Err(format!(
                    "invalid config: ignore-from-file pattern in {} is invalid: {err}",
                    resolved.display()
                ));
            }
            extra_patterns.push(normalized.to_string());
            any_pattern = true;
        }
    }

    let matcher = any_pattern.then(|| {
        builder
            .build()
            .expect("ignore matcher build should not fail after validation")
    });
    Ok((matcher, extra_patterns))
}

fn build_per_file_ignores(
    per_file_ignores: &BTreeMap<String, Vec<String>>,
    base_dir: &Path,
) -> Result<Vec<PerFileIgnore>, String> {
    per_file_ignores
        .iter()
        .map(|(pattern, rules)| PerFileIgnore::new(pattern, rules.clone(), base_dir))
        .collect()
}

fn path_matches_ignore(matcher: &Gitignore, path: &Path, base_dir: &Path) -> bool {
    let rel = path.strip_prefix(base_dir).unwrap_or(path);
    let direct = matcher.matched(rel, false);
    if direct.is_whitelist() {
        return false;
    }
    if direct.is_ignore() {
        return true;
    }
    matcher.matched_path_or_any_parents(rel, false).is_ignore()
}

fn resolve_extend_path(
    entry: &str,
    envx: &dyn Env,
    base_dir: Option<&Path>,
) -> PathBuf {
    let candidate = PathBuf::from(entry);
    if candidate.is_absolute() {
        return candidate;
    }
    if let Some(joined) = base_dir
        .map(|base| base.join(&candidate))
        .filter(|candidate| envx.path_exists(candidate))
    {
        return joined;
    }
    let cwd = envx.current_dir();
    let fallback = cwd.join(&candidate);
    if envx.path_exists(&fallback) {
        fallback
    } else {
        candidate
    }
}

fn deep_merge_yaml_owned(dst: &mut YamlOwned, src: &YamlOwned) {
    if let (Some(_), Some(src_map)) = (dst.as_mapping(), src.as_mapping()) {
        for (k, v) in src_map {
            let key = k
                .as_str()
                .expect("config parsing should reject non-string mapping keys");
            let merged = dst.as_mapping_get_mut(key).is_some_and(|dv| {
                deep_merge_yaml_owned(dv, v);
                true
            });
            if !merged {
                let map = dst.as_mapping_mut().expect("checked mapping above");
                map.insert(
                    YamlOwned::Value(ScalarOwned::String(key.to_owned())),
                    v.clone(),
                );
            }
        }
    } else {
        *dst = src.clone();
    }
}

fn typed_fix_config(fix: &NormalizedFixConfig) -> FixConfig {
    let fixable = fix
        .fixable
        .iter()
        .copied()
        .map(typed_fix_selector)
        .collect();
    let unfixable = fix.unfixable.iter().copied().map(typed_fix_rule).collect();
    FixConfig { fixable, unfixable }
}

fn normalized_fix_config(fix: &FixConfig) -> Option<NormalizedFixConfig> {
    if fix.is_default() {
        return None;
    }

    Some(NormalizedFixConfig {
        fixable: fix
            .fixable
            .iter()
            .copied()
            .map(normalized_fix_selector)
            .collect(),
        unfixable: fix
            .unfixable
            .iter()
            .copied()
            .map(normalized_fix_rule)
            .collect(),
    })
}

fn normalized_fix_selector(selector: FixRuleSelector) -> TomlFixableRuleSelector {
    match selector {
        FixRuleSelector::All => TomlFixableRuleSelector::All,
        FixRuleSelector::Rule(FixRule::Braces) => TomlFixableRuleSelector::Braces,
        FixRuleSelector::Rule(FixRule::Brackets) => TomlFixableRuleSelector::Brackets,
        FixRuleSelector::Rule(FixRule::Commas) => TomlFixableRuleSelector::Commas,
        FixRuleSelector::Rule(FixRule::Comments) => TomlFixableRuleSelector::Comments,
        FixRuleSelector::Rule(FixRule::CommentsIndentation) => {
            TomlFixableRuleSelector::CommentsIndentation
        }
        FixRuleSelector::Rule(FixRule::NewLineAtEndOfFile) => {
            TomlFixableRuleSelector::NewLineAtEndOfFile
        }
        FixRuleSelector::Rule(FixRule::DocumentEnd) => {
            TomlFixableRuleSelector::DocumentEnd
        }
        FixRuleSelector::Rule(FixRule::DocumentStart) => {
            TomlFixableRuleSelector::DocumentStart
        }
        FixRuleSelector::Rule(FixRule::EmptyLines) => {
            TomlFixableRuleSelector::EmptyLines
        }
        FixRuleSelector::Rule(FixRule::NewLines) => TomlFixableRuleSelector::NewLines,
        FixRuleSelector::Rule(FixRule::QuotedStrings) => {
            TomlFixableRuleSelector::QuotedStrings
        }
        FixRuleSelector::Rule(FixRule::TrailingSpaces) => {
            TomlFixableRuleSelector::TrailingSpaces
        }
    }
}

fn normalized_fix_rule(rule: FixRule) -> TomlFixRuleName {
    match rule {
        FixRule::Braces => TomlFixRuleName::Braces,
        FixRule::Brackets => TomlFixRuleName::Brackets,
        FixRule::Commas => TomlFixRuleName::Commas,
        FixRule::Comments => TomlFixRuleName::Comments,
        FixRule::CommentsIndentation => TomlFixRuleName::CommentsIndentation,
        FixRule::DocumentEnd => TomlFixRuleName::DocumentEnd,
        FixRule::DocumentStart => TomlFixRuleName::DocumentStart,
        FixRule::EmptyLines => TomlFixRuleName::EmptyLines,
        FixRule::NewLineAtEndOfFile => TomlFixRuleName::NewLineAtEndOfFile,
        FixRule::NewLines => TomlFixRuleName::NewLines,
        FixRule::QuotedStrings => TomlFixRuleName::QuotedStrings,
        FixRule::TrailingSpaces => TomlFixRuleName::TrailingSpaces,
    }
}

fn normalized_config_from_runtime(config: &YamlLintConfig) -> NormalizedConfig {
    NormalizedConfig {
        ignore_patterns: (!config.ignore_patterns.is_empty())
            .then(|| config.ignore_patterns.clone()),
        ignore_from_files: (!config.ignore_from_files.is_empty())
            .then(|| config.ignore_from_files.clone()),
        per_file_ignores: config.per_file_ignores.clone(),
        per_line_ignores: config.per_line_ignores.clone(),
        yaml_file_patterns: Some(config.yaml_file_patterns.clone()),
        markdown_file_patterns: (!config.markdown_file_patterns.is_empty())
            .then(|| config.markdown_file_patterns.clone()),
        markdown: (!config.markdown_file_patterns.is_empty()).then_some(
            NormalizedMarkdown {
                front_matter: Some(config.lint_markdown_front_matter),
                fenced_blocks: Some(config.lint_markdown_fenced_blocks),
            },
        ),
        output: config.output.clone(),
        locale: config.locale.clone(),
        fix: normalized_fix_config(&config.fix),
        rules: config
            .rules
            .iter()
            .map(|(name, rule)| (name.clone(), rule.value.clone()))
            .collect(),
    }
}

fn typed_fix_selector(selector: TomlFixableRuleSelector) -> FixRuleSelector {
    match selector {
        TomlFixableRuleSelector::All => FixRuleSelector::All,
        TomlFixableRuleSelector::Braces => FixRuleSelector::Rule(FixRule::Braces),
        TomlFixableRuleSelector::Brackets => FixRuleSelector::Rule(FixRule::Brackets),
        TomlFixableRuleSelector::Commas => FixRuleSelector::Rule(FixRule::Commas),
        TomlFixableRuleSelector::Comments => FixRuleSelector::Rule(FixRule::Comments),
        TomlFixableRuleSelector::CommentsIndentation => {
            FixRuleSelector::Rule(FixRule::CommentsIndentation)
        }
        TomlFixableRuleSelector::NewLineAtEndOfFile => {
            FixRuleSelector::Rule(FixRule::NewLineAtEndOfFile)
        }
        TomlFixableRuleSelector::DocumentEnd => {
            FixRuleSelector::Rule(FixRule::DocumentEnd)
        }
        TomlFixableRuleSelector::DocumentStart => {
            FixRuleSelector::Rule(FixRule::DocumentStart)
        }
        TomlFixableRuleSelector::EmptyLines => {
            FixRuleSelector::Rule(FixRule::EmptyLines)
        }
        TomlFixableRuleSelector::NewLines => FixRuleSelector::Rule(FixRule::NewLines),
        TomlFixableRuleSelector::QuotedStrings => {
            FixRuleSelector::Rule(FixRule::QuotedStrings)
        }
        TomlFixableRuleSelector::TrailingSpaces => {
            FixRuleSelector::Rule(FixRule::TrailingSpaces)
        }
    }
}

fn typed_fix_rule(rule: TomlFixRuleName) -> FixRule {
    match rule {
        TomlFixRuleName::Braces => FixRule::Braces,
        TomlFixRuleName::Brackets => FixRule::Brackets,
        TomlFixRuleName::Commas => FixRule::Commas,
        TomlFixRuleName::Comments => FixRule::Comments,
        TomlFixRuleName::CommentsIndentation => FixRule::CommentsIndentation,
        TomlFixRuleName::DocumentEnd => FixRule::DocumentEnd,
        TomlFixRuleName::DocumentStart => FixRule::DocumentStart,
        TomlFixRuleName::EmptyLines => FixRule::EmptyLines,
        TomlFixRuleName::NewLineAtEndOfFile => FixRule::NewLineAtEndOfFile,
        TomlFixRuleName::NewLines => FixRule::NewLines,
        TomlFixRuleName::QuotedStrings => FixRule::QuotedStrings,
        TomlFixRuleName::TrailingSpaces => FixRule::TrailingSpaces,
    }
}

/// Result of configuration discovery.
#[derive(Debug, Clone)]
pub struct ConfigContext {
    pub config: YamlLintConfig,
    pub base_dir: PathBuf,
    pub source: Option<PathBuf>,
    pub notices: Vec<String>,
    /// Whether an actual configuration source was used (inline data, a config file,
    /// a discovered project/user-global config, or an env-var config). `false` only
    /// when nothing was found and resolution fell back to an empty config, which lets
    /// the lint CLI distinguish "no configuration found" from "a configuration that
    /// enables no rules".
    pub config_found: bool,
}

fn finalize_context(
    envx: &dyn Env,
    mut cfg: YamlLintConfig,
    base_dir: impl Into<PathBuf>,
    source: Option<PathBuf>,
    notices: Vec<String>,
    config_found: bool,
) -> Result<ConfigContext, String> {
    let base_dir = base_dir.into();
    cfg.finalize(envx, &base_dir)?;
    Ok(ConfigContext {
        config: cfg,
        base_dir,
        source,
        notices,
        config_found,
    })
}

/// Discover configuration with precedence:
/// config-data > config-file > project (TOML-first, YAML fallback) > env var >
/// ryl user-global (TOML) > yamllint user-global (YAML) > defaults.
///
/// # Errors
/// Returns an error when a config file cannot be read or parsed.
pub fn discover_config(
    inputs: &[PathBuf],
    overrides: &Overrides,
) -> Result<ConfigContext, String> {
    discover_config_with(inputs, overrides, &SystemEnv)
}

/// Discover configuration using a provided `Env` implementation.
///
/// # Errors
/// Returns an error when a configuration file cannot be read or parsed.
///
/// # Panics
/// Panics only if a built-in preset referenced via `extends:` cannot be parsed,
/// which indicates a programming error.
pub fn discover_config_with(
    inputs: &[PathBuf],
    overrides: &Overrides,
    envx: &dyn Env,
) -> Result<ConfigContext, String> {
    // Global config resolution: inline > file > project > env var > user-global.
    if let Some(ref data) = overrides.config_data {
        let base_dir = envx.current_dir();
        let cfg =
            YamlLintConfig::from_yaml_str_with_env(data, Some(envx), Some(&base_dir))?;
        return finalize_context(envx, cfg, base_dir, None, Vec::new(), true);
    }
    if let Some(ref file) = overrides.config_file {
        return ctx_from_config_path_core(envx, file, false, Vec::new());
    }
    let discovered = find_project_config_core(envx, inputs)?;
    if let Some(discovered) = discovered {
        return ctx_from_config_path_core(
            envx,
            &discovered.cfg_path,
            true,
            discovered.notices,
        );
    }
    if let Some(ctx) = try_env_config_core(envx)? {
        return Ok(ctx);
    }
    let cwd = envx.current_dir();
    try_user_global_core(envx, &cwd)?.map_or_else(
        move || {
            finalize_context(
                envx,
                YamlLintConfig::default(),
                cwd,
                None,
                Vec::new(),
                false,
            )
        },
        Ok,
    )
}

/// Variant of `discover_config` with injectable environment access to keep tests safe.
///
/// # Errors
/// Returns an error when a config file cannot be read or parsed.
///
/// # Panics
/// Panics only if a built-in preset referenced via `extends:` cannot be parsed
/// (a programming error).
pub fn discover_config_with_env(
    inputs: &[PathBuf],
    overrides: &Overrides,
    env_get: &dyn Fn(&str) -> Option<String>,
) -> Result<ConfigContext, String> {
    discover_config_with(inputs, overrides, &ClosureEnv { get: env_get })
}

/// Discover the config for a single file path, ignoring env/global overrides.
/// Precedence: nearest project config up-tree (TOML-first, YAML fallback),
/// then user-global, then an empty config (no rules enabled).
///
/// # Errors
/// Returns an error when a config file cannot be read or parsed.
/// Discover the effective config for a single file.
///
/// # Errors
/// Returns an error when a config file cannot be read or parsed.
///
/// # Panics
/// Panics only if a built-in preset referenced via `extends:` cannot be parsed
/// (a programming error).
pub fn discover_per_file(path: &Path) -> Result<ConfigContext, String> {
    discover_per_file_with(path, &SystemEnv)
}

/// Discover the effective config for a single file using a provided `Env`.
///
/// # Errors
/// Returns an error when a configuration file cannot be read or parsed.
///
/// # Panics
/// Panics only if a built-in preset referenced via `extends:` cannot be parsed
/// (a programming error).
pub fn discover_per_file_with(
    path: &Path,
    envx: &dyn Env,
) -> Result<ConfigContext, String> {
    let start_dir = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };

    let discovered = find_project_config_core(envx, &[start_dir.to_path_buf()])?;
    if let Some(discovered) = discovered {
        return ctx_from_config_path_core(
            envx,
            &discovered.cfg_path,
            true,
            discovered.notices,
        );
    }
    try_user_global_core(envx, start_dir)?.map_or_else(
        || {
            finalize_context(
                envx,
                YamlLintConfig::default(),
                envx.current_dir(),
                None,
                Vec::new(),
                false,
            )
        },
        Ok,
    )
}

// Testable core helpers below.
fn ctx_from_config_path_core(
    envx: &dyn Env,
    p: &Path,
    allow_missing_pyproject: bool,
    notices: Vec<String>,
) -> Result<ConfigContext, String> {
    let base = p
        .parent()
        .map_or_else(|| envx.current_dir(), Path::to_path_buf);
    let cfg = load_config_from_path_core(envx, p, &base, allow_missing_pyproject)?
        .expect("missing [tool.ryl] should be filtered or returned as an error before this point");
    finalize_context(envx, cfg, base, Some(p.to_path_buf()), notices, true)
}

fn expand_user_path(envx: &dyn Env, raw: &str) -> PathBuf {
    if let Some(rest) = raw.strip_prefix('~') {
        let trimmed = rest.trim_start_matches(['/', '\\']);
        return envx
            .home_dir()
            .map_or_else(|| PathBuf::from(raw), |home| home.join(trimmed));
    }
    PathBuf::from(raw)
}

fn try_env_config_core(envx: &dyn Env) -> Result<Option<ConfigContext>, String> {
    envx.env_var("YAMLLINT_CONFIG_FILE")
        .map(|raw| expand_user_path(envx, &raw))
        .filter(|p| envx.path_exists(p))
        .map(|p| ctx_from_config_path_core(envx, &p, false, Vec::new()))
        .transpose()
}

// no separate try_env_config_with; discover_config_with_env uses ClosureEnv + discover_config_with

/// User-global config fallback. ryl checks its own branded location first, then the
/// yamllint-compatible path so migrators keep working.
fn try_user_global_core(
    envx: &dyn Env,
    base_dir: &Path,
) -> Result<Option<ConfigContext>, String> {
    if let Some(ctx) = try_ryl_user_global_core(envx, base_dir)? {
        return Ok(Some(ctx));
    }
    try_yamllint_user_global_core(envx, base_dir)
}

/// Directory holding ryl's own user-global config, following the ruff/Biome convention
/// via `config_dir` (`$XDG_CONFIG_HOME` else the platform-native dir), so on macOS it
/// resolves under `~/Library/Application Support/ryl`, unlike the yamllint path.
fn ryl_user_global_dir(envx: &dyn Env) -> Option<PathBuf> {
    envx.config_dir().map(|base| base.join("ryl"))
}

/// yamllint's user-global config path, matching yamllint exactly across platforms:
/// `$XDG_CONFIG_HOME/yamllint/config` if set, else `~/.config/yamllint/config` —
/// deliberately NOT the platform-native config dir (verified against yamllint/cli.py).
fn yamllint_user_global_path(envx: &dyn Env) -> Option<PathBuf> {
    envx.env_var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| envx.home_dir().map(|home| home.join(".config")))
        .map(|base| base.join("yamllint").join("config"))
}

/// Resolve the `(yamllint source, ryl target)` paths for migrating a yamllint user-global
/// config to ryl's own location. `None` when no config directory or home can be
/// determined. The target is `ryl.toml` (non-hidden, in the dedicated `ryl/` dir).
#[must_use]
pub fn user_config_migration_paths(envx: &dyn Env) -> Option<(PathBuf, PathBuf)> {
    let source = yamllint_user_global_path(envx)?;
    let target = ryl_user_global_dir(envx)?.join("ryl.toml");
    Some((source, target))
}

/// ryl-native user-global config: `<config-dir>/ryl/{.ryl.toml,ryl.toml}`, TOML only per
/// the YAML-mirrors-yamllint / TOML-for-ryl-only split.
fn try_ryl_user_global_core(
    envx: &dyn Env,
    base_dir: &Path,
) -> Result<Option<ConfigContext>, String> {
    let Some(dir) = ryl_user_global_dir(envx) else {
        return Ok(None);
    };
    for name in RYL_USER_GLOBAL_CONFIG_CANDIDATES {
        let candidate = dir.join(name);
        if !envx.path_exists(&candidate) {
            continue;
        }
        let cfg = load_config_from_path_core(envx, &candidate, base_dir, false)?
            .expect(
                "non-pyproject .toml always yields a config (empty errors earlier)",
            );
        return finalize_context(
            envx,
            cfg,
            base_dir.to_path_buf(),
            Some(candidate),
            Vec::new(),
            true,
        )
        .map(Some);
    }
    Ok(None)
}

/// yamllint-compatible user-global config (`<base>/yamllint/config`, YAML), kept for
/// users migrating from yamllint.
fn try_yamllint_user_global_core(
    envx: &dyn Env,
    base_dir: &Path,
) -> Result<Option<ConfigContext>, String> {
    yamllint_user_global_path(envx)
        .filter(|p| envx.path_exists(p))
        .map(|p| {
            let data = envx.read_to_string(&p)?;
            let cfg = YamlLintConfig::from_yaml_str_with_env(
                &data,
                Some(envx),
                Some(base_dir),
            )?;
            finalize_context(
                envx,
                cfg,
                base_dir.to_path_buf(),
                Some(p),
                Vec::new(),
                true,
            )
        })
        .transpose()
}

const TOML_PROJECT_CONFIG_CANDIDATES: [&str; 3] =
    [".ryl.toml", "ryl.toml", "pyproject.toml"];
const YAML_PROJECT_CONFIG_CANDIDATES: [&str; 3] =
    [".yamllint", ".yamllint.yaml", ".yamllint.yml"];
// ryl-native user-global candidates (TOML only); checked inside `<config-dir>/ryl/`.
const RYL_USER_GLOBAL_CONFIG_CANDIDATES: [&str; 2] = [".ryl.toml", "ryl.toml"];

#[derive(Debug, Clone)]
struct ProjectConfigDiscovery {
    cfg_path: PathBuf,
    notices: Vec<String>,
}

fn load_config_from_path_core(
    envx: &dyn Env,
    path: &Path,
    base_dir: &Path,
    allow_missing_pyproject: bool,
) -> Result<Option<YamlLintConfig>, String> {
    let data = envx.read_to_string(path)?;
    if path
        .file_name()
        .is_some_and(|name| name == "pyproject.toml")
    {
        let cfg = YamlLintConfig::from_toml_str_with_env(
            &data,
            Some(envx),
            Some(base_dir),
            true,
        )?;
        if cfg.is_none() && !allow_missing_pyproject {
            return Err(format!(
                "failed to parse config file {}: missing [tool.ryl] section",
                path.display()
            ));
        }
        return Ok(cfg);
    }
    if is_toml_path(path) {
        let cfg = YamlLintConfig::from_toml_str_with_env(
            &data,
            Some(envx),
            Some(base_dir),
            false,
        )?;
        return Ok(cfg);
    }
    let cfg =
        YamlLintConfig::from_yaml_str_with_env(&data, Some(envx), Some(base_dir))?;
    Ok(Some(cfg))
}

fn is_toml_path(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "toml")
}

fn build_project_search_starts(envx: &dyn Env, inputs: &[PathBuf]) -> Vec<PathBuf> {
    let cwd = envx.current_dir();
    let mut starts = Vec::new();
    if inputs.is_empty() {
        starts.push(cwd.clone());
        return starts;
    }
    for path in inputs {
        let start = if path.is_dir() {
            path.clone()
        } else {
            path.parent().map_or_else(|| cwd.clone(), Path::to_path_buf)
        };
        let abs = if start.is_absolute() {
            start
        } else {
            cwd.join(start)
        };
        if !starts.iter().any(|existing| existing == &abs) {
            starts.push(abs);
        }
    }
    starts
}

fn find_first_yaml_candidate(
    envx: &dyn Env,
    start: &Path,
    home_abs: Option<&PathBuf>,
) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        for name in YAML_PROJECT_CONFIG_CANDIDATES {
            let candidate = dir.join(name);
            if envx.path_exists(&candidate) {
                return Some(candidate);
            }
        }
        if home_abs.is_some_and(|home| home == &dir) {
            break;
        }
        match dir.parent() {
            Some(parent) if parent != dir => dir = parent.to_path_buf(),
            _ => break,
        }
    }
    None
}

fn find_project_config_core(
    envx: &dyn Env,
    inputs: &[PathBuf],
) -> Result<Option<ProjectConfigDiscovery>, String> {
    let starts = build_project_search_starts(envx, inputs);
    let cwd = envx.current_dir();
    let home_abs = envx
        .env_var("HOME")
        .map(PathBuf::from)
        .or_else(dirs_next::home_dir)
        .map(|home| {
            if home.is_absolute() {
                home
            } else {
                cwd.join(home)
            }
        });

    for start in &starts {
        let mut dir = start.clone();
        loop {
            for name in TOML_PROJECT_CONFIG_CANDIDATES {
                let candidate = dir.join(name);
                if !envx.path_exists(&candidate) {
                    continue;
                }
                if name == "pyproject.toml" {
                    let loaded =
                        load_config_from_path_core(envx, &candidate, &dir, true)?;
                    if loaded.is_none() {
                        continue;
                    }
                }
                let notices = find_first_yaml_candidate(envx, start, home_abs.as_ref())
                    .map(|yaml_path| {
                        format!(
                            "warning: ignoring legacy YAML config discovery because TOML config {} was found (legacy candidate: {})",
                            candidate.display(),
                            yaml_path.display()
                        )
                    })
                    .into_iter()
                    .collect();
                return Ok(Some(ProjectConfigDiscovery {
                    cfg_path: candidate,
                    notices,
                }));
            }
            if home_abs.as_ref().is_some_and(|home| home == &dir) {
                break;
            }
            match dir.parent() {
                Some(parent) if parent != dir => dir = parent.to_path_buf(),
                _ => break,
            }
        }
    }

    for start in starts {
        if let Some(candidate) =
            find_first_yaml_candidate(envx, &start, home_abs.as_ref())
        {
            return Ok(Some(ProjectConfigDiscovery {
                cfg_path: candidate,
                notices: Vec::new(),
            }));
        }
    }

    Ok(None)
}
