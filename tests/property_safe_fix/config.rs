//! Named config matrix and shared parsing helpers for the safe-fix property
//! suite.

use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use crate::common::yaml_canonical;
use ryl::config::{Overrides, YamlLintConfig, discover_config};
use ryl::lint::{LintProblem, lint_str};
use ryl::rules::yaml_directive;
use ryl::yaml_dom::YamlOwned;
use tempfile::TempDir;

use super::representation;
pub use yaml_canonical::parse_for_compare;

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
  key-ordering: enable
";

const TRUTHY_DEFAULT: &str = "  truthy: enable\n";

const SPACING_DEFAULT: &str = "  colons: enable\n  hyphens: enable\n";

/// Zero tolerance, where a lint fix stops at the grammar's one space, and every check
/// disabled, where it fixes nothing.
const SPACING_VARIANTS: &[(&str, &str)] = &[
    (
        "spacing-zero",
        "  colons:
    max-spaces-before: 0
    max-spaces-after: 0
  hyphens:
    max-spaces-after: 0
",
    ),
    (
        "spacing-disabled",
        "  colons:
    max-spaces-before: -1
    max-spaces-after: -1
  hyphens:
    max-spaces-after: -1
",
    ),
];

const TRUTHY_TITLE_CASE: &str = "  quoted-strings: enable
  truthy:
    allowed-values: ['True', 'False']
    check-keys: false
";

pub const QUOTED_STRINGS_VARIANTS: &[(&str, &str)] = &[
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
    "colons",
    "hyphens",
    "new-line-at-end-of-file",
    "quoted-strings",
    "trailing-spaces",
    "document-start",
    "document-end",
    "empty-lines",
    "truthy",
    "key-ordering",
    "yaml-directive",
];

const COMMON_SAFE_FIX_RULES_TOML: &str = "locale = 'en_US.UTF-8'

[rules]
new-lines = 'enable'
comments-indentation = 'enable'
commas = 'enable'
braces = 'enable'
brackets = 'enable'
colons = 'enable'
hyphens = 'enable'
new-line-at-end-of-file = 'enable'
trailing-spaces = 'enable'
document-start = 'enable'
document-end = 'enable'
empty-lines = 'enable'
truthy = 'enable'

[rules.key-ordering]
ignored-keys = ['^k']

[rules.quoted-strings]
quote-type = 'single'
required = 'only-when-needed'
allow-double-quotes-for-escaping = true
";

const COMMENTS_TOML_VARIANTS: &[(&str, &str)] = &[
    ("best-practice-toml", "[rules.comments]\n"),
    (
        "comments-exact-spacing-toml",
        "[rules.comments]\nmin-spaces-from-content = 2\nmax-spaces-from-content = 2\n",
    ),
    (
        "comments-spacing-band-toml",
        "[rules.comments]\nmin-spaces-from-content = 1\nmax-spaces-from-content = 4\n",
    ),
];

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
            (*name, format!("{suffix}{TRUTHY_DEFAULT}{SPACING_DEFAULT}"))
        })
        .chain([(
            "truthy-title-case",
            format!("{TRUTHY_TITLE_CASE}{SPACING_DEFAULT}"),
        )])
        .chain(SPACING_VARIANTS.iter().map(|(name, spacing)| {
            let quoted = QUOTED_STRINGS_VARIANTS[1].1;
            (*name, format!("{quoted}{TRUTHY_DEFAULT}{spacing}"))
        }))
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

    let target_1_2 = format!(
        "target-version = '1.2'\n{COMMON_SAFE_FIX_RULES_TOML}\n[rules.comments]\n\
         [rules.yaml-directive]\n"
    );
    let tomls = COMMENTS_TOML_VARIANTS
        .iter()
        .map(|(name, comments)| {
            (*name, format!("{COMMON_SAFE_FIX_RULES_TOML}\n{comments}"))
        })
        .chain([("target-1.2-toml", target_1_2)]);
    for (name, toml) in tomls {
        let dir = TempDir::new().expect("create tempdir for TOML config");
        let toml_path = dir.path().join(".ryl.toml");
        fs::write(&toml_path, toml).expect("write TOML config");
        let overrides = Overrides {
            config_file: Some(toml_path),
            config_data: None,
        };
        let ctx = discover_config(&[], &overrides)
            .expect("named TOML safe-fix config must load");
        configs.push(PreparedConfig {
            name,
            cfg: ctx.config,
            _backing: Some(dir),
        });
    }

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

/// The values a safe fix of `input` must load to: `input`'s own, after `yaml-directive`
/// carries any `%YAML 1.1` document over to 1.2, since the loader reads every document
/// as 1.2. `property_yaml_migration` holds the carry-over to the 1.1 values.
pub fn parse_for_compare_migrated(
    input: &str,
    cfg: &YamlLintConfig,
) -> Option<Vec<YamlOwned>> {
    let migrated = cfg.rule_level(yaml_directive::ID).and_then(|_| {
        yaml_directive::fix(input, &yaml_directive::Config::resolve(cfg), &[])
    });
    parse_for_compare(migrated.as_deref().unwrap_or(input))
}

pub fn reference_safe_eof_append(input: &str, fixed: &str) -> bool {
    if input.ends_with(['\r', '\n']) {
        return false;
    }
    let Some(before) = representation::representation(input) else {
        return false;
    };
    ["\n", "\r\n", "\r"].into_iter().any(|newline| {
        let terminated = format!("{input}{newline}");
        representation::representation(&terminated).as_ref() == Some(&before)
            && parse_for_compare(&terminated)
                .is_some_and(|value| Some(value) == parse_for_compare(fixed))
    })
}
