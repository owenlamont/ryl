//! Property tests for **configuration parsing** robustness (issue #246).
//!
//! The audit that motivated these turned up two config-path defects a property
//! test would have caught: a panic on an empty config (`docs[0]`) and an
//! out-of-memory on a billion-laughs config. This suite generates randomized
//! configs — mixing valid settings with hostile ones (invalid regexes, ill-typed
//! and out-of-range scalars, bogus locales) — renders them to both YAML and TOML,
//! and asserts the oracle-free invariant that the whole pipeline **errors or
//! succeeds but never panics**:
//!
//! - YAML configs are parsed with [`YamlLintConfig::from_yaml_str`] and, when they
//!   parse, used to lint sample documents. Config validation rejects hostile option
//!   values (invalid regexes, wrong types) before a rule's `resolve()` runs, so the
//!   `.expect()` calls in `key-ordering`/`quoted-strings` `resolve()` are not
//!   reachable from a config that parses — the value here is the guard against a
//!   future *validation gap*: were one introduced, a generated config would carry a
//!   bad value into `resolve()` and panic the property.
//! - TOML configs are pushed through `parse -> validate -> normalize`, the
//!   TOML-specific surface.
//!
//! Deterministic siblings pin the empty-config, invalid-regex, billion-laughs, and
//! valid-config cases so the random invariant cannot pass vacuously if the generator
//! drifts.

#[path = "property_config/strategy.rs"]
mod strategy;

use std::path::Path;

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use ryl::config::YamlLintConfig;
use ryl::config_schema::{
    normalize_toml_config, parse_toml_config_str, validate_toml_config,
};
use ryl::lint::lint_str;

use strategy::{arb_config, render_toml, render_yaml};

/// Documents crafted to trigger the rules whose `resolve()` compiles config
/// regexes or reads typed options, so a hostile option value reaches the rule.
const SAMPLE_DOCS: &[&str] = &[
    "b: 1\na: 2\n",
    "name: value\nok_key: 'x'\n",
    "list: [1,2 ,3]\n",
    "flag: yes\n",
];

fn lint_with(yaml_config: &str) -> Option<YamlLintConfig> {
    let cfg = YamlLintConfig::from_yaml_str(yaml_config).ok()?;
    for doc in SAMPLE_DOCS {
        // Panics here (e.g. a rule's resolve() `.expect()`) fail the property.
        let _ = lint_str(doc, Path::new("in.yaml"), &cfg, Path::new("."));
    }
    Some(cfg)
}

fn parse_toml_without_panicking(toml_config: &str) {
    if let Ok(Some(typed)) = parse_toml_config_str(toml_config, false)
        && validate_toml_config(&typed).is_ok()
    {
        let _ = normalize_toml_config(&typed);
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/proptest-regressions/property_config.txt",
        ))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn config_parsing_and_linting_never_panics(model in arb_config()) {
        lint_with(&render_yaml(&model));
        parse_toml_without_panicking(&render_toml(&model));
    }
}

#[test]
fn empty_yaml_config_errors_without_panicking() {
    assert!(
        YamlLintConfig::from_yaml_str("").is_err(),
        "an empty YAML config is not a mapping and must error"
    );
}

#[test]
fn empty_toml_config_errors_without_panicking() {
    assert!(
        parse_toml_config_str("", false).is_err(),
        "an empty TOML config configures nothing and must error"
    );
}

#[test]
fn invalid_regex_config_errors_without_panicking() {
    let yaml = "rules:\n  key-ordering:\n    level: error\n    ignored-keys: [\"(\"]\n";
    assert!(
        YamlLintConfig::from_yaml_str(yaml).is_err(),
        "an invalid regex must be rejected at parse time, not panic in resolve()"
    );
}

#[test]
fn billion_laughs_config_errors_without_panicking() {
    let mut yaml = String::from("a0: &a0 \"x\"\n");
    for level in 1..=9 {
        let refs = vec![format!("*a{}", level - 1); 9].join(",");
        yaml.push_str(&format!("a{level}: &a{level} [{refs}]\n"));
    }
    yaml.push_str(&format!("boom: [{}]\n", ["*a9"; 9].join(",")));
    assert!(
        YamlLintConfig::from_yaml_str(&yaml).is_err(),
        "alias expansion must be capped and reported, not exhaust memory"
    );
}

#[test]
fn rich_valid_config_lints_without_panicking() {
    let yaml = "ignore:\n  - \"vendor/**\"\nlocale: \"en_US.UTF-8\"\nrules:\n  \
                key-ordering:\n    level: error\n    ignored-keys: [\"^ok$\"]\n  \
                quoted-strings:\n    required: false\n  trailing-spaces: enable\n";
    let cfg = lint_with(yaml);
    assert!(
        cfg.is_some(),
        "a well-formed config must parse and lint cleanly"
    );
}
