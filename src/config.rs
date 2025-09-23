use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use saphyr::{LoadableYamlNode, ScalarOwned, YamlOwned};

use crate::conf;

/// Abstraction over environment/filesystem to enable full test coverage.
/// Minimal environment abstraction used by tests to cover file system and env-var behavior.
pub trait Env {
    /// Current working directory.
    fn current_dir(&self) -> PathBuf;
    /// Platform configuration directory (e.g., XDG config dir).
    fn config_dir(&self) -> Option<PathBuf>;
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
        dirs::config_dir()
    }
    fn read_to_string(&self, p: &Path) -> Result<String, String> {
        match fs::read_to_string(p) {
            Ok(s) => Ok(s),
            Err(e) => Err(format!("failed to read config file {}: {e}", p.display())),
        }
    }
    fn path_exists(&self, p: &Path) -> bool {
        p.exists()
    }
    fn env_var(&self, key: &str) -> Option<String> {
        env::var(key).ok()
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
    yaml_file_patterns: Vec<String>,
    yaml_matcher: Option<GlobSet>,
    locale: Option<String>,
}

const DEFAULT_YAML_FILE_PATTERNS: [&str; 3] = ["*.yaml", "*.yml", ".yamllint"];

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

impl Default for YamlLintConfig {
    fn default() -> Self {
        Self {
            ignore_patterns: Vec::new(),
            ignore_from_files: Vec::new(),
            ignore_matcher: None,
            rule_names: Vec::new(),
            rules: std::collections::BTreeMap::new(),
            yaml_file_patterns: DEFAULT_YAML_FILE_PATTERNS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            yaml_matcher: None,
            locale: None,
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
        let node = self.rules.get(rule)?;
        let map = node.as_mapping()?;
        map.iter()
            .find_map(|(key, value)| (key.as_str() == Some(option)).then(|| value.as_str()))
            .flatten()
    }

    #[must_use]
    pub fn locale(&self) -> Option<&str> {
        self.locale.as_deref()
    }

    fn build_yaml_matcher(&mut self) {
        self.yaml_matcher = if self.yaml_file_patterns.is_empty() {
            None
        } else {
            let mut b = GlobSetBuilder::new();
            for pat in &self.yaml_file_patterns {
                if let Ok(glob) = Glob::new(pat) {
                    b.add(glob);
                }
            }
            b.build().ok()
        };
    }

    /// Returns true when `path` should be ignored according to config patterns.
    /// Matching is performed on the path relative to `base_dir`.
    #[must_use]
    pub fn is_file_ignored(&self, path: &Path, base_dir: &Path) -> bool {
        let Some(matcher) = &self.ignore_matcher else {
            return false;
        };
        let rel = path.strip_prefix(base_dir).map_or(path, |r| r);
        matcher.matched_path_or_any_parents(rel, false).is_ignore()
    }

    #[must_use]
    pub fn is_yaml_candidate(&self, path: &Path, base_dir: &Path) -> bool {
        let rel = path.strip_prefix(base_dir).map_or(path, |r| r);
        if let Some(matcher) = &self.yaml_matcher {
            return matcher.is_match(rel);
        }
        crate::discover::is_yaml_path(path)
    }

    fn from_yaml_str_with_env(
        s: &str,
        envx: Option<&dyn Env>,
        base_dir: Option<&Path>,
    ) -> Result<Self, String> {
        let docs =
            YamlOwned::load_from_str(s).map_err(|e| format!("failed to parse config data: {e}"))?;
        let mut cfg = Self::default();

        let doc = &docs[0];
        if doc.as_mapping().is_none() {
            return Err("invalid config: not a mapping".to_string());
        }

        // Handle `extends` first (string or sequence)
        if let Some(extends) = doc.as_mapping_get("extends") {
            cfg.apply_extends(extends, envx, base_dir)?;
        }

        // Current document overrides
        let ignore = doc.as_mapping_get("ignore");
        let ignore_from_file = doc.as_mapping_get("ignore-from-file");
        if ignore.is_some() && ignore_from_file.is_some() {
            return Err(
                "invalid config: ignore and ignore-from-file keys cannot be used together"
                    .to_string(),
            );
        }

        if let Some(node) = ignore {
            let mut patterns = load_ignore_patterns(node)?;
            cfg.ignore_patterns.append(&mut patterns);
        }

        if let Some(node) = ignore_from_file {
            cfg.ignore_from_files = load_ignore_from_files(node)?;
        }

        if let Some(yf) = doc.as_mapping_get("yaml-files") {
            if let Some(seq) = yf.as_sequence() {
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
            } else {
                return Err(
                    "invalid config: yaml-files should be a list of file patterns".to_string(),
                );
            }
        }

        if let Some(locale) = doc.as_mapping_get("locale") {
            let Some(loc) = locale.as_str() else {
                return Err("invalid config: locale should be a string".to_string());
            };
            cfg.locale = Some(loc.to_owned());
        }

        if let Some(rules) = doc.as_mapping_get("rules")
            && let Some(map) = rules.as_mapping()
        {
            for (k, v) in map {
                let Some(name) = k.as_str() else {
                    continue;
                };
                validate_rule_value(name, v)?;
                if let Some(dst) = cfg.rules.get_mut(name) {
                    deep_merge_yaml_owned(dst, v);
                } else {
                    cfg.rules.insert(name.to_owned(), v.clone());
                }
                let mut seen = false;
                for e in &cfg.rule_names {
                    if e == name {
                        seen = true;
                        break;
                    }
                }
                if !seen {
                    cfg.rule_names.push(name.to_owned());
                }
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
            if let Some(dst) = self.rules.get_mut(&name) {
                deep_merge_yaml_owned(dst, &val);
            } else {
                self.rules.insert(name.clone(), val.clone());
            }
            if !self.rule_names.iter().any(|e| e == &name) {
                self.rule_names.push(name);
            }
        }
        if !other.yaml_file_patterns.is_empty() {
            self.yaml_file_patterns = other.yaml_file_patterns;
        }
        if self.locale.is_none() {
            self.locale = other.locale;
        }
    }

    fn finalize(&mut self, envx: &dyn Env, base_dir: &Path) -> Result<(), String> {
        let mut builder = GitignoreBuilder::new(base_dir);
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

        self.build_yaml_matcher();
        Ok(())
    }
}

fn load_ignore_patterns(node: &YamlOwned) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    if let Some(seq) = node.as_sequence() {
        for it in seq {
            let Some(s) = it.as_str() else {
                return Err("invalid config: ignore should contain file patterns".to_string());
            };
            out.extend(patterns_from_scalar(s));
        }
    } else if let Some(s) = node.as_str() {
        out.extend(patterns_from_scalar(s));
    } else {
        return Err("invalid config: ignore should contain file patterns".to_string());
    }
    Ok(out)
}

fn load_ignore_from_files(node: &YamlOwned) -> Result<Vec<String>, String> {
    if let Some(seq) = node.as_sequence() {
        let mut files = Vec::new();
        for it in seq {
            let Some(s) = it.as_str() else {
                return Err(
                    "invalid config: ignore-from-file should contain filename(s), either as a list or string"
                        .to_string(),
                );
            };
            files.push(s.to_owned());
        }
        Ok(files)
    } else if let Some(s) = node.as_str() {
        Ok(vec![s.to_owned()])
    } else {
        Err(
            "invalid config: ignore-from-file should contain filename(s), either as a list or string"
                .to_string(),
        )
    }
}

fn patterns_from_scalar(value: &str) -> Vec<String> {
    value
        .lines()
        .map(|line| line.trim_end_matches(['\r']))
        .filter(|line| !line.trim().is_empty())
        .map(std::string::ToString::to_string)
        .collect()
}

fn determine_rule_level(node: &YamlOwned) -> Option<RuleLevel> {
    if let Some(s) = node.as_str() {
        return if s == "disable" {
            None
        } else {
            Some(RuleLevel::Error)
        };
    }

    node.as_mapping()
        .and_then(|map| {
            map.iter().find_map(|(key, value)| {
                (key.as_str() == Some("level")).then(|| value.as_str().and_then(RuleLevel::parse))
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

    if let Some(map) = value.as_mapping() {
        for (key, val) in map {
            if key.as_str() == Some("level") {
                let Some(level_text) = val.as_str() else {
                    return Err(format!(
                        "invalid config: rule '{name}' level should be \"error\" or \"warning\""
                    ));
                };
                if RuleLevel::parse(level_text).is_none() {
                    return Err(format!(
                        "invalid config: rule '{name}' level should be \"error\" or \"warning\""
                    ));
                }
                continue;
            }

            if name == "new-lines" {
                if key.as_str() == Some("type") {
                    let Some(kind) = val.as_str() else {
                        return Err(
                            "invalid config: option \"type\" of \"new-lines\" should be in ('unix', 'dos', 'platform')"
                                .to_string(),
                        );
                    };
                    if !matches!(kind, "unix" | "dos" | "platform") {
                        return Err(
                            "invalid config: option \"type\" of \"new-lines\" should be in ('unix', 'dos', 'platform')"
                                .to_string(),
                        );
                    }
                } else {
                    let key_name = match (
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
                    };
                    return Err(format!(
                        "invalid config: unknown option \"{key_name}\" for rule \"new-lines\""
                    ));
                }
            }
        }
        return Ok(());
    }

    Err(format!(
        "invalid config: rule '{name}' should be 'enable', 'disable', or a mapping"
    ))
}

fn resolve_extend_path(entry: &str, envx: &dyn Env, base_dir: Option<&Path>) -> PathBuf {
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

/// Result of configuration discovery.
#[derive(Debug, Clone)]
pub struct ConfigContext {
    pub config: YamlLintConfig,
    pub base_dir: PathBuf,
    pub source: Option<PathBuf>,
}

fn finalize_context(
    envx: &dyn Env,
    mut cfg: YamlLintConfig,
    base_dir: impl Into<PathBuf>,
    source: Option<PathBuf>,
) -> Result<ConfigContext, String> {
    let base_dir = base_dir.into();
    cfg.finalize(envx, &base_dir)?;
    Ok(ConfigContext {
        config: cfg,
        base_dir,
        source,
    })
}

/// Discover configuration with precedence inspired by yamllint:
/// config-data > config-file > project > user-global > defaults.
///
/// # Errors
/// Returns an error when a config file cannot be read or parsed.
pub fn discover_config(inputs: &[PathBuf], overrides: &Overrides) -> Result<ConfigContext, String> {
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
        let cfg = YamlLintConfig::from_yaml_str_with_env(data, Some(envx), Some(&base_dir))?;
        return finalize_context(envx, cfg, base_dir, None);
    }
    if let Some(ref file) = overrides.config_file {
        let base = file
            .parent()
            .map_or_else(|| envx.current_dir(), Path::to_path_buf);
        let data = envx.read_to_string(file)?;
        let cfg = YamlLintConfig::from_yaml_str_with_env(&data, Some(envx), Some(&base))?;
        return finalize_context(envx, cfg, base, Some(file.clone()));
    }
    if let Some((cfg_path, base_dir)) = find_project_config_core(envx, inputs) {
        let data = envx.read_to_string(&cfg_path)?;
        let cfg = YamlLintConfig::from_yaml_str_with_env(&data, Some(envx), Some(&base_dir))?;
        return finalize_context(envx, cfg, base_dir, Some(cfg_path));
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
    _inputs: &[PathBuf],
    overrides: &Overrides,
    env_get: &dyn Fn(&str) -> Option<String>,
) -> Result<ConfigContext, String> {
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
    discover_config_with(&[], overrides, &ClosureEnv { get: env_get })
}

/// Discover the config for a single file path, ignoring env/global overrides.
/// Precedence: nearest project config up-tree from the file's directory,
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
pub fn discover_per_file_with(path: &Path, envx: &dyn Env) -> Result<ConfigContext, String> {
    let start_dir = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };

    if let Some((cfg_path, base_dir)) = find_project_config_core(envx, &[start_dir.to_path_buf()]) {
        let data = envx.read_to_string(&cfg_path)?;
        let cfg = YamlLintConfig::from_yaml_str_with_env(&data, Some(envx), Some(&base_dir))?;
        return finalize_context(envx, cfg, base_dir, Some(cfg_path));
    }
    try_user_global_core(envx, start_dir)?.map_or_else(
        || {
            finalize_context(
                envx,
                YamlLintConfig::from_yaml_str(conf::builtin("default").unwrap())
                    .expect("builtin preset must parse"),
                envx.current_dir(),
                None,
            )
        },
        Ok,
    )
}

// Testable core helpers below.
fn ctx_from_config_path_core(envx: &dyn Env, p: &Path) -> Result<ConfigContext, String> {
    let data = envx.read_to_string(p)?;
    let base = p
        .parent()
        .map_or_else(|| envx.current_dir(), Path::to_path_buf);
    let cfg = YamlLintConfig::from_yaml_str_with_env(&data, Some(envx), Some(&base))?;
    finalize_context(envx, cfg, base, Some(p.to_path_buf()))
}

fn try_env_config_core(envx: &dyn Env) -> Result<Option<ConfigContext>, String> {
    envx.env_var("YAMLLINT_CONFIG_FILE")
        .map(PathBuf::from)
        .filter(|p| envx.path_exists(p))
        .map(|p| ctx_from_config_path_core(envx, &p))
        .transpose()
}

// no separate try_env_config_with; discover_config_with_env uses ClosureEnv + discover_config_with

fn try_user_global_core(envx: &dyn Env, base_dir: &Path) -> Result<Option<ConfigContext>, String> {
    envx.config_dir()
        .map(|base| base.join("yamllint").join("config"))
        .filter(|p| envx.path_exists(p))
        .map(|p| {
            let data = envx.read_to_string(&p)?;
            let cfg = YamlLintConfig::from_yaml_str_with_env(&data, Some(envx), Some(base_dir))?;
            finalize_context(envx, cfg, base_dir.to_path_buf(), Some(p))
        })
        .transpose()
}

fn find_project_config_core(envx: &dyn Env, inputs: &[PathBuf]) -> Option<(PathBuf, PathBuf)> {
    let mut starts: Vec<PathBuf> = Vec::new();
    let cwd = envx.current_dir();
    if inputs.is_empty() {
        starts.push(cwd.clone());
    } else {
        for p in inputs {
            let s = if p.is_dir() {
                p.clone()
            } else {
                p.parent().map_or_else(|| cwd.clone(), Path::to_path_buf)
            };
            let abs = if s.is_absolute() { s } else { cwd.join(s) };
            if !starts.iter().any(|e| e == &abs) {
                starts.push(abs);
            }
        }
    }
    let candidates = [".yamllint", ".yamllint.yaml", ".yamllint.yml"];
    let home_dir = envx
        .env_var("HOME")
        .map(PathBuf::from)
        .or_else(dirs::home_dir);
    let home_abs = home_dir.as_ref().map(|h| {
        if h.is_absolute() {
            h.clone()
        } else {
            cwd.join(h)
        }
    });
    for start in starts {
        let mut dir = if start.is_absolute() {
            start
        } else {
            cwd.join(start)
        };
        loop {
            for name in candidates {
                let cand = dir.join(name);
                if envx.path_exists(&cand) {
                    return Some((cand, dir));
                }
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
    None
}
