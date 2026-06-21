use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use ryl::config::{
    Env, Overrides, SystemEnv, discover_config_with, discover_per_file_with,
};
use tempfile::tempdir;

#[derive(Default)]
struct FakeEnv {
    cwd: PathBuf,
    cfg_dir: Option<PathBuf>,
    files: HashMap<PathBuf, String>,
    exists: HashSet<PathBuf>,
    vars: HashMap<String, String>,
    home: Option<PathBuf>,
}

impl FakeEnv {
    fn with_cwd(mut self, p: impl Into<PathBuf>) -> Self {
        self.cwd = p.into();
        self
    }
    fn with_config_dir(mut self, p: impl Into<PathBuf>) -> Self {
        self.cfg_dir = Some(p.into());
        self
    }
    fn add_file(mut self, p: impl Into<PathBuf>, content: impl Into<String>) -> Self {
        self.files.insert(p.into(), content.into());
        self
    }
    fn add_exist(mut self, p: impl Into<PathBuf>) -> Self {
        self.exists.insert(p.into());
        self
    }
    fn set_var(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.vars.insert(k.into(), v.into());
        self
    }
    fn with_home(mut self, p: impl Into<PathBuf>) -> Self {
        self.home = Some(p.into());
        self
    }
}

impl Env for FakeEnv {
    fn current_dir(&self) -> PathBuf {
        self.cwd.clone()
    }
    fn config_dir(&self) -> Option<PathBuf> {
        self.cfg_dir.clone()
    }
    fn read_to_string(&self, p: &Path) -> Result<String, String> {
        self.files.get(p).cloned().ok_or_else(|| {
            format!("failed to read config file {}: not found", p.display())
        })
    }
    fn path_exists(&self, p: &Path) -> bool {
        self.files.contains_key(p) || self.exists.contains(p)
    }
    fn env_var(&self, key: &str) -> Option<String> {
        self.vars.get(key).cloned()
    }
    fn home_dir(&self) -> Option<PathBuf> {
        self.home
            .clone()
            .or_else(|| self.vars.get("HOME").map(PathBuf::from))
            .or_else(|| self.vars.get("USERPROFILE").map(PathBuf::from))
    }
}

fn cfg_rules_empty() -> String {
    "rules: {}\n".to_string()
}

#[test]
fn shim_inline_config_path() {
    let env = FakeEnv::default().with_cwd("/home/user");
    let ctx = discover_config_with(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(cfg_rules_empty()),
        },
        &env,
    )
    .unwrap();
    assert!(ctx.config.rule_names().is_empty());
    assert_eq!(ctx.base_dir, PathBuf::from("/home/user"));
    assert!(ctx.source.is_none());
}

#[test]
fn shim_file_config_path_with_parent_none_uses_cwd() {
    let env = FakeEnv::default()
        .with_cwd("/work")
        .add_file("", cfg_rules_empty());
    let ctx = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(PathBuf::from("")),
            config_data: None,
        },
        &env,
    )
    .unwrap();
    assert_eq!(ctx.base_dir, PathBuf::from("/work"));
    assert_eq!(ctx.source.unwrap(), PathBuf::from(""));
}

#[test]
fn shim_env_var_points_to_file_base_dir_from_cwd() {
    let env = FakeEnv::default()
        .with_cwd("/tmp/cwd")
        .add_file("", cfg_rules_empty())
        .set_var("YAMLLINT_CONFIG_FILE", "");
    let ctx = discover_config_with(&[], &Overrides::default(), &env).unwrap();
    assert_eq!(ctx.base_dir, PathBuf::from("/tmp/cwd"));
    assert_eq!(ctx.source.unwrap(), PathBuf::from(""));
}

