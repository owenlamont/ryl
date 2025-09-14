use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};
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
#[derive(Debug, Clone, Default)]
pub struct YamlLintConfig {
    ignore_patterns: Vec<String>,
    #[allow(clippy::struct_field_names)]
    matcher: Option<GlobSet>,
    rule_names: Vec<String>,
    rules: std::collections::BTreeMap<String, YamlOwned>,
    yaml_file_patterns: Vec<String>,
    yaml_matcher: Option<GlobSet>,
}

#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub config_file: Option<PathBuf>,
    pub config_data: Option<String>,
}

impl YamlLintConfig {
    fn apply_extends(&mut self, node: &YamlOwned) {
        if let Some(y) = node.as_str().and_then(conf::builtin) {
            let base = Self::from_yaml_str(y).expect("builtin preset must parse");
            self.merge_from(base);
            return;
        }
        node.as_sequence()
            .into_iter()
            .flat_map(|seq| seq.iter())
            .filter_map(|it| it.as_str().and_then(conf::builtin))
            .for_each(|y| {
                let base = Self::from_yaml_str(y).expect("builtin preset must parse");
                self.merge_from(base);
            });
    }
    #[must_use]
    pub fn ignore_patterns(&self) -> &[String] {
        &self.ignore_patterns
    }

    #[must_use]
    pub fn rule_names(&self) -> &[String] {
        &self.rule_names
    }

    fn build_matcher(&mut self) {
        self.matcher = if self.ignore_patterns.is_empty() {
            None
        } else {
            let mut b = GlobSetBuilder::new();
            for pat in &self.ignore_patterns {
                if let Ok(glob) = Glob::new(pat) {
                    b.add(glob);
                }
            }
            b.build().ok()
        };

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
        let Some(matcher) = &self.matcher else {
            return false;
        };
        let rel = path.strip_prefix(base_dir).map_or(path, |r| r);
        matcher.is_match(rel)
    }

    #[must_use]
    pub fn is_yaml_candidate(&self, path: &Path, base_dir: &Path) -> bool {
        let rel = path.strip_prefix(base_dir).map_or(path, |r| r);
        if let Some(matcher) = &self.yaml_matcher {
            return matcher.is_match(rel);
        }
        crate::discover::is_yaml_path(path)
    }

