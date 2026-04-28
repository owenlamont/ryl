use std::borrow::Cow;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use globset::{Glob, GlobMatcher};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use saphyr::{LoadableYamlNode, ScalarOwned, YamlOwned};

use crate::config_schema::{
    FixRuleName as TomlFixRuleName, FixableRuleSelector as TomlFixableRuleSelector,
    NormalizedConfig, NormalizedFixConfig, TomlConfig, normalize_toml_config,
    normalized_config_to_toml_value, parse_toml_config_str, parse_yaml_config,
    validate_toml_config, yaml_rule_filter_patterns, yaml_rule_level,
};
use crate::{conf, decoder};

pub use crate::config_schema::RuleLevel;

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
    per_file_ignores: BTreeMap<String, Vec<String>>,
    per_file_ignore_matchers: Vec<PerFileIgnore>,
    rule_names: Vec<String>,
    rules: std::collections::BTreeMap<String, RuleConfig>,
    yaml_file_patterns: Vec<String>,
    yaml_matcher: Option<Gitignore>,
    locale: Option<String>,
    fix: FixConfig,
}

const DEFAULT_YAML_FILE_PATTERNS: [&str; 3] = ["*.yaml", "*.yml", ".yamllint"];

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
        let (negated, pattern) = pattern
            .strip_prefix('!')
            .map_or((false, pattern), |stripped| (true, stripped));
        let absolute_pattern = if Path::new(pattern).is_absolute() {
            PathBuf::from(pattern)
        } else {
            base_dir.join(pattern)
        };
        let basename_matcher = Glob::new(pattern)
            .map_err(|err| {
                format!(
                    "invalid config: per-file-ignores pattern '{pattern}' is invalid: {err}"
                )
            })?
            .compile_matcher();
        let absolute_matcher = Glob::new(&absolute_pattern.to_string_lossy())
            .map_err(|err| {
                format!(
                    "invalid config: per-file-ignores pattern '{pattern}' is invalid: {err}"
                )
            })?
            .compile_matcher();
        Ok(Self {
            basename_matcher,
            absolute_matcher,
            negated,
            rules,
        })
    }

    fn matches(&self, path: &Path, base_dir: &Path) -> bool {
        let filename_matches = path.file_name().is_some_and(|file_name| {
            self.basename_matcher.is_match(Path::new(file_name))
        });
        let absolute_path = if path.is_absolute() {
            Cow::Borrowed(path)
        } else {
            Cow::Owned(base_dir.join(path))
        };
        let path_matches = self.absolute_matcher.is_match(absolute_path.as_ref());

        if self.negated {
            !filename_matches && !path_matches
        } else {
            filename_matches || path_matches
        }
    }
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
    NewLineAtEndOfFile,
    NewLines,
}

impl FixRule {
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
            rule_names: Vec::new(),
            rules: std::collections::BTreeMap::new(),
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

    /// Returns true when `path` should be ignored according to config patterns.
    /// Matching is performed on the path relative to `base_dir`.
    #[must_use]
    pub fn is_file_ignored(&self, path: &Path, base_dir: &Path) -> bool {
        self.ignore_matcher
            .as_ref()
            .is_some_and(|matcher| path_matches_ignore(matcher, path, base_dir))
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
        cfg.apply_normalized_config(normalized);
        cfg
    }

    fn from_doc_with_env(
        doc: &YamlOwned,
        envx: Option<&dyn Env>,
        base_dir: Option<&Path>,
    ) -> Result<Self, String> {
        let parsed = parse_yaml_config(doc)?;
        let mut cfg = Self::default();
        let base_path = base_dir.unwrap_or_else(|| Path::new(""));
        for entry in &parsed.extends {
            cfg.extend_from_entry(entry, envx, base_path)?;
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
        self.per_file_ignore_matchers = other.per_file_ignore_matchers;
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

        if !normalized.per_file_ignores.is_empty() {
            self.per_file_ignores = normalized.per_file_ignores;
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

        self.build_yaml_matcher(base_dir);

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
        FixRuleSelector::Rule(FixRule::NewLines) => TomlFixableRuleSelector::NewLines,
    }
}

fn normalized_fix_rule(rule: FixRule) -> TomlFixRuleName {
    match rule {
        FixRule::Braces => TomlFixRuleName::Braces,
        FixRule::Brackets => TomlFixRuleName::Brackets,
        FixRule::Commas => TomlFixRuleName::Commas,
        FixRule::Comments => TomlFixRuleName::Comments,
        FixRule::CommentsIndentation => TomlFixRuleName::CommentsIndentation,
        FixRule::NewLineAtEndOfFile => TomlFixRuleName::NewLineAtEndOfFile,
        FixRule::NewLines => TomlFixRuleName::NewLines,
    }
}

fn normalized_config_from_runtime(config: &YamlLintConfig) -> NormalizedConfig {
    NormalizedConfig {
        ignore_patterns: (!config.ignore_patterns.is_empty())
            .then(|| config.ignore_patterns.clone()),
        ignore_from_files: (!config.ignore_from_files.is_empty())
            .then(|| config.ignore_from_files.clone()),
        per_file_ignores: config.per_file_ignores.clone(),
        yaml_file_patterns: Some(config.yaml_file_patterns.clone()),
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