#[test]
fn shim_env_var_with_tilde_expands_home() {
    let env = FakeEnv::default()
        .with_cwd("/tmp/cwd")
        .with_home("/home/tester")
        .set_var("YAMLLINT_CONFIG_FILE", "~/.config/yamllint/custom.yml")
        .add_file(
            "/home/tester/.config/yamllint/custom.yml",
            cfg_rules_empty(),
        );
    let ctx = discover_config_with(&[], &Overrides::default(), &env).unwrap();
    assert_eq!(
        ctx.source.unwrap(),
        PathBuf::from("/home/tester/.config/yamllint/custom.yml")
    );
}

#[test]
fn shim_env_var_tilde_alone_uses_home_directory() {
    let env = FakeEnv::default()
        .with_cwd("/tmp/cwd")
        .with_home("/home/tester")
        .set_var("YAMLLINT_CONFIG_FILE", "~")
        .add_file("/home/tester", cfg_rules_empty());
    let ctx = discover_config_with(&[], &Overrides::default(), &env).unwrap();
    assert_eq!(ctx.source.unwrap(), PathBuf::from("/home/tester"));
}

#[test]
fn shim_env_var_tilde_without_home_keeps_literal_path() {
    let env = FakeEnv::default()
        .with_cwd("/tmp/cwd")
        .set_var("YAMLLINT_CONFIG_FILE", "~/.config/yamllint/custom.yml")
        .add_file("~/.config/yamllint/custom.yml", cfg_rules_empty());
    let ctx = discover_config_with(&[], &Overrides::default(), &env).unwrap();
    assert_eq!(
        ctx.source.unwrap(),
        PathBuf::from("~/.config/yamllint/custom.yml")
    );
}

#[test]
fn shim_env_var_rejects_toml_target_pointing_at_native_config() {
    // YAMLLINT_CONFIG_FILE is yamllint's env var (YAML configs only). A `.toml` target
    // used to load as a ryl-native config; it must now error and steer the user to
    // -c / project discovery instead of silently loading TOML.
    let env = FakeEnv::default()
        .with_cwd("/tmp/cwd")
        .add_file("/proj/ryl.toml", "[rules]\nkey-duplicates = 'enable'\n")
        .set_var("YAMLLINT_CONFIG_FILE", "/proj/ryl.toml");
    let err = discover_config_with(&[], &Overrides::default(), &env).unwrap_err();
    assert!(
        err.contains("YAMLLINT_CONFIG_FILE") && err.contains("--config-file"),
        "expected an actionable TOML-rejection error, got: {err}"
    );
}

#[test]
fn system_env_home_dir_accessible() {
    let env = SystemEnv;
    let _ = env.home_dir();
}

#[test]
fn shim_env_var_userprofile_handles_windows_tilde_backslash() {
    let env = FakeEnv::default()
        .with_cwd("/tmp/cwd")
        .set_var("USERPROFILE", "/profiles/user")
        .set_var("YAMLLINT_CONFIG_FILE", "~\\config.yml")
        .add_file("/profiles/user/config.yml", cfg_rules_empty());
    let ctx = discover_config_with(&[], &Overrides::default(), &env).unwrap();
    assert_eq!(
        ctx.source.unwrap(),
        PathBuf::from("/profiles/user/config.yml")
    );
}

#[test]
fn shim_project_config_discovered_from_inputs() {
    let env = FakeEnv::default()
        .with_cwd("/wd")
        .add_file("/proj/.yamllint", cfg_rules_empty())
        .add_exist("/proj/.yamllint")
        .add_exist("/proj/file.yaml");
    let ctx = discover_config_with(
        &[PathBuf::from("/proj/file.yaml")],
        &Overrides::default(),
        &env,
    )
    .unwrap();
    assert_eq!(ctx.base_dir, PathBuf::from("/proj"));
    assert!(ctx.source.unwrap().ends_with(".yamllint"));
}

