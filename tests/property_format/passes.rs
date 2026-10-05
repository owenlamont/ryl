//! The formatting passes the suite proves. Every row must satisfy every invariant in
//! `property_format.rs`; a new formatter pass joins the guarantee by adding a row here.

use std::fs;
use std::sync::LazyLock;

use ryl::config::{Overrides, YamlLintConfig, discover_config};
use ryl::fix::apply_safe_fixes;
use tempfile::TempDir;

use super::config::{QUOTED_STRINGS_VARIANTS, synthetic_base_dir, synthetic_path};

/// The format-owned rules with a safe fix today; the lint-owned `truthy` and
/// `key-ordering` fixes rewrite meaning-bearing text and stay out.
pub const FORMAT_OWNED_RULES: [&str; 12] = [
    "braces",
    "brackets",
    "commas",
    "comments",
    "comments-indentation",
    "document-end",
    "document-start",
    "empty-lines",
    "new-line-at-end-of-file",
    "new-lines",
    "quoted-strings",
    "trailing-spaces",
];

const TOML_LADDER: &str = "[lint.rules]
braces = 'enable'
brackets = 'enable'
commas = 'enable'
comments-indentation = 'enable'
document-end = 'enable'
document-start = 'enable'
empty-lines = 'enable'
new-line-at-end-of-file = 'enable'
new-lines = 'enable'
trailing-spaces = 'enable'

[lint.rules.comments]
min-spaces-from-content = 2
max-spaces-from-content = 2

[lint.rules.quoted-strings]
quote-type = 'single'
required = 'only-when-needed'
allow-double-quotes-for-escaping = true
";

pub struct FormatPass {
    pub name: String,
    pub format: Box<dyn Fn(&str) -> String + Send + Sync>,
}

fn fix_pass(name: String, cfg: YamlLintConfig) -> FormatPass {
    FormatPass {
        name,
        format: Box::new(move |input| {
            apply_safe_fixes(input, &cfg, synthetic_path(), synthetic_base_dir())
        }),
    }
}

pub fn yaml_rules_for(quoted_strings: &str) -> String {
    let others: String = FORMAT_OWNED_RULES
        .iter()
        .filter(|rule| **rule != "quoted-strings")
        .map(|rule| format!("  {rule}: enable\n"))
        .collect();
    format!("rules:\n{others}{quoted_strings}")
}

fn toml_ladder_pass() -> FormatPass {
    let dir = TempDir::new().expect("create tempdir for TOML config");
    let path = dir.path().join(".ryl.toml");
    fs::write(&path, TOML_LADDER).expect("write TOML config");
    let overrides = Overrides {
        config_file: Some(path),
        config_data: None,
    };
    let cfg = discover_config(&[], &overrides)
        .expect("TOML format config loads")
        .config;
    FormatPass {
        name: "fix/ladder-toml".to_string(),
        // The config keeps paths into the tempdir, so the closure owns it.
        format: Box::new(move |input| {
            let _backing = &dir;
            apply_safe_fixes(input, &cfg, synthetic_path(), synthetic_base_dir())
        }),
    }
}

static PASSES: LazyLock<Vec<FormatPass>> = LazyLock::new(|| {
    QUOTED_STRINGS_VARIANTS
        .iter()
        .map(|(name, quoted_strings)| {
            let cfg = YamlLintConfig::from_yaml_str(&yaml_rules_for(quoted_strings))
                .expect("format-owned YAML config parses");
            fix_pass(format!("fix/{name}"), cfg)
        })
        .chain([toml_ladder_pass()])
        .collect()
});

pub fn format_passes() -> &'static [FormatPass] {
    &PASSES
}

pub fn named_pass(name: &str) -> &'static FormatPass {
    format_passes()
        .iter()
        .find(|pass| pass.name == name)
        .unwrap_or_else(|| panic!("unknown format pass '{name}'"))
}
