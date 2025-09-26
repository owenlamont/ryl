use ryl::config::YamlLintConfig;
use ryl::rules::octal_values::{self, Config};

fn build_config(yaml: &str) -> Config {
    let cfg = YamlLintConfig::from_yaml_str(yaml).expect("config parses");
    Config::resolve(&cfg)
}

#[test]
fn flags_plain_implicit_and_explicit_values() {
    let resolved = build_config("rules:\n  octal-values: enable\n");
    let hits = octal_values::check("foo: 010\nbar: 0o10\n", &resolved);
    assert_eq!(hits.len(), 2, "expected to flag both values");

    assert_eq!(hits[0].line, 1);
    assert_eq!(hits[0].column, 9);
    assert_eq!(hits[0].message, "forbidden implicit octal value \"010\"");

    assert_eq!(hits[1].line, 2);
    assert_eq!(hits[1].column, 10);
    assert_eq!(hits[1].message, "forbidden explicit octal value \"0o10\"");
}

#[test]
fn respects_forbid_implicit_override() {
    let resolved = build_config("rules:\n  octal-values:\n    forbid-implicit-octal: false\n");
    let hits = octal_values::check("foo: 010\nbar: 0o10\n", &resolved);
    assert_eq!(hits.len(), 1, "implicit octals should be allowed");
    assert_eq!(hits[0].message, "forbidden explicit octal value \"0o10\"");
}

#[test]
fn respects_forbid_explicit_override() {
    let resolved = build_config("rules:\n  octal-values:\n    forbid-explicit-octal: false\n");
    let hits = octal_values::check("foo: 010\nbar: 0o10\n", &resolved);
    assert_eq!(hits.len(), 1, "explicit octals should be allowed");
    assert_eq!(hits[0].message, "forbidden implicit octal value \"010\"");
}

#[test]
fn skips_quoted_and_tagged_values() {
    let resolved = build_config("rules:\n  octal-values: enable\n");
    let hits = octal_values::check(
        "quoted: '010'\ntagged: !!str 0o10\nflow: [010, '010']\n",
        &resolved,
    );
    assert_eq!(hits.len(), 1, "only plain implicit value should remain");
    assert_eq!(hits[0].line, 3);
    assert_eq!(hits[0].column, 11);
    assert_eq!(hits[0].message, "forbidden implicit octal value \"010\"");
}