    fn from_yaml_str(s: &str) -> Result<Self, String> {
        let docs =
            YamlOwned::load_from_str(s).map_err(|e| format!("failed to parse config data: {e}"))?;
        let mut cfg = Self::default();

        let doc = &docs[0];

        // Handle `extends` first (string or sequence)
        if let Some(extends) = doc.as_mapping_get("extends") {
            cfg.apply_extends(extends);
        }

        // Current document overrides
        if let Some(ignore) = doc.as_mapping_get("ignore") {
            if let Some(seq) = ignore.as_sequence() {
                for it in seq {
                    if let Some(s) = it.as_str() {
                        cfg.ignore_patterns.push(s.to_owned());
                    }
                }
            } else if let Some(s) = ignore.as_str() {
                cfg.ignore_patterns.push(s.to_owned());
            }
        }

        if let Some(yf) = doc.as_mapping_get("yaml-files") {
            if let Some(seq) = yf.as_sequence() {
                for it in seq {
                    if let Some(s) = it.as_str() {
                        cfg.yaml_file_patterns.push(s.to_owned());
                    }
                }
            } else if let Some(s) = yf.as_str() {
                cfg.yaml_file_patterns.push(s.to_owned());
            }
        }

        if let Some(rules) = doc.as_mapping_get("rules")
            && let Some(map) = rules.as_mapping()
        {
            for (k, v) in map {
                let Some(name) = k.as_str() else {
                    continue;
                };
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

        cfg.build_matcher();
        Ok(cfg)
    }

    fn merge_from(&mut self, mut other: Self) {
        // Merge ignore patterns (append, then dedup later during matcher build)
        self.ignore_patterns.append(&mut other.ignore_patterns);
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
        // Merge yaml file patterns
        self.yaml_file_patterns
            .append(&mut other.yaml_file_patterns);
        self.build_matcher();
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
pub fn discover_config_with(
    inputs: &[PathBuf],
    overrides: &Overrides,
    envx: &dyn Env,
) -> Result<ConfigContext, String> {
    // Global config resolution: inline > file > env var.
    if let Some(ref data) = overrides.config_data {
        let cfg = YamlLintConfig::from_yaml_str(data)?;
        return Ok(ConfigContext {
            config: cfg,
            base_dir: envx.current_dir(),
            source: None,
        });
    }
    if let Some(ref file) = overrides.config_file {
        let data = envx.read_to_string(file)?;
        let cfg = YamlLintConfig::from_yaml_str(&data)?;
        let base = file
            .parent()
            .map_or_else(|| envx.current_dir(), Path::to_path_buf);
        return Ok(ConfigContext {
            config: cfg,
            base_dir: base,
            source: Some(file.clone()),
        });
    }
    if let Some(ctx) = try_env_config_core(envx)? {
        return Ok(ctx);
    }
    if let Some((cfg_path, base_dir)) = find_project_config_core(envx, inputs) {
        let data = envx.read_to_string(&cfg_path)?;
        let cfg = YamlLintConfig::from_yaml_str(&data)?;
        return Ok(ConfigContext {
            config: cfg,
            base_dir,
            source: Some(cfg_path),
        });
    }
    try_user_global_core(envx, envx.current_dir())?.map_or_else(
        || {
            Ok(ConfigContext {
                config: YamlLintConfig::default(),
                base_dir: envx.current_dir(),
                source: None,
            })
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
        let cfg = YamlLintConfig::from_yaml_str(&data)?;
        return Ok(ConfigContext {
            config: cfg,
            base_dir,
            source: Some(cfg_path),
        });
    }
    try_user_global_core(envx, start_dir.to_path_buf())?.map_or_else(
        || {
            let cfg = YamlLintConfig::from_yaml_str(conf::builtin("default").unwrap())
                .expect("builtin preset must parse");
            Ok(ConfigContext {
                config: cfg,
                base_dir: envx.current_dir(),
                source: None,
            })
        },
        Ok,
    )
}

// Testable core helpers below.
fn ctx_from_config_path_core(envx: &dyn Env, p: &Path) -> Result<ConfigContext, String> {
    let data = envx.read_to_string(p)?;
    let cfg = YamlLintConfig::from_yaml_str(&data)?;
    let base = p
        .parent()
        .map_or_else(|| envx.current_dir(), Path::to_path_buf);
    Ok(ConfigContext {
        config: cfg,
        base_dir: base,
        source: Some(p.to_path_buf()),
    })
}

fn try_env_config_core(envx: &dyn Env) -> Result<Option<ConfigContext>, String> {
    envx.env_var("YAMLLINT_CONFIG_FILE")
        .map(PathBuf::from)
        .filter(|p| envx.path_exists(p))
        .map(|p| ctx_from_config_path_core(envx, &p))
        .transpose()
}

// no separate try_env_config_with; discover_config_with_env uses ClosureEnv + discover_config_with

fn try_user_global_core(
    envx: &dyn Env,
    base_dir: PathBuf,
) -> Result<Option<ConfigContext>, String> {
    envx.config_dir()
        .map(|base| base.join("yamllint").join("config"))
        .filter(|p| envx.path_exists(p))
        .map(|p| {
            let data = envx.read_to_string(&p)?;
            let cfg = YamlLintConfig::from_yaml_str(&data)?;
            Ok(ConfigContext {
                config: cfg,
                base_dir,
                source: Some(p),
            })
        })
        .transpose()
}

fn find_project_config_core(envx: &dyn Env, inputs: &[PathBuf]) -> Option<(PathBuf, PathBuf)> {
    let mut starts: Vec<PathBuf> = Vec::new();
    if inputs.is_empty() {
        starts.push(envx.current_dir());
    } else {
        for p in inputs {
            let s = if p.is_dir() {
                p.clone()
            } else {
                p.parent()
                    .map_or_else(|| envx.current_dir(), Path::to_path_buf)
            };
            if !starts.iter().any(|e| e == &s) {
                starts.push(s);
            }
        }
    }
    let candidates = [".yamllint", ".yamllint.yaml", ".yamllint.yml"];
    for start in starts {
        let mut dir = start.as_path();
        loop {
            for name in candidates {
                let cand = dir.join(name);
                if envx.path_exists(&cand) {
                    return Some((cand, dir.to_path_buf()));
                }
            }
            match dir.parent() {
                Some(parent) if parent != dir => dir = parent,
                _ => break,
            }
        }
    }
    None
}
