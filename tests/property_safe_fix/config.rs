//! Named config matrix and shared parsing helpers for the safe-fix property
//! suite.

use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use ryl::config::{Overrides, YamlLintConfig, discover_config};
use ryl::lint::{LintProblem, lint_str};
use saphyr::{LoadableYamlNode, YamlOwned};
use tempfile::TempDir;

const COMMON_SAFE_FIX_RULES_YAML: &str = "rules:
  new-lines: enable
  comments: enable
  comments-indentation: enable
  commas: enable
  braces: enable
  brackets: enable
  new-line-at-end-of-file: enable
  trailing-spaces: enable
  document-start: enable
  document-end: enable
  empty-lines: enable
";

const QUOTED_STRINGS_VARIANTS: &[(&str, &str)] = &[
    ("yamllint-default", "  quoted-strings: enable\n"),
    (
        "best-practice",
        "  quoted-strings:
    quote-type: single
    required: only-when-needed
",
    ),
    (
        "strict-single",
        "  quoted-strings:
    quote-type: single
    required: true
",
    ),
    (
        "strict-double",
        "  quoted-strings:
    quote-type: double
    required: true
",
    ),
    (
        "consistent",
        "  quoted-strings:
    quote-type: consistent
    required: true
",
    ),
];

pub const SAFE_FIX_RULES: &[&str] = &[
    "new-lines",
    "comments",
    "comments-indentation",
    "commas",
    "braces",
    "brackets",
    "new-line-at-end-of-file",
    "quoted-strings",
    "trailing-spaces",
    "document-start",
    "document-end",
    "empty-lines",
];

const BEST_PRACTICE_TOML: &str = "[rules]
new-lines = 'enable'
comments = 'enable'
comments-indentation = 'enable'
commas = 'enable'
braces = 'enable'
brackets = 'enable'
new-line-at-end-of-file = 'enable'
trailing-spaces = 'enable'
document-start = 'enable'
document-end = 'enable'
empty-lines = 'enable'

[rules.quoted-strings]
quote-type = 'single'
required = 'only-when-needed'
allow-double-quotes-for-escaping = true
";

pub struct PreparedConfig {
    pub name: &'static str,
    pub cfg: YamlLintConfig,
    // Holds the tempdir containing the .ryl.toml that `discover_config` was
    // given; kept alive so the path embedded in `cfg` (used by per-file
    // ignore matching) stays valid for the lifetime of the LazyLock.
    _backing: Option<TempDir>,
}

static SAFE_FIX_CONFIGS: LazyLock<Vec<PreparedConfig>> = LazyLock::new(|| {
    let mut configs: Vec<PreparedConfig> = QUOTED_STRINGS_VARIANTS
        .iter()
        .map(|(name, suffix)| {
            let yaml = format!("{COMMON_SAFE_FIX_RULES_YAML}{suffix}");
            let cfg = YamlLintConfig::from_yaml_str(&yaml)
                .expect("named safe-fix config must parse");
            PreparedConfig {
                name,
                cfg,
                _backing: None,
            }
        })
        .collect();

    let dir = TempDir::new().expect("create tempdir for TOML config");
    let toml_path = dir.path().join(".ryl.toml");
    fs::write(&toml_path, BEST_PRACTICE_TOML).expect("write TOML config");
    let overrides = Overrides {
        config_file: Some(toml_path),
        config_data: None,
    };
    let ctx = discover_config(&[], &overrides)
        .expect("TOML-backed best-practice config must load");
    configs.push(PreparedConfig {
        name: "best-practice-toml",
        cfg: ctx.config,
        _backing: Some(dir),
    });

    configs
});

pub fn safe_fix_configs() -> &'static [PreparedConfig] {
    &SAFE_FIX_CONFIGS
}

pub fn named_config(name: &str) -> &'static YamlLintConfig {
    &safe_fix_configs()
        .iter()
        .find(|prepared| prepared.name == name)
        .unwrap_or_else(|| panic!("unknown safe-fix config '{name}'"))
        .cfg
}

pub fn synthetic_path() -> &'static Path {
    Path::new("synthetic.yaml")
}

pub fn synthetic_base_dir() -> &'static Path {
    Path::new(".")
}

pub fn safe_fix_rule_diagnostics(
    content: &str,
    cfg: &YamlLintConfig,
) -> Vec<LintProblem> {
    lint_str(content, synthetic_path(), cfg, synthetic_base_dir())
        .into_iter()
        .filter(|diag| {
            diag.rule
                .map(|rule| SAFE_FIX_RULES.contains(&rule))
                .unwrap_or(false)
        })
        .collect()
}

pub fn parse_for_compare(content: &str) -> Option<Vec<YamlOwned>> {
    YamlOwned::load_from_str(content).ok()
}
