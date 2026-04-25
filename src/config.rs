use std::borrow::Cow;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use regex::Regex;
use saphyr::{LoadableYamlNode, MappingOwned, ScalarOwned, YamlOwned};
use toml::Value as TomlValue;

use crate::config_schema::{
    FixRuleName as TomlFixRuleName, FixableRuleSelector as TomlFixableRuleSelector,
    ForbidSetting, IndentSequencesSetting, NewLinesType, NormalizedFixConfig,
    QuoteType, QuotedStringsRequiredModeForValidation, SpacesSetting, TomlConfig,
    TruthyAllowedValue, normalize_toml_config, parse_toml_config_str,
    quoted_strings_required_mode_from_yaml_value, validate_key_ordering_patterns,
    validate_quoted_strings_semantics, validate_toml_config, yaml_owned_to_toml_value,
    yaml_value_matches_toml_type,
};
use crate::{conf, decoder};

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
        SystemEnv.config_dir()
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
    rule_names: Vec<String>,
    rules: std::collections::BTreeMap<String, YamlOwned>,
    rule_filters: std::collections::BTreeMap<String, RuleFilter>,
    yaml_file_patterns: Vec<String>,
    yaml_matcher: Option<Gitignore>,
    locale: Option<String>,
    fix: FixConfig,
}

const DEFAULT_YAML_FILE_PATTERNS: [&str; 3] = ["*.yaml", "*.yml", ".yamllint"];

const TRUTHY_ALLOWED_VALUES_DISPLAY: &str = "['YES', 'Yes', 'yes', 'NO', 'No', 'no', 'TRUE', 'True', 'true', 'FALSE', 'False', 'false', 'ON', 'On', 'on', 'OFF', 'Off', 'off']";

#[derive(Debug, Clone, Default)]
struct RuleFilter {
    patterns: Vec<String>,
    from_files: Vec<String>,
    matcher: Option<Gitignore>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleLevel {
    Error,
    Warning,
}

impl RuleLevel {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "error" => Some(Self::Error),
            "warning" => Some(Self::Warning),
            _ => None,
        }
    }
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
    NewLineAtEndOfFile,
    NewLines,
}