#[test]
fn shim_yamllint_user_global_config_applies_when_no_project_or_env() {
    // The yamllint-compat path resolves via XDG_CONFIG_HOME (or ~/.config), matching
    // yamllint itself, not the platform-native config dir.
    let env = FakeEnv::default()
        .with_cwd("/wd")
        .set_var("XDG_CONFIG_HOME", "/xdg")
        .add_file("/xdg/yamllint/config", cfg_rules_empty())
        .add_exist("/xdg/yamllint/config");
    let ctx = discover_config_with(&[], &Overrides::default(), &env).unwrap();
    assert!(ctx.source.unwrap().ends_with("yamllint/config"));
}

#[test]
fn shim_ryl_user_global_toml_applies_when_no_project_or_env() {
    let env = FakeEnv::default()
        .with_cwd("/wd")
        .with_config_dir("/xdg")
        .add_file("/xdg/ryl/ryl.toml", "[rules]\nkey-duplicates = 'enable'\n")
        .add_exist("/xdg/ryl/ryl.toml");
    let ctx = discover_config_with(&[], &Overrides::default(), &env).unwrap();
    assert_eq!(ctx.source.unwrap(), PathBuf::from("/xdg/ryl/ryl.toml"));
    // Confirms the file was parsed as TOML (a `[rules]` table), not YAML.
    assert!(
        ctx.config
            .rule_names()
            .iter()
            .any(|r| r == "key-duplicates")
    );
}

#[test]
fn shim_ryl_user_global_prefers_dotfile_over_plain() {
    let env = FakeEnv::default()
        .with_cwd("/wd")
        .with_config_dir("/xdg")
        .add_file("/xdg/ryl/.ryl.toml", "[rules]\nkey-duplicates = 'enable'\n")
        .add_exist("/xdg/ryl/.ryl.toml")
        .add_file("/xdg/ryl/ryl.toml", "[rules]\ntrailing-spaces = 'enable'\n")
        .add_exist("/xdg/ryl/ryl.toml");
    let ctx = discover_config_with(&[], &Overrides::default(), &env).unwrap();
    assert_eq!(ctx.source.unwrap(), PathBuf::from("/xdg/ryl/.ryl.toml"));
    // The dotfile's own contents loaded (key-duplicates, not ryl.toml's trailing-spaces),
    // so the dotfile won by being read, not merely by appearing first in the path list.
    let rules = ctx.config.rule_names();
    assert!(
        rules.iter().any(|r| r == "key-duplicates")
            && !rules.iter().any(|r| r == "trailing-spaces"),
        "expected .ryl.toml contents to load, got {rules:?}"
    );
}

#[test]
fn shim_ryl_user_global_takes_precedence_over_yamllint() {
    let env = FakeEnv::default()
        .with_cwd("/wd")
        .with_config_dir("/xdg")
        .set_var("XDG_CONFIG_HOME", "/xdg")
        .add_file("/xdg/ryl/ryl.toml", "[rules]\nkey-duplicates = 'enable'\n")
        .add_exist("/xdg/ryl/ryl.toml")
        .add_file("/xdg/yamllint/config", cfg_rules_empty())
        .add_exist("/xdg/yamllint/config");
    let ctx = discover_config_with(&[], &Overrides::default(), &env).unwrap();
    assert_eq!(ctx.source.unwrap(), PathBuf::from("/xdg/ryl/ryl.toml"));
}

#[test]
fn shim_ryl_user_global_unreadable_config_errors() {
    // The ryl-native config exists but cannot be read: discovery must surface the error
    // rather than silently falling through to the yamllint path or the empty default.
    let env = FakeEnv::default()
        .with_cwd("/wd")
        .with_config_dir("/xdg")
        .add_exist("/xdg/ryl/ryl.toml");
    let err = discover_config_with(&[], &Overrides::default(), &env).unwrap_err();
    assert!(err.contains("failed to read"), "unexpected error: {err}");
}

