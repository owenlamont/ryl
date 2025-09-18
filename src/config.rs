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
    fn from_yaml_str(s: &str) -> Result<Self, String> {
        Self::from_yaml_str_with_env(s, None, None)
    }

    fn apply_extends(
        &mut self,
        node: &YamlOwned,
        envx: Option<&dyn Env>,
        base_dir: Option<&Path>,
    ) -> Result<(), String> {
        match node {
            YamlOwned::Value(value) => {
                if let Some(ext) = value.as_str() {
                    self.extend_from_entry(ext, envx, base_dir)?;
                }
            }
            YamlOwned::Sequence(seq) => {
                for item in seq {
                    if let Some(ext) = item.as_str() {
                        self.extend_from_entry(ext, envx, base_dir)?;
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
        base_dir: Option<&Path>,
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

        let resolved = resolve_extend_path(entry, envx, base_dir);
        let data = match envx.read_to_string(&resolved) {
            Ok(text) => text,
            Err(err) => {
                return Err(format!(
                    "failed to read extended config {}: {err}",
                    resolved.display()
                ));
            }
        };
        let parent_dir = resolved.parent().map_or_else(
            || base_dir.map_or_else(|| envx.current_dir(), Path::to_path_buf),
            Path::to_path_buf,
        );
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
            if normalized.trim().is_empty() {
                continue;
            }
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

fn resolve_extend_path(entry: &str, envx: &dyn Env, base_dir: Option<&Path>) -> PathBuf {
    let candidate = PathBuf::from(entry);
    if candidate.is_absolute() {
        return candidate;
    }
    if let Some(base) = base_dir {
        let joined = base.join(&candidate);
        if envx.path_exists(&joined) {
            return joined;
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    #[derive(Default)]
    struct StubEnv {
        cwd: PathBuf,
        files: HashMap<PathBuf, String>,
        exists: HashSet<PathBuf>,
        vars: HashMap<String, String>,
    }

    impl StubEnv {
        fn new() -> Self {
            Self {
                cwd: PathBuf::from("."),
                ..Self::default()
            }
        }

        fn with_cwd(mut self, path: impl Into<PathBuf>) -> Self {
            self.cwd = path.into();
            self
        }

        fn with_file(mut self, path: impl Into<PathBuf>, content: impl Into<String>) -> Self {
            self.files.insert(path.into(), content.into());
            self
        }

        fn with_exists(mut self, path: impl Into<PathBuf>) -> Self {
            self.exists.insert(path.into());
            self
        }

        fn with_var(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
            self.vars.insert(key.into(), value.into());
            self
        }
    }

    impl Env for StubEnv {
        fn current_dir(&self) -> PathBuf {
            self.cwd.clone()
        }

        fn config_dir(&self) -> Option<PathBuf> {
            None
        }

        fn read_to_string(&self, p: &Path) -> Result<String, String> {
            self.files
                .get(p)
                .cloned()
                .ok_or_else(|| format!("missing file {}", p.display()))
        }

        fn path_exists(&self, p: &Path) -> bool {
            self.files.contains_key(p) || self.exists.contains(p)
        }

        fn env_var(&self, key: &str) -> Option<String> {
            self.vars.get(key).cloned()
        }
    }

    #[test]
    fn extends_mapping_is_ignored() {
        let yaml = "extends:\n  invalid: true\nrules: {}\n";
        let cfg = YamlLintConfig::from_yaml_str(yaml).expect("parse config");
        assert!(cfg.rule_names().is_empty());
    }

    #[test]
    fn stub_env_config_dir_none() {
        assert!(StubEnv::new().config_dir().is_none());
    }

    #[test]
    fn path_parent_empty_is_none() {
        assert!(Path::new("").parent().is_none());
    }

    #[test]
    fn extends_requires_env_error() {
        let err = YamlLintConfig::from_yaml_str("extends: child.yml\n")
            .expect_err("extends without env should error");
        assert!(err.contains("requires filesystem access"));
    }

    #[test]
    fn extends_missing_file_error() {
        let env = StubEnv::new().with_cwd(PathBuf::from("/workspace"));
        let err = YamlLintConfig::from_yaml_str_with_env(
            "extends: child.yml\n",
            Some(&env),
            Some(Path::new("/workspace")),
        )
        .expect_err("missing extended file should error");
        assert!(err.contains("failed to read extended config"));
    }

    #[test]
    fn extends_value_non_string_is_ignored() {
        let cfg = YamlLintConfig::from_yaml_str("extends: 123\nrules: {}\n")
            .expect("non-string extends should be ignored");
        assert!(cfg.rule_names().is_empty());
    }

    #[test]
    fn extends_sequence_skips_non_strings() {
        let cfg = YamlLintConfig::from_yaml_str("extends: [default, 1]\n")
            .expect("sequence with non-strings should parse");
        assert!(cfg.rule_names().iter().any(|r| r == "anchors"));
    }

    #[test]
    fn extends_sequence_missing_entry_errors() {
        let env = StubEnv::new()
            .with_cwd(PathBuf::from("/workspace"))
            .with_file(PathBuf::from("/workspace/base.yml"), "rules: {}\n");
        let err = YamlLintConfig::from_yaml_str_with_env(
            "extends: [base.yml, missing.yml]\n",
            Some(&env),
            Some(Path::new("/workspace")),
        )
        .expect_err("missing entry should error");
        assert!(err.contains("failed to read extended config"));
    }

    #[test]
    fn extend_invalid_yaml_propagates_error() {
        let env = StubEnv::new()
            .with_cwd(PathBuf::from("/workspace"))
            .with_file(PathBuf::from("/workspace/base.yml"), "- not mapping\n");
        let err = YamlLintConfig::from_yaml_str_with_env(
            "extends: base.yml\n",
            Some(&env),
            Some(Path::new("/workspace")),
        )
        .expect_err("invalid base should error");
        assert!(err.contains("invalid config"));
    }

    #[test]
    fn resolve_extend_path_prefers_absolute() {
        let env = StubEnv::new();
        let path = resolve_extend_path("/abs/conf.yml", &env, None);
        assert_eq!(path, PathBuf::from("/abs/conf.yml"));
    }

    #[test]
    fn resolve_extend_path_joins_base_dir() {
        let env = StubEnv::new().with_exists(PathBuf::from("/base/conf.yml"));
        let path = resolve_extend_path("conf.yml", &env, Some(Path::new("/base")));
        assert_eq!(path, PathBuf::from("/base/conf.yml"));
    }

    #[test]
    fn resolve_extend_path_falls_back_to_cwd() {
        let env = StubEnv::new()
            .with_cwd(PathBuf::from("/cwd"))
            .with_exists(PathBuf::from("/cwd/conf.yml"));
        let path = resolve_extend_path("conf.yml", &env, None);
        assert_eq!(path, PathBuf::from("/cwd/conf.yml"));
    }

    #[test]
    fn resolve_extend_path_returns_candidate() {
        let env = StubEnv::new().with_cwd(PathBuf::from("/cwd"));
        let path = resolve_extend_path("conf.yml", &env, Some(Path::new("/base")));
        assert_eq!(path, PathBuf::from("conf.yml"));
    }

    #[test]
    fn extend_merges_relative_config() {
        let env = StubEnv::new()
            .with_cwd(PathBuf::from("/workspace"))
            .with_file(
                PathBuf::from("/workspace/base.yml"),
                "rules:\n  base_rule: enable\n",
            );
        let cfg = YamlLintConfig::from_yaml_str_with_env(
            "extends: base.yml\nrules:\n  extra: enable\n",
            Some(&env),
            Some(Path::new("/workspace")),
        )
        .expect("extended config parses");
        assert!(cfg.rule_names().iter().any(|r| r == "base_rule"));
        assert!(cfg.rule_names().iter().any(|r| r == "extra"));
    }

    #[test]
    fn extend_uses_base_dir_when_parent_missing() {
        let env = StubEnv::new()
            .with_cwd(PathBuf::from("/cwd"))
            .with_file(PathBuf::from("base.yml"), "rules: {}\n");
        let cfg = YamlLintConfig::from_yaml_str_with_env(
            "extends: base.yml\n",
            Some(&env),
            Some(Path::new("/workspace")),
        )
        .expect("extend with base dir");
        assert!(cfg.rule_names().is_empty());
    }

    #[test]
    fn extend_defaults_to_env_cwd_when_no_base_dir() {
        let env = StubEnv::new()
            .with_cwd(PathBuf::from("/cwd"))
            .with_file(PathBuf::from("base.yml"), "rules: {}\n");
        let cfg = YamlLintConfig::from_yaml_str_with_env("extends: base.yml\n", Some(&env), None)
            .expect("extend without base dir");
        assert!(cfg.rule_names().is_empty());
    }

    #[test]
    fn extend_empty_entry_uses_base_dir() {
        let env = StubEnv::new()
            .with_cwd(PathBuf::from("/workspace"))
            .with_file(PathBuf::from(""), "rules: {}\n");
        let cfg = YamlLintConfig::from_yaml_str_with_env(
            "extends: ''\n",
            Some(&env),
            Some(Path::new("/workspace")),
        )
        .expect("extend empty should parse");
        assert!(cfg.rule_names().is_empty());
    }

    #[test]
    fn extend_empty_entry_uses_env_cwd() {
        let env = StubEnv::new()
            .with_cwd(PathBuf::from("/cwd"))
            .with_file(PathBuf::from(""), "rules: {}\n");
        let cfg = YamlLintConfig::from_yaml_str_with_env("extends: ''\n", Some(&env), None)
            .expect("extend empty should parse");
        assert!(cfg.rule_names().is_empty());
    }

    #[test]
    fn invalid_yaml_data_errors() {
        let err = YamlLintConfig::from_yaml_str("[1]").expect_err("non mapping should error");
        assert!(err.contains("not a mapping"));
    }

    #[test]
    fn yaml_files_non_sequence_errors() {
        let err =
            YamlLintConfig::from_yaml_str("yaml-files: 5\n").expect_err("non list should error");
        assert!(err.contains("yaml-files should be a list"));
    }

    #[test]
    fn yaml_files_invalid_pattern_is_skipped() {
        let mut cfg = YamlLintConfig::from_yaml_str("yaml-files: ['[']\n")
            .expect("invalid glob should parse");
        let env = StubEnv::new().with_cwd(PathBuf::from("/workspace"));
        cfg.finalize(&env, Path::new("/workspace"))
            .expect("finalize should succeed");
        assert!(
            !cfg.is_yaml_candidate(Path::new("/workspace/file.yaml"), Path::new("/workspace"),)
        );
    }

    #[test]
    fn ignore_and_ignore_from_file_conflict_errors() {
        let cfg = "ignore: ['a']\nignore-from-file: ['b']\n";
        let err = YamlLintConfig::from_yaml_str(cfg).expect_err("conflict should error");
        assert!(err.contains("cannot be used together"));
    }

    #[test]
    fn ignore_from_file_non_string_errors() {
        let cfg = "ignore-from-file: [1]\n";
        let err = YamlLintConfig::from_yaml_str(cfg).expect_err("non string should error");
        assert!(err.contains("ignore-from-file"));
    }

    #[test]
    fn ignore_from_file_invalid_mapping_errors() {
        let cfg = "ignore-from-file: { bad: 1 }\n";
        let err = YamlLintConfig::from_yaml_str(cfg).expect_err("mapping should error");
        assert!(err.contains("ignore-from-file should contain"));
    }

    #[test]
    fn ignore_patterns_non_string_errors() {
        let cfg = "ignore: [1]\n";
        let err = YamlLintConfig::from_yaml_str(cfg).expect_err("non string should error");
        assert!(err.contains("ignore should contain"));
    }

    #[test]
    fn ignore_from_file_string_parses() {
        let mut cfg = YamlLintConfig::from_yaml_str("ignore-from-file: .gitignore\n").unwrap();
        let env = StubEnv::new()
            .with_cwd(PathBuf::from("/workspace"))
            .with_file(PathBuf::from("/workspace/.gitignore"), "vendor/**\n");
        cfg.finalize(&env, Path::new("/workspace"))
            .expect("finalize");
        assert!(cfg.ignore_patterns().contains(&"vendor/**".to_string()));
    }

    #[test]
    fn discover_config_with_file_finalize() {
        let env = StubEnv::new()
            .with_cwd(PathBuf::from("/workspace"))
            .with_file(
                PathBuf::from("/workspace/override.yml"),
                "ignore: ['docs/**']\n",
            );
        let ctx = discover_config_with(
            &[],
            &Overrides {
                config_file: Some(PathBuf::from("/workspace/override.yml")),
                config_data: None,
            },
            &env,
        )
        .expect("discover override");
        assert!(ctx.config.ignore_patterns().iter().any(|p| p == "docs/**"));
    }

    #[test]
    fn discover_config_with_project_finalize() {
        let env = StubEnv::new()
            .with_cwd(PathBuf::from("/workspace"))
            .with_file(
                PathBuf::from("/workspace/project/.yamllint"),
                "ignore: ['build/**']\n",
            )
            .with_exists(PathBuf::from("/workspace/project/.yamllint"))
            .with_exists(PathBuf::from("/workspace/project/file.yaml"));
        let ctx = discover_config_with(
            &[PathBuf::from("/workspace/project/file.yaml")],
            &Overrides {
                config_file: None,
                config_data: None,
            },
            &env,
        )
        .expect("discover project");
        assert!(ctx.config.ignore_patterns().iter().any(|p| p == "build/**"));
    }

    #[test]
    fn locale_value_is_parsed() {
        let cfg = YamlLintConfig::from_yaml_str("locale: en_US.UTF-8\nrules: {}\n")
            .expect("locale parse");
        assert_eq!(cfg.locale(), Some("en_US.UTF-8"));
    }

    #[test]
    fn extend_preserves_existing_locale() {
        let env = StubEnv::new()
            .with_cwd(PathBuf::from("/workspace"))
            .with_file(
                PathBuf::from("/workspace/base.yml"),
                "locale: en_US.UTF-8\n",
            );
        let cfg = YamlLintConfig::from_yaml_str_with_env(
            "locale: fr_FR.UTF-8\nextends: base.yml\n",
            Some(&env),
            Some(Path::new("/workspace")),
        )
        .expect("extend locale");
        assert_eq!(cfg.locale(), Some("fr_FR.UTF-8"));
    }

    #[test]
    fn locale_non_string_errors() {
        let err =
            YamlLintConfig::from_yaml_str("locale: [1]\n").expect_err("locale list should error");
        assert!(err.contains("locale should be a string"));
    }

    #[test]
    fn patterns_from_scalar_skips_blank_lines() {
        let patterns = patterns_from_scalar("first\n\nsecond\n");
        assert_eq!(patterns, vec!["first".to_string(), "second".to_string()]);
    }

    #[test]
    fn finalize_skips_blank_patterns() {
        let mut cfg = YamlLintConfig::default();
        cfg.ignore_patterns.push("  ".to_string());
        let env = StubEnv::new().with_cwd(PathBuf::from("/workspace"));
        cfg.finalize(&env, Path::new("/workspace"))
            .expect("finalize");
    }

    #[test]
    fn finalize_invalid_ignore_pattern_errors() {
        let mut cfg = YamlLintConfig::default();
        cfg.ignore_patterns.push("[".to_string());
        let env = StubEnv::new().with_cwd(PathBuf::from("/workspace"));
        let err = cfg
            .finalize(&env, Path::new("/workspace"))
            .expect_err("invalid pattern");
        assert!(err.contains("invalid config: ignore pattern"));
    }

    #[test]
    fn finalize_missing_ignore_from_file_errors() {
        let mut cfg = YamlLintConfig::default();
        cfg.ignore_from_files.push("missing.txt".to_string());
        let env = StubEnv::new().with_cwd(PathBuf::from("/workspace"));
        let err = cfg
            .finalize(&env, Path::new("/workspace"))
            .expect_err("missing file");
        assert!(err.contains("failed to read ignore-from-file"));
    }

    #[test]
    fn finalize_invalid_ignore_from_file_pattern_errors() {
        let mut cfg = YamlLintConfig::default();
        cfg.ignore_from_files.push("rules.ignore".to_string());
        let env = StubEnv::new()
            .with_cwd(PathBuf::from("/workspace"))
            .with_file(PathBuf::from("/workspace/rules.ignore"), "[\n");
        let err = cfg
            .finalize(&env, Path::new("/workspace"))
            .expect_err("invalid file pattern");
        assert!(err.contains("invalid config: ignore-from-file pattern"));
    }

    #[test]
    fn finalize_collects_patterns_from_files() {
        let mut cfg = YamlLintConfig::default();
        cfg.ignore_from_files.push(".ignore".to_string());
        let env = StubEnv::new()
            .with_cwd(PathBuf::from("/workspace"))
            .with_file(PathBuf::from("/workspace/.ignore"), "generated/**\n");
        cfg.finalize(&env, Path::new("/workspace"))
            .expect("finalize");
        assert!(cfg.ignore_patterns.contains(&"generated/**".to_string()));
    }

    #[test]
    fn ignore_block_string_parses() {
        let cfg = YamlLintConfig::from_yaml_str("ignore: |\n  docs/**\n  build/\n")
            .expect("block ignore parse");
        assert!(cfg.ignore_patterns.contains(&"docs/**".to_string()));
        assert!(cfg.ignore_patterns.contains(&"build/".to_string()));
    }

    #[test]
    fn finalize_absolute_ignore_from_file() {
        let mut cfg = YamlLintConfig::default();
        cfg.ignore_from_files.push("/workspace/.ignore".to_string());
        let env = StubEnv::new()
            .with_cwd(PathBuf::from("/workspace"))
            .with_file(PathBuf::from("/workspace/.ignore"), "\nabs/**\n");
        cfg.finalize(&env, Path::new("/workspace"))
            .expect("finalize");
        assert!(cfg.ignore_patterns.contains(&"abs/**".to_string()));
    }

    #[test]
    fn load_ignore_patterns_errors_on_non_string() {
        let nodes = YamlOwned::load_from_str("[1]").expect("load");
        let err = load_ignore_patterns(&nodes[0]).expect_err("should fail");
        assert!(err.contains("ignore should contain file patterns"));
    }

    #[test]
    fn load_ignore_from_files_errors_on_non_string() {
        let nodes = YamlOwned::load_from_str("[1]").expect("load");
        let err = load_ignore_from_files(&nodes[0]).expect_err("should fail");
        assert!(err.contains("ignore-from-file should contain"));
    }

    #[test]
    fn load_ignore_from_files_sequence_success() {
        let nodes = YamlOwned::load_from_str("['a', 'b']").expect("load");
        let files = load_ignore_from_files(&nodes[0]).expect("should parse");
        assert_eq!(files, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn find_project_config_stops_at_home_boundary() {
        let env = StubEnv::new()
            .with_cwd(PathBuf::from("/workspace"))
            .with_var("HOME", "userhome")
            .with_exists(PathBuf::from("/workspace/.yamllint"));
        let inputs = [PathBuf::from("/workspace/userhome/project/file.yaml")];
        assert!(find_project_config_core(&env, &inputs).is_none());
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
        let mut cfg = YamlLintConfig::from_yaml_str_with_env(data, Some(envx), Some(&base_dir))?;
        cfg.finalize(envx, &base_dir)?;
        return Ok(ConfigContext {
            config: cfg,
            base_dir,
            source: None,
        });
    }
    if let Some(ref file) = overrides.config_file {
        let base = file
            .parent()
            .map_or_else(|| envx.current_dir(), Path::to_path_buf);
        let data = envx.read_to_string(file)?;
        let mut cfg = YamlLintConfig::from_yaml_str_with_env(&data, Some(envx), Some(&base))?;
        cfg.finalize(envx, &base)?;
        return Ok(ConfigContext {
            config: cfg,
            base_dir: base,
            source: Some(file.clone()),
        });
    }
    if let Some((cfg_path, base_dir)) = find_project_config_core(envx, inputs) {
        let data = envx.read_to_string(&cfg_path)?;
        let mut cfg = YamlLintConfig::from_yaml_str_with_env(&data, Some(envx), Some(&base_dir))?;
        cfg.finalize(envx, &base_dir)?;
        return Ok(ConfigContext {
            config: cfg,
            base_dir,
            source: Some(cfg_path),
        });
    }
    if let Some(ctx) = try_env_config_core(envx)? {
        return Ok(ctx);
    }
    try_user_global_core(envx, envx.current_dir())?.map_or_else(
        || {
            let base_dir = envx.current_dir();
            let mut cfg = YamlLintConfig::from_yaml_str(conf::builtin("default").unwrap())
                .expect("builtin preset must parse");
            cfg.finalize(envx, &base_dir)?;
            Ok(ConfigContext {
                config: cfg,
                base_dir,
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
        let mut cfg = YamlLintConfig::from_yaml_str_with_env(&data, Some(envx), Some(&base_dir))?;
        cfg.finalize(envx, &base_dir)?;
        return Ok(ConfigContext {
            config: cfg,
            base_dir,
            source: Some(cfg_path),
        });
    }
    try_user_global_core(envx, start_dir.to_path_buf())?.map_or_else(
        || {
            let mut cfg = YamlLintConfig::from_yaml_str(conf::builtin("default").unwrap())
                .expect("builtin preset must parse");
            cfg.finalize(envx, &envx.current_dir())?;
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
    let base = p
        .parent()
        .map_or_else(|| envx.current_dir(), Path::to_path_buf);
    let mut cfg = YamlLintConfig::from_yaml_str_with_env(&data, Some(envx), Some(&base))?;
    cfg.finalize(envx, &base)?;
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
            let mut cfg =
                YamlLintConfig::from_yaml_str_with_env(&data, Some(envx), Some(&base_dir))?;
            cfg.finalize(envx, &base_dir)?;
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
