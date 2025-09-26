use ryl::config::YamlLintConfig;
use ryl::rules::key_ordering::{self, Config};

fn build_config(yaml: &str) -> Config {
    let cfg = YamlLintConfig::from_yaml_str(yaml).expect("config parses");
    Config::resolve(&cfg)
}

#[test]
fn reports_block_mapping_out_of_order() {
    let cfg = build_config("rules:\n  key-ordering: enable\n");
    let input = "---\nblock mapping:\n  secondkey: a\n  firstkey: b\n";
    let hits = key_ordering::check(input, &cfg);
    assert_eq!(hits.len(), 1, "expected single violation: {hits:?}");
    let hit = &hits[0];
    assert_eq!(hit.line, 4);
    assert_eq!(hit.column, 3);
    assert_eq!(hit.message, "wrong ordering of key \"firstkey\" in mapping");
}

#[test]
fn reports_flow_mapping_out_of_order() {
    let cfg = build_config("rules:\n  key-ordering: enable\n");
    let input = "---\nflow mapping:\n  {secondkey: a, firstkey: b}\n";
    let hits = key_ordering::check(input, &cfg);
    assert_eq!(hits.len(), 1, "expected single violation: {hits:?}");
    let hit = &hits[0];
    assert_eq!(hit.line, 3);
    assert_eq!(hit.column, 18);
    assert_eq!(hit.message, "wrong ordering of key \"firstkey\" in mapping");
}

#[test]
fn ordered_mapping_passes() {
    let cfg = build_config("rules:\n  key-ordering: enable\n");
    let input = "---\nfirst: 1\nsecond: 2\nthird: 3\n";
    let hits = key_ordering::check(input, &cfg);
    assert!(
        hits.is_empty(),
        "ordered mapping should be accepted: {hits:?}"
    );
}

#[test]
fn ignored_keys_are_not_enforced() {
    let cfg = build_config("rules:\n  key-ordering:\n    ignored-keys: [\"n(a|o)me\", \"^b\"]\n");
    let input = "---\na:\nb:\nc:\nname: ignored\nfirst-name: ignored\nnome: ignored\ngnomes: ignored\nd:\ne:\nboat: ignored\n.boat: ERROR\ncall: ERROR\nf:\ng:\n";
    let hits = key_ordering::check(input, &cfg);
    assert_eq!(
        hits.len(),
        2,
        "expected violations for keys outside ignore filters"
    );
    assert_eq!(hits[0].line, 12);
    assert_eq!(hits[0].column, 1);
    assert_eq!(
        hits[0].message,
        "wrong ordering of key \".boat\" in mapping"
    );
    assert_eq!(hits[1].line, 13);
    assert_eq!(hits[1].column, 1);
    assert_eq!(hits[1].message, "wrong ordering of key \"call\" in mapping");
}

#[test]
fn locale_enables_case_and_accent_friendly_ordering() {
    let ascii_cfg = build_config("rules:\n  key-ordering: enable\n");
    let locale_cfg = build_config("locale: en_US.UTF-8\nrules:\n  key-ordering: enable\n");

    let case_input = "---\nT-shirt: 1\nT-shirts: 2\nt-shirt: 3\nt-shirts: 4\n";
    let case_hits = key_ordering::check(case_input, &ascii_cfg);
    assert!(
        case_hits.is_empty(),
        "uppercase-first ordering should pass: {case_hits:?}"
    );

    let case_fail = "---\nt-shirt: 1\nT-shirt: 2\nt-shirts: 3\nT-shirts: 4\n";
    let ascii_fail = key_ordering::check(case_fail, &ascii_cfg);
    assert!(
        !ascii_fail.is_empty(),
        "expected ascii failure when lowercase precedes uppercase"
    );
    assert!(
        ascii_fail
            .iter()
            .any(|hit| hit.line == 3 && hit.message.contains("T-shirt")),
        "expected violation on uppercase key: {ascii_fail:?}"
    );

    let locale_pass = key_ordering::check(case_fail, &locale_cfg);
    assert!(
        locale_pass.is_empty(),
        "locale-aware comparison should accept case-insensitive order: {locale_pass:?}"
    );

    let accent_fail = "---\nhaïr: true\nhais: true\n";
    let ascii_accent = key_ordering::check(accent_fail, &ascii_cfg);
    assert_eq!(ascii_accent.len(), 1, "expected accent-sensitive failure");
    assert_eq!(ascii_accent[0].line, 3);
    assert_eq!(ascii_accent[0].column, 1);

    let accent_pass = "---\nhair: true\nhaïr: true\nhais: true\nhaïssable: true\n";
    let locale_accent = key_ordering::check(accent_pass, &locale_cfg);
    assert!(
        locale_accent.is_empty(),
        "locale-aware comparison should accept accent-friendly order: {locale_accent:?}"
    );
}

#[test]
fn ignored_keys_accepts_scalar_configuration() {
    let raw =
        YamlLintConfig::from_yaml_str("rules:\n  key-ordering:\n    ignored-keys: \"name\"\n")
            .expect("config parses");
    let option = raw
        .rule_option(key_ordering::ID, "ignored-keys")
        .expect("option present");
    assert!(option.as_str().is_some(), "ignored-keys should be scalar");
    let cfg = key_ordering::Config::resolve(&raw);
    let hits = key_ordering::check("name: 1\nalpha: 2\n", &cfg);
    assert!(
        hits.is_empty(),
        "scalar ignored key should be skipped: {hits:?}"
    );
}

#[test]
fn c_locale_devolves_to_codepoint_ordering() {
    let raw = YamlLintConfig::from_yaml_str("locale: C.UTF-8\nrules:\n  key-ordering: enable\n")
        .expect("config parses");
    assert_eq!(raw.locale(), Some("C.UTF-8"));
    let cfg = key_ordering::Config::resolve(&raw);
    let hits = key_ordering::check("t-shirt: 1\nT-shirt: 2\n", &cfg);
    assert!(
        hits.iter().any(|hit| hit.message.contains("T-shirt")),
        "C locale should behave like default ordering: {hits:?}"
    );
}

#[test]
fn sequence_elements_are_checked_inside_lists() {
    let cfg = build_config("rules:\n  key-ordering: enable\n");
    let input = "---\n- second: 1\n  first: 0\n";
    let hits = key_ordering::check(input, &cfg);
    assert_eq!(
        hits.len(),
        1,
        "expected violation inside sequence: {hits:?}"
    );
    assert_eq!(hits[0].line, 3);
    assert_eq!(hits[0].column, 3);
}

#[test]
fn complex_sequence_keys_do_not_break_tracking() {
    let cfg = build_config("rules:\n  key-ordering: enable\n");
    let input = "---\n? [b, a]\n: value\n? {c: 1, d: 2}\n: other\n";
    let hits = key_ordering::check(input, &cfg);
    assert!(
        hits.is_empty(),
        "complex keys should not produce diagnostics: {hits:?}"
    );
}