#[test]
fn shim_yamllint_user_global_falls_back_to_home_config_without_xdg() {
    // With no XDG_CONFIG_HOME, yamllint's user-global lives at ~/.config/yamllint/config
    // on every platform (not the native config dir); config_dir is unset here, which also
    // exercises the ryl-native path returning None before the yamllint fallback.
    let env = FakeEnv::default()
        .with_cwd("/wd")
        .with_home("/home/tester")
        .add_file("/home/tester/.config/yamllint/config", cfg_rules_empty())
        .add_exist("/home/tester/.config/yamllint/config");
    let ctx = discover_config_with(&[], &Overrides::default(), &env).unwrap();
    assert_eq!(
        ctx.source.unwrap(),
        PathBuf::from("/home/tester/.config/yamllint/config")
    );
}

#[test]
fn shim_user_global_missing_falls_back_to_default() {
    let env = FakeEnv::default()
        .with_cwd("/wd")
        .with_config_dir("/xdg-none");
    let ctx = discover_config_with(&[], &Overrides::default(), &env).unwrap();
    assert!(ctx.source.is_none());
    // default preset filters by extension
    assert!(
        ctx.config
            .is_yaml_candidate(&PathBuf::from("x.yaml"), &ctx.base_dir)
    );
}

#[test]
fn shim_systemenv_read_error_is_mapped() {
    let env = SystemEnv;
    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(PathBuf::from("no_such_file.yml")),
            config_data: None,
        },
        &env,
    )
    .unwrap_err();
    assert!(err.contains("failed to read config file"));
}

#[test]
fn shim_systemenv_read_success_is_used() {
    let td = tempfile::tempdir().unwrap();
    let cfgp = td.path().join("ok.yml");
    std::fs::write(&cfgp, "rules: {}\n").unwrap();
    let env = SystemEnv;
    let ctx = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfgp.clone()),
            config_data: None,
        },
        &env,
    )
    .unwrap();
    assert!(ctx.source.unwrap().ends_with("ok.yml"));
}

#[test]
fn system_env_read_invalid_encoding_reports_error() {
    let td = tempdir().unwrap();
    let path = td.path().join("bad.yml");
    std::fs::write(&path, [0xFFu8, 0xFE, 0x00]).unwrap();
    let env = SystemEnv;
    let err = env.read_to_string(&path).unwrap_err();
    assert!(err.contains("invalid"));
}

#[test]
fn shim_env_var_tilde_slash_only_returns_home() {
    let env = FakeEnv::default()
        .with_cwd("/tmp/cwd")
        .set_var("HOME", "/home/tester")
        .set_var("YAMLLINT_CONFIG_FILE", "~/")
        .add_file("/home/tester", cfg_rules_empty());
    let ctx = discover_config_with(&[], &Overrides::default(), &env).unwrap();
    assert_eq!(ctx.source.unwrap(), PathBuf::from("/home/tester"));
}

#[test]
fn shim_env_var_tilde_backslash_only_returns_home() {
    let env = FakeEnv::default()
        .with_cwd("/tmp/cwd")
        .set_var("USERPROFILE", "/profiles/user")
        .set_var("YAMLLINT_CONFIG_FILE", "~\\")
        .add_file("/profiles/user", cfg_rules_empty());
    let ctx = discover_config_with(&[], &Overrides::default(), &env).unwrap();
    assert_eq!(ctx.source.unwrap(), PathBuf::from("/profiles/user"));
}

#[test]
fn shim_discover_per_file_uses_project_else_user_global_else_default() {
    let env = FakeEnv::default()
        .with_cwd("/wd")
        .set_var("XDG_CONFIG_HOME", "/xdg")
        .add_file("/xdg/yamllint/config", cfg_rules_empty())
        .add_exist("/xdg/yamllint/config");
    // No project config, so user-global applies
    let file = PathBuf::from("/proj/no_config/file.yaml");
    let ctx = discover_per_file_with(&file, &env).unwrap();
    assert!(ctx.source.unwrap().ends_with("yamllint/config"));
}
