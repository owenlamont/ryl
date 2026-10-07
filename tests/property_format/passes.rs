//! The formatting passes the suite proves. Every row must satisfy every invariant in
//! `property_format.rs`; a new formatter pass joins the guarantee by adding a row here.

use std::fs;
use std::sync::LazyLock;

use ryl::config::{Overrides, YamlLintConfig, discover_config};
use ryl::fix::apply_safe_fixes;
use ryl::format::format_str;
use tempfile::TempDir;

use super::config::{QUOTED_STRINGS_VARIANTS, synthetic_base_dir, synthetic_path};

/// The format-owned rules, each with a safe fix but `line-length`, which only `ryl format`
/// folds; the lint-owned `truthy` and `key-ordering` fixes rewrite meaning-bearing text
/// and stay out.
pub const FORMAT_OWNED_RULES: [&str; 15] = [
    "braces",
    "brackets",
    "colons",
    "commas",
    "comments",
    "comments-indentation",
    "document-end",
    "document-start",
    "empty-lines",
    "hyphens",
    "line-length",
    "new-line-at-end-of-file",
    "new-lines",
    "quoted-strings",
    "trailing-spaces",
];

const TOML_LADDER: &str = "[lint.rules]
braces = 'enable'
brackets = 'enable'
colons = 'enable'
commas = 'enable'
comments-indentation = 'enable'
document-end = 'enable'
document-start = 'enable'
empty-lines = 'enable'
hyphens = 'enable'
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

fn toml_pass(
    name: &str,
    toml: &str,
    rewrite: fn(&str, &YamlLintConfig) -> String,
) -> FormatPass {
    let dir = TempDir::new().expect("create tempdir for TOML config");
    let path = dir.path().join(".ryl.toml");
    fs::write(&path, toml).expect("write TOML config");
    let overrides = Overrides {
        config_file: Some(path),
        config_data: None,
    };
    let cfg = discover_config(&[], &overrides)
        .expect("TOML format config loads")
        .config;
    FormatPass {
        name: name.to_string(),
        // The config keeps paths into the tempdir, so the closure owns it.
        format: Box::new(move |input| {
            let _backing = &dir;
            rewrite(input, &cfg)
        }),
    }
}

fn fix(input: &str, cfg: &YamlLintConfig) -> String {
    apply_safe_fixes(input, cfg, synthetic_path(), synthetic_base_dir())
}

fn format(input: &str, cfg: &YamlLintConfig) -> String {
    format_str(input, cfg, synthetic_path(), &[])
}

/// The `(line-length, indent-width)` of each `format/fold-*` row.
pub const FOLD_TARGETS: [(u16, u8); 4] = [(12, 2), (1, 2), (12, 4), (12, 1)];

fn fold_toml(width: u16, indent: u8) -> String {
    format!(
        "line-length = {width}\nindent-width = {indent}\n[format]\nfold-long-lines = true\n"
    )
}

/// `input` after `ryl format`'s fold alone, with every other formatting rule skipped.
pub fn fold_alone(input: &str, width: u16, indent: u8) -> String {
    let cfg = YamlLintConfig::from_toml_str(&fold_toml(width, indent))
        .expect("fold config parses");
    let others: Vec<&str> = ryl::format::FORMAT_RULE_IDS
        .into_iter()
        .filter(|rule| *rule != "line-length")
        .collect();
    format_str(input, &cfg, synthetic_path(), &others)
}

static PASSES: LazyLock<Vec<FormatPass>> = LazyLock::new(|| {
    QUOTED_STRINGS_VARIANTS
        .iter()
        .map(|(name, quoted_strings)| {
            let cfg = YamlLintConfig::from_yaml_str(&yaml_rules_for(quoted_strings))
                .expect("format-owned YAML config parses");
            fix_pass(format!("fix/{name}"), cfg)
        })
        .chain([
            toml_pass("fix/ladder-toml", TOML_LADDER, fix),
            toml_pass("format/default", "[format]\n", format),
            toml_pass(
                "format/quote-double",
                "[format]\nquote-style = 'double'\n",
                format,
            ),
            toml_pass(
                "format/brace-spacing",
                "[format]\nbrace-spacing = true\n",
                format,
            ),
            toml_pass("format/preview", "[format]\npreview = true\n", format),
            toml_pass(
                "format/non-defaults",
                "[format]\nquote-style = 'preserve'\nline-ending = 'cr-lf'\n\
                 document-start = 'preserve'\ndocument-end = 'add'\nfold-long-lines = true\n",
                format,
            ),
            toml_pass("format/fold-narrow", &fold_toml(12, 2), format),
            toml_pass("format/fold-1", &fold_toml(1, 2), format),
            toml_pass("format/fold-indent-4", &fold_toml(12, 4), format),
            toml_pass("format/fold-indent-1", &fold_toml(12, 1), format),
        ])
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