impl FixRule {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Braces => "braces",
            Self::Brackets => "brackets",
            Self::Commas => "commas",
            Self::Comments => "comments",
            Self::CommentsIndentation => "comments-indentation",
            Self::NewLineAtEndOfFile => "new-line-at-end-of-file",
            Self::NewLines => "new-lines",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "braces" => Some(Self::Braces),
            "brackets" => Some(Self::Brackets),
            "commas" => Some(Self::Commas),
            "comments" => Some(Self::Comments),
            "comments-indentation" => Some(Self::CommentsIndentation),
            "new-line-at-end-of-file" => Some(Self::NewLineAtEndOfFile),
            "new-lines" => Some(Self::NewLines),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixRuleSelector {
    All,
    Rule(FixRule),
}

impl FixRuleSelector {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::All => "ALL",
            Self::Rule(rule) => rule.as_str(),
        }
    }
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
            rule_names: Vec::new(),
            rules: std::collections::BTreeMap::new(),
            rule_filters: std::collections::BTreeMap::new(),
            yaml_file_patterns: DEFAULT_YAML_FILE_PATTERNS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            yaml_matcher: None,
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

    fn apply_extends(
        &mut self,
        node: &YamlOwned,
        envx: Option<&dyn Env>,
        base_dir: Option<&Path>,
    ) -> Result<(), String> {
        let base_path = base_dir.unwrap_or_else(|| Path::new(""));

        match node {
            YamlOwned::Value(value) => {
                if let Some(ext) = value.as_str() {
                    self.extend_from_entry(ext, envx, base_path)?;
                }
            }
            YamlOwned::Sequence(seq) => {
                for item in seq {
                    if let Some(ext) = item.as_str() {
                        self.extend_from_entry(ext, envx, base_path)?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn extend_from_entry(
        &mut self,
        entry: &str,
        envx: Option<&dyn Env>,
        base_dir: &Path,
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
        let base = Self::from_yaml_str_with_env(&data, Some(envx), Some(&parent_dir))?;
        self.merge_from(base);
        Ok(())
    }
    #[must_use]
    pub fn ignore_patterns(&self) -> &[String] {
        &self.ignore_patterns
    }

    #[must_use]
    pub fn rule_names(&self) -> &[String] {
        &self.rule_names
    }

    #[must_use]
    pub fn rule_level(&self, rule: &str) -> Option<RuleLevel> {
        let value = self.rules.get(rule)?;
        determine_rule_level(value)
    }

    #[must_use]
    pub fn rule_option_str(&self, rule: &str, option: &str) -> Option<&str> {
        self.rule_option(rule, option).and_then(YamlOwned::as_str)
    }

    #[must_use]
    pub fn rule_option(&self, rule: &str, option: &str) -> Option<&YamlOwned> {
        self.rules.get(rule)?.as_mapping_get(option)
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

    fn build_yaml_matcher(&mut self, base_dir: &Path) {
        if self.yaml_file_patterns.is_empty() {
            self.yaml_matcher = None;
            return;
        }

        let mut builder = GitignoreBuilder::new(base_dir);
        builder.allow_unclosed_class(false);
        for pat in &self.yaml_file_patterns {
            let normalized = pat.trim_end_matches(['\r']);
            let _ = builder.add_line(None, normalized);
        }

        self.yaml_matcher = builder.build().ok();
    }

    fn refresh_rule_filter(&mut self, rule: &str) {
        let node = self
            .rules
            .get(rule)
            .expect("refresh_rule_filter should only be called for existing rules");

        if node.as_mapping().is_none() {
            self.rule_filters.remove(rule);
            return;
        }

        let patterns = node
            .as_mapping_get("ignore")
            .map(|n| {
                load_ignore_patterns(n)
                    .expect("ignore patterns validated during parsing")
            })
            .unwrap_or_default();
        let from_files = node
            .as_mapping_get("ignore-from-file")
            .map(|n| {
                load_ignore_from_files(n)
                    .expect("ignore-from-file entries validated during parsing")
            })
            .unwrap_or_default();

        let filter = self.rule_filters.entry(rule.to_owned()).or_default();
        filter.patterns = patterns;
        filter.from_files = from_files;
        filter.matcher = None;
    }

    /// Returns true when `path` should be ignored according to config patterns.
    /// Matching is performed on the path relative to `base_dir`.
    #[must_use]
    pub fn is_file_ignored(&self, path: &Path, base_dir: &Path) -> bool {
        let Some(matcher) = &self.ignore_matcher else {
            return false;
        };
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

    #[must_use]
    pub fn is_rule_ignored(&self, rule: &str, path: &Path, base_dir: &Path) -> bool {
        let Some(filter) = self.rule_filters.get(rule) else {
            return false;
        };
        let Some(matcher) = &filter.matcher else {
            return false;
        };
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

    fn from_yaml_str_with_env(
        s: &str,
        envx: Option<&dyn Env>,
        base_dir: Option<&Path>,
    ) -> Result<Self, String> {
        let docs = YamlOwned::load_from_str(s)
            .map_err(|e| format!("failed to parse config data: {e}"))?;
        Self::from_doc_with_env(&docs[0], envx, base_dir)
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

        if let Some(ignore) = normalized.ignore_patterns {
            cfg.ignore_patterns.clear();
            cfg.ignore_from_files.clear();
            cfg.ignore_patterns = ignore;
        }

        if let Some(ignore_from_file) = normalized.ignore_from_files {
            cfg.ignore_patterns.clear();
            cfg.ignore_from_files = ignore_from_file;
        }

        if let Some(yaml_files) = normalized.yaml_file_patterns {
            cfg.yaml_file_patterns.clear();
            cfg.yaml_file_patterns = yaml_files;
        }

        cfg.locale = normalized.locale;

        if let Some(fix) = normalized.fix.as_ref() {
            cfg.fix = typed_fix_config(fix);
        }

        for (name, value) in &normalized.rules {
            cfg.merge_rule(name, value);
        }

        cfg
    }

    fn from_doc_with_env(
        doc: &YamlOwned,
        envx: Option<&dyn Env>,
        base_dir: Option<&Path>,
    ) -> Result<Self, String> {
        let mut cfg = Self::default();

        if doc.as_mapping().is_none() {
            return Err("invalid config: not a mapping".to_string());
        }

        if let Some(extends) = doc.as_mapping_get("extends") {
            cfg.apply_extends(extends, envx, base_dir)?;
        }

        let ignore = doc.as_mapping_get("ignore");
        let ignore_from_file = doc.as_mapping_get("ignore-from-file");
        if ignore.is_some() && ignore_from_file.is_some() {
            return Err(
                "invalid config: ignore and ignore-from-file keys cannot be used together"
                    .to_string(),
            );
        }

        if let Some(node) = ignore {
            cfg.ignore_patterns.clear();
            cfg.ignore_from_files.clear();
            let mut patterns = load_ignore_patterns(node)?;
            cfg.ignore_patterns.append(&mut patterns);
        }

        if let Some(node) = ignore_from_file {
            cfg.ignore_patterns.clear();
            cfg.ignore_from_files = load_ignore_from_files(node)?;
        }

        let yaml_files = doc.as_mapping_get("yaml-files");
        if let Some(yf) = yaml_files
            && let Some(seq) = yf.as_sequence()
        {
            cfg.yaml_file_patterns.clear();
            for it in seq {
                let Some(s) = it.as_str() else {
                    return Err(
                        "invalid config: yaml-files should be a list of file patterns"
                            .to_string(),
                    );
                };
                cfg.yaml_file_patterns.push(s.to_owned());
            }
        } else if yaml_files.is_some() {
            return Err(
                "invalid config: yaml-files should be a list of file patterns"
                    .to_string(),
            );
        }

        if let Some(locale) = doc.as_mapping_get("locale") {
            let Some(loc) = locale.as_str() else {
                return Err("invalid config: locale should be a string".to_string());
            };
            cfg.locale = Some(loc.to_owned());
        }

        if let Some(fix) = doc.as_mapping_get("fix") {
            let _ = fix;
            return Err(
                "invalid config: fix is only supported in TOML configuration"
                    .to_string(),
            );
        }

        if let Some(rules) = doc.as_mapping_get("rules")
            && let Some(map) = rules.as_mapping()
        {
            for (k, v) in map {
                let Some(name) = k.as_str() else {
                    continue;
                };
                validate_rule_value(name, v)?;
                cfg.merge_rule(name, v);
            }
        }

        Ok(cfg)
    }

    fn merge_from(&mut self, mut other: Self) {
        // Merge ignore patterns (append, then dedup later during matcher build)
        self.ignore_patterns.append(&mut other.ignore_patterns);
        self.ignore_from_files.append(&mut other.ignore_from_files);
        // Merge rules deeply and accumulate names
        for (name, val) in other.rules {
            self.merge_rule(&name, &val);
        }
        if !other.yaml_file_patterns.is_empty() {
            self.yaml_file_patterns = other.yaml_file_patterns;
        }
        if self.locale.is_none() {
            self.locale = other.locale;
        }
    }

    /// Render the effective configuration as TOML.
    ///
    /// # Errors
    /// Returns an error if a value cannot be represented in TOML.
    ///
    /// # Panics
    /// Panics if serializing a valid TOML value fails unexpectedly.
    pub fn to_toml_string(&self) -> Result<String, String> {
        let mut root = toml::map::Map::new();

        root.insert(
            "yaml-files".to_string(),
            TomlValue::Array(
                self.yaml_file_patterns
                    .iter()
                    .map(|item| TomlValue::String(item.clone()))
                    .collect(),
            ),
        );

        if !self.ignore_from_files.is_empty() {
            root.insert(
                "ignore-from-file".to_string(),
                TomlValue::Array(
                    self.ignore_from_files
                        .iter()
                        .map(|item| TomlValue::String(item.clone()))
                        .collect(),
                ),
            );
        } else if !self.ignore_patterns.is_empty() {
            root.insert(
                "ignore".to_string(),
                TomlValue::Array(
                    self.ignore_patterns
                        .iter()
                        .map(|item| TomlValue::String(item.clone()))
                        .collect(),
                ),
            );
        }

        if let Some(locale) = &self.locale {
            root.insert("locale".to_string(), TomlValue::String(locale.clone()));
        }

        if !self.fix.is_default() {
            let mut fix = toml::map::Map::new();
            fix.insert(
                "fixable".to_string(),
                TomlValue::Array(
                    self.fix
                        .fixable
                        .iter()
                        .map(|item| TomlValue::String(item.as_str().to_string()))
                        .collect(),
                ),
            );
            fix.insert(
                "unfixable".to_string(),
                TomlValue::Array(
                    self.fix
                        .unfixable
                        .iter()
                        .map(|item| TomlValue::String(item.as_str().to_string()))
                        .collect(),
                ),
            );
            root.insert("fix".to_string(), TomlValue::Table(fix));
        }

        let mut rules: BTreeMap<String, TomlValue> = BTreeMap::new();
        for (name, value) in &self.rules {
            let converted = yaml_owned_to_toml_value(value)?;
            rules.insert(name.clone(), converted);
        }
        if !rules.is_empty() {
            root.insert(
                "rules".to_string(),
                TomlValue::Table(toml::map::Map::from_iter(rules)),
            );
        }

        Ok(toml::to_string_pretty(&TomlValue::Table(root))
            .expect("serializing TOML Value should not fail"))
    }

    fn finalize(&mut self, envx: &dyn Env, base_dir: &Path) -> Result<(), String> {
        let mut builder = GitignoreBuilder::new(base_dir);
        builder.allow_unclosed_class(false);
        let mut any_pattern = false;

        for pat in &self.ignore_patterns {
            let normalized = pat.trim_end_matches(['\r']);
            if let Err(err) = builder.add_line(None, normalized) {
                return Err(format!(
                    "invalid config: ignore pattern '{normalized}' is invalid: {err}"
                ));
            }
            any_pattern = true;
        }

        let mut extra_patterns: Vec<String> = Vec::new();
        for source in &self.ignore_from_files {
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

        if !extra_patterns.is_empty() {
            self.ignore_patterns.extend(extra_patterns);
        }

        self.ignore_matcher = if any_pattern {
            Some(
                builder
                    .build()
                    .expect("ignore matcher build should not fail after validation"),
            )
        } else {
            None
        };

        self.build_yaml_matcher(base_dir);

        for filter in self.rule_filters.values_mut() {
            build_rule_filter(filter, envx, base_dir)?;
        }
        Ok(())
    }

    fn merge_rule(&mut self, name: &str, value: &YamlOwned) {
        if let Some(dst) = self.rules.get_mut(name) {
            deep_merge_yaml_owned(dst, value);
        } else {
            self.rules.insert(name.to_owned(), value.clone());
        }
        self.refresh_rule_filter(name);
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
    if filter.patterns.is_empty() && filter.from_files.is_empty() {
        filter.matcher = None;
        return Ok(());
    }

    let mut builder = GitignoreBuilder::new(base_dir);
    builder.allow_unclosed_class(false);
    let mut any_pattern = false;

    for pat in &filter.patterns {
        let normalized = pat.trim_end_matches(['\r']);
        if let Err(err) = builder.add_line(None, normalized) {
            return Err(format!(
                "invalid config: ignore pattern '{normalized}' is invalid: {err}"
            ));
        }
        any_pattern = true;
    }

    let mut extra_patterns: Vec<String> = Vec::new();
    for source in &filter.from_files {
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

    if !extra_patterns.is_empty() {
        filter.patterns.extend(extra_patterns);
    }

    filter.matcher = if any_pattern {
        Some(
            builder
                .build()
                .expect("rule ignore matcher build should not fail after validation"),
        )
    } else {
        None
    };
    Ok(())
}

fn load_ignore_patterns(node: &YamlOwned) -> Result<Vec<String>, String> {
    parse_string_items(
        node,
        "invalid config: ignore should contain file patterns",
        patterns_from_scalar,
    )
}

fn load_ignore_from_files(node: &YamlOwned) -> Result<Vec<String>, String> {
    parse_string_items(
        node,
        "invalid config: ignore-from-file should contain filename(s), either as a list or string",
        |value| vec![value.to_owned()],
    )
}

fn patterns_from_scalar(value: &str) -> Vec<String> {
    value
        .lines()
        .map(|line| line.trim_end_matches(['\r']))
        .filter(|line| !line.trim().is_empty())
        .map(std::string::ToString::to_string)
        .collect()
}

fn parse_string_items(
    node: &YamlOwned,
    error: &str,
    map: impl Fn(&str) -> Vec<String>,
) -> Result<Vec<String>, String> {
    if let Some(seq) = node.as_sequence() {
        let mut values = Vec::with_capacity(seq.len());
        for item in seq {
            let Some(text) = item.as_str() else {
                return Err(error.to_string());
            };
            values.extend(map(text));
        }
        Ok(values)
    } else if let Some(text) = node.as_str() {
        Ok(map(text))
    } else {
        Err(error.to_string())
    }
}

fn determine_rule_level(node: &YamlOwned) -> Option<RuleLevel> {
    if let Some(s) = node.as_str() {
        return if s == "disable" {
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
                (key.as_str() == Some("level"))
                    .then(|| value.as_str().and_then(RuleLevel::parse))
            })
        })
        .flatten()
        .or(Some(RuleLevel::Error))
}

fn validate_rule_value(name: &str, value: &YamlOwned) -> Result<(), String> {
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
            validate_quoted_strings_rule(map)?;
            return Ok(());
        }

        for (key, val) in map {
            if handle_common_rule_key(name, key, val)? {
                continue;
            }

            match name {
                "anchors" => validate_anchors_option(key, val)?,
                "braces" => validate_brace_like_option("braces", key, val)?,
                "brackets" => validate_brace_like_option("brackets", key, val)?,
                "document-end" => validate_document_end_option(key, val)?,
                "document-start" => validate_document_start_option(key, val)?,
                "empty-lines" => validate_empty_lines_option(key, val)?,
                "commas" => validate_commas_option(key, val)?,
                "comments" => validate_comments_option(key, val)?,
                "new-lines" => validate_new_lines_option(key, val)?,
                "octal-values" => validate_octal_values_option(key, val)?,
                "float-values" => validate_float_values_option(key, val)?,
                "empty-values" => validate_empty_values_option(key, val)?,
                "key-duplicates" => validate_key_duplicates_option(key, val)?,
                "hyphens" => validate_hyphens_option(key, val)?,
                "truthy" => validate_truthy_option(key, val)?,
                "key-ordering" => validate_key_ordering_option(key, val)?,
                "indentation" => validate_indentation_option(key, val)?,
                "line-length" => validate_line_length_option(key, val)?,
                "trailing-spaces" => {
                    return Err(unknown_rule_option("trailing-spaces", key));
                }
                "comments-indentation" => {
                    return Err(unknown_rule_option("comments-indentation", key));
                }
                _ => {}
            }
        }
        return Ok(());
    }

    Err(format!(
        "invalid config: rule '{name}' should be 'enable', 'disable', or a mapping"
    ))
}

fn handle_common_rule_key(
    rule: &str,
    key: &YamlOwned,
    val: &YamlOwned,
) -> Result<bool, String> {
    if key.as_str() == Some("level") {
        let Some(level_text) = val.as_str() else {
            return Err(format!(
                "invalid config: rule '{rule}' level should be \"error\" or \"warning\""
            ));
        };
        if RuleLevel::parse(level_text).is_none() {
            return Err(format!(
                "invalid config: rule '{rule}' level should be \"error\" or \"warning\""
            ));
        }
        return Ok(true);
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

fn validate_document_end_option(
    key: &YamlOwned,
    val: &YamlOwned,
) -> Result<(), String> {
    validate_document_presence_option("document-end", key, val)
}

fn validate_document_start_option(
    key: &YamlOwned,
    val: &YamlOwned,
) -> Result<(), String> {
    validate_document_presence_option("document-start", key, val)
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
        "forbid" => {
            if yaml_value_matches_toml_type::<ForbidSetting>(val) {
                Ok(())
            } else {
                Err(format!(
                    "invalid config: option \"forbid\" of \"{rule}\" should be bool or \"non-empty\""
                ))
            }
        }
        "min-spaces-inside" => val.as_integer().map(|_| ()).ok_or_else(|| {
            format!("invalid config: option \"min-spaces-inside\" of \"{rule}\" should be int")
        }),
        "max-spaces-inside" => val.as_integer().map(|_| ()).ok_or_else(|| {
            format!("invalid config: option \"max-spaces-inside\" of \"{rule}\" should be int")
        }),
        "min-spaces-inside-empty" => val.as_integer().map(|_| ()).ok_or_else(|| {
            format!(
                "invalid config: option \"min-spaces-inside-empty\" of \"{rule}\" should be int"
            )
        }),
        "max-spaces-inside-empty" => val.as_integer().map(|_| ()).ok_or_else(|| {
            format!(
                "invalid config: option \"max-spaces-inside-empty\" of \"{rule}\" should be int"
            )
        }),
        _ => Err(unknown_rule_option(rule, key)),
    }
}

fn validate_anchors_option(key: &YamlOwned, val: &YamlOwned) -> Result<(), String> {
    let Some(name) = key.as_str() else {
        return Err(unknown_rule_option("anchors", key));
    };

    match name {
        "forbid-undeclared-aliases"
        | "forbid-duplicated-anchors"
        | "forbid-unused-anchors" => {
            if val.as_bool().is_some() {
                Ok(())
            } else {
                Err(format!(
                    "invalid config: option \"{name}\" of \"anchors\" should be bool"
                ))
            }
        }
        _ => Err(unknown_rule_option("anchors", key)),
    }
}

fn validate_hyphens_option(key: &YamlOwned, val: &YamlOwned) -> Result<(), String> {
    match key.as_str() {
        Some("max-spaces-after") => val.as_integer().map(|_| ()).ok_or_else(|| {
            "invalid config: option \"max-spaces-after\" of \"hyphens\" should be int".to_string()
        }),
        _ => Err(unknown_rule_option("hyphens", key)),
    }
}

fn validate_commas_option(key: &YamlOwned, val: &YamlOwned) -> Result<(), String> {
    let Some(name) = key.as_str() else {
        return Err(unknown_rule_option("commas", key));
    };

    match name {
        "max-spaces-before" => val.as_integer().map(|_| ()).ok_or_else(|| {
            "invalid config: option \"max-spaces-before\" of \"commas\" should be int".to_string()
        }),
        "min-spaces-after" => val.as_integer().map(|_| ()).ok_or_else(|| {
            "invalid config: option \"min-spaces-after\" of \"commas\" should be int".to_string()
        }),
        "max-spaces-after" => val.as_integer().map(|_| ()).ok_or_else(|| {
            "invalid config: option \"max-spaces-after\" of \"commas\" should be int".to_string()
        }),
        _ => Err(unknown_rule_option("commas", key)),
    }
}

fn validate_comments_option(key: &YamlOwned, val: &YamlOwned) -> Result<(), String> {
    let Some(name) = key.as_str() else {
        // Non-string keys are ignored during deep merge, matching yamllint.
        return Ok(());
    };

    match name {
        "require-starting-space" => validate_bool_option(val, "comments", "require-starting-space"),
        "ignore-shebangs" => validate_bool_option(val, "comments", "ignore-shebangs"),
        "min-spaces-from-content" => val.as_integer().map(|_| ()).ok_or_else(|| {
            "invalid config: option \"min-spaces-from-content\" of \"comments\" should be int"
                .to_string()
        }),
        _ => Err(unknown_rule_option("comments", key)),
    }
}

fn validate_empty_lines_option(key: &YamlOwned, val: &YamlOwned) -> Result<(), String> {
    match key.as_str() {
        Some("max") => val.as_integer().map(|_| ()).ok_or_else(|| {
            "invalid config: option \"max\" of \"empty-lines\" should be int"
                .to_string()
        }),
        Some("max-start") => val.as_integer().map(|_| ()).ok_or_else(|| {
            "invalid config: option \"max-start\" of \"empty-lines\" should be int"
                .to_string()
        }),
        Some("max-end") => val.as_integer().map(|_| ()).ok_or_else(|| {
            "invalid config: option \"max-end\" of \"empty-lines\" should be int"
                .to_string()
        }),
        _ => Err(unknown_rule_option("empty-lines", key)),
    }
}

fn validate_line_length_option(key: &YamlOwned, val: &YamlOwned) -> Result<(), String> {
    match key.as_str() {
        Some("max") => val.as_integer().map(|_| ()).ok_or_else(|| {
            "invalid config: option \"max\" of \"line-length\" should be int"
                .to_string()
        }),
        Some("allow-non-breakable-words") => {
            validate_bool_option(val, "line-length", "allow-non-breakable-words")
        }
        Some("allow-non-breakable-inline-mappings") => validate_bool_option(
            val,
            "line-length",
            "allow-non-breakable-inline-mappings",
        ),
        _ => Err(unknown_rule_option("line-length", key)),
    }
}

fn validate_new_lines_option(key: &YamlOwned, val: &YamlOwned) -> Result<(), String> {
    if key.as_str() != Some("type") {
        return Err(unknown_rule_option("new-lines", key));
    }

    if yaml_value_matches_toml_type::<NewLinesType>(val) {
        Ok(())
    } else {
        Err(
            "invalid config: option \"type\" of \"new-lines\" should be in ('unix', 'dos', 'platform')"
                .to_string(),
        )
    }
}

fn validate_octal_values_option(
    key: &YamlOwned,
    val: &YamlOwned,
) -> Result<(), String> {
    match key.as_str() {
        Some("forbid-implicit-octal") => {
            validate_bool_option(val, "octal-values", "forbid-implicit-octal")
        }
        Some("forbid-explicit-octal") => {
            validate_bool_option(val, "octal-values", "forbid-explicit-octal")
        }
        _ => Err(unknown_rule_option("octal-values", key)),
    }
}

fn validate_empty_values_option(
    key: &YamlOwned,
    val: &YamlOwned,
) -> Result<(), String> {
    match key.as_str() {
        Some("forbid-in-block-mappings") => {
            validate_bool_option(val, "empty-values", "forbid-in-block-mappings")
        }
        Some("forbid-in-flow-mappings") => {
            validate_bool_option(val, "empty-values", "forbid-in-flow-mappings")
        }
        Some("forbid-in-block-sequences") => {
            validate_bool_option(val, "empty-values", "forbid-in-block-sequences")
        }
        _ => Err(unknown_rule_option("empty-values", key)),
    }
}

fn validate_float_values_option(
    key: &YamlOwned,
    val: &YamlOwned,
) -> Result<(), String> {
    match key.as_str() {
        Some("require-numeral-before-decimal") => {
            validate_bool_option(val, "float-values", "require-numeral-before-decimal")
        }
        Some("forbid-scientific-notation") => {
            validate_bool_option(val, "float-values", "forbid-scientific-notation")
        }
        Some("forbid-nan") => validate_bool_option(val, "float-values", "forbid-nan"),
        Some("forbid-inf") => validate_bool_option(val, "float-values", "forbid-inf"),
        _ => Err(unknown_rule_option("float-values", key)),
    }
}

fn validate_key_duplicates_option(
    key: &YamlOwned,
    val: &YamlOwned,
) -> Result<(), String> {
    match key.as_str() {
        Some("forbid-duplicated-merge-keys") => {
            validate_bool_option(val, "key-duplicates", "forbid-duplicated-merge-keys")
        }
        _ => Err(unknown_rule_option("key-duplicates", key)),
    }
}

fn validate_truthy_option(key: &YamlOwned, val: &YamlOwned) -> Result<(), String> {
    match key.as_str() {
        Some("allowed-values") => {
            if yaml_value_matches_toml_type::<Vec<TruthyAllowedValue>>(val) {
                Ok(())
            } else {
                Err(format!(
                    "invalid config: option \"allowed-values\" of \"truthy\" should only contain values in {TRUTHY_ALLOWED_VALUES_DISPLAY}"
                ))
            }
        }
        Some("check-keys") => {
            if val.as_bool().is_none() {
                Err(
                    "invalid config: option \"check-keys\" of \"truthy\" should be bool"
                        .to_string(),
                )
            } else {
                Ok(())
            }
        }
        _ => Err(unknown_rule_option("truthy", key)),
    }
}

fn validate_key_ordering_option(
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
        Some("spaces") => {
            if yaml_value_matches_toml_type::<SpacesSetting>(val) {
                Ok(())
            } else {
                Err(
                    "invalid config: option \"spaces\" of \"indentation\" should be in (<class 'int'>, 'consistent')"
                        .to_string(),
                )
            }
        }
        Some("indent-sequences") => {
            if yaml_value_matches_toml_type::<IndentSequencesSetting>(val) {
                Ok(())
            } else {
                Err(
                    "invalid config: option \"indent-sequences\" of \"indentation\" should be in (<class 'bool'>, 'whatever', 'consistent')"
                        .to_string(),
                )
            }
        }
        Some("check-multi-line-strings") => {
            if val.as_bool().is_some() {
                Ok(())
            } else {
                Err(
                    "invalid config: option \"check-multi-line-strings\" of \"indentation\" should be bool"
                        .to_string(),
                )
            }
        }
        _ => Err(unknown_rule_option("indentation", key)),
    }
}

fn validate_quoted_strings_rule(map: &MappingOwned) -> Result<(), String> {
    let mut state = QuotedStringsValidationState::default();
    for (key, val) in map {
        if handle_common_rule_key("quoted-strings", key, val)? {
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
        Some("quote-type") => validate_quote_type_option(val),
        Some("required") => validate_required_option(val, state),
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

fn validate_quote_type_option(val: &YamlOwned) -> Result<(), String> {
    if yaml_value_matches_toml_type::<QuoteType>(val) {
        Ok(())
    } else {
        Err(
            "invalid config: option \"quote-type\" of \"quoted-strings\" should be in ('any', 'single', 'double')"
                .to_string(),
        )
    }
}

fn validate_required_option(
    val: &YamlOwned,
    state: &mut QuotedStringsValidationState,
) -> Result<(), String> {
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
    if val.as_bool().is_some() {
        Ok(())
    } else {
        Err(format!(
            "invalid config: option \"{option_name}\" of \"{rule_name}\" should be bool"
        ))
    }
}

fn validate_document_presence_option(
    rule: &str,
    key: &YamlOwned,
    val: &YamlOwned,
) -> Result<(), String> {
    match key.as_str() {
        Some("present") => validate_bool_option(val, rule, "present"),
        _ => Err(unknown_rule_option(rule, key)),
    }
}

fn unknown_rule_option(rule: &str, key: &YamlOwned) -> String {
    let key_name = describe_rule_option_key(key);
    format!("invalid config: unknown option \"{key_name}\" for rule \"{rule}\"")
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
            let Some(key) = k.as_str() else {
                continue;
            };
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

fn describe_rule_option_key(key: &YamlOwned) -> String {
    match (
        key.as_integer(),
        key.as_floating_point(),
        key.as_bool(),
        key.is_null(),
        key.as_str(),
    ) {
        (Some(num), _, _, _, _) => num.to_string(),
        (None, Some(float), _, _, _) => float.to_string(),
        (None, None, Some(flag), _, _) => flag.to_string(),
        (None, None, None, true, _) => "None".to_string(),
        (None, None, None, false, Some(text)) => text.to_owned(),
        _ => format!("{key:?}"),
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
        TomlFixableRuleSelector::NewLines => FixRuleSelector::Rule(FixRule::NewLines),
    }
}

fn typed_fix_rule(rule: TomlFixRuleName) -> FixRule {
    match rule {
        TomlFixRuleName::Braces => FixRule::Braces,
        TomlFixRuleName::Brackets => FixRule::Brackets,
        TomlFixRuleName::Commas => FixRule::Commas,
        TomlFixRuleName::Comments => FixRule::Comments,
        TomlFixRuleName::CommentsIndentation => FixRule::CommentsIndentation,
        TomlFixRuleName::NewLineAtEndOfFile => FixRule::NewLineAtEndOfFile,
        TomlFixRuleName::NewLines => FixRule::NewLines,
    }
}

/// Result of configuration discovery.
#[derive(Debug, Clone)]
pub struct ConfigContext {
    pub config: YamlLintConfig,
    pub base_dir: PathBuf,
    pub source: Option<PathBuf>,
    pub notices: Vec<String>,
}

fn finalize_context(
    envx: &dyn Env,
    mut cfg: YamlLintConfig,
    base_dir: impl Into<PathBuf>,
    source: Option<PathBuf>,
    notices: Vec<String>,
) -> Result<ConfigContext, String> {
    let base_dir = base_dir.into();
    cfg.finalize(envx, &base_dir)?;
    Ok(ConfigContext {
        config: cfg,
        base_dir,
        source,
        notices,
    })
}

/// Discover configuration with precedence:
/// config-data > config-file > project (TOML-first, YAML fallback) > env var > user-global > defaults.
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
/// Panics only if built-in preset YAML cannot be parsed, which indicates a programming error.
pub fn discover_config_with(
    inputs: &[PathBuf],
    overrides: &Overrides,
    envx: &dyn Env,
) -> Result<ConfigContext, String> {
    // Global config resolution: inline > file > project > env var.
    if let Some(ref data) = overrides.config_data {
        let base_dir = envx.current_dir();
        let cfg =
            YamlLintConfig::from_yaml_str_with_env(data, Some(envx), Some(&base_dir))?;
        return finalize_context(envx, cfg, base_dir, None, Vec::new());
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
                YamlLintConfig::from_yaml_str(conf::builtin("default").unwrap())
                    .expect("builtin preset must parse"),
                cwd,
                None,
                Vec::new(),
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
/// Panics only if the built-in default preset is not embedded (programming error).
pub fn discover_config_with_env(
    inputs: &[PathBuf],
    overrides: &Overrides,
    env_get: &dyn Fn(&str) -> Option<String>,
) -> Result<ConfigContext, String> {
    discover_config_with(inputs, overrides, &ClosureEnv { get: env_get })
}

/// Discover the config for a single file path, ignoring env/global overrides.
/// Precedence: nearest project config up-tree (TOML-first, YAML fallback),
/// then user-global, then defaults.
///
/// # Errors
/// Returns an error when a config file cannot be read or parsed.
/// Discover the effective config for a single file.
///
/// # Errors
/// Returns an error when a config file cannot be read or parsed.
///
/// # Panics
/// Panics only if the built-in default preset is not embedded (programming error).
pub fn discover_per_file(path: &Path) -> Result<ConfigContext, String> {
    discover_per_file_with(path, &SystemEnv)
}

/// Discover the effective config for a single file using a provided `Env`.
///
/// # Errors
/// Returns an error when a configuration file cannot be read or parsed.
///
/// # Panics
/// Panics only if the built-in default preset cannot be parsed.
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
                YamlLintConfig::from_yaml_str(conf::builtin("default").unwrap())
                    .expect("builtin preset must parse"),
                envx.current_dir(),
                None,
                Vec::new(),
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
    finalize_context(envx, cfg, base, Some(p.to_path_buf()), notices)
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

fn try_user_global_core(
    envx: &dyn Env,
    base_dir: &Path,
) -> Result<Option<ConfigContext>, String> {
    envx.config_dir()
        .map(|base| base.join("yamllint").join("config"))
        .filter(|p| envx.path_exists(p))
        .map(|p| {
            let data = envx.read_to_string(&p)?;
            let cfg = YamlLintConfig::from_yaml_str_with_env(
                &data,
                Some(envx),
                Some(base_dir),
            )?;
            finalize_context(envx, cfg, base_dir.to_path_buf(), Some(p), Vec::new())
        })
        .transpose()
}

const TOML_PROJECT_CONFIG_CANDIDATES: [&str; 3] =
    [".ryl.toml", "ryl.toml", "pyproject.toml"];
const YAML_PROJECT_CONFIG_CANDIDATES: [&str; 3] =
    [".yamllint", ".yamllint.yaml", ".yamllint.yml"];

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
