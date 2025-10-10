use ryl::config::YamlLintConfig;
use ryl::rules::float_values::{self, Config};

fn build_config(yaml: &str) -> Config {
    let cfg = YamlLintConfig::from_yaml_str(yaml).expect("config parses");
    Config::resolve(&cfg)
}

#[test]
fn flags_forbidden_float_variants() {
    let resolved = build_config(
        "rules:\n  float-values:\n    require-numeral-before-decimal: true\n    forbid-scientific-notation: true\n    forbid-nan: true\n    forbid-inf: true\n",
    );

    let hits = float_values::check("a: .5\nb: 1e2\nc: .nan\nd: .inf\n", &resolved);

    assert_eq!(hits.len(), 4, "all variants should be flagged");
    assert_eq!(hits[0].line, 1);
    assert_eq!(hits[0].column, 4);
    assert_eq!(hits[0].message, "forbidden decimal missing 0 prefix \".5\"");

    assert_eq!(hits[1].line, 2);
    assert_eq!(hits[1].column, 4);
    assert_eq!(hits[1].message, "forbidden scientific notation \"1e2\"");

    assert_eq!(hits[2].line, 3);
    assert_eq!(hits[2].column, 4);
    assert_eq!(hits[2].message, "forbidden not a number value \".nan\"");

    assert_eq!(hits[3].line, 4);
    assert_eq!(hits[3].column, 4);
    assert_eq!(hits[3].message, "forbidden infinite value \".inf\"");
}

#[test]
fn skips_quoted_and_tagged_values() {
    let resolved =
        build_config("rules:\n  float-values:\n    require-numeral-before-decimal: true\n");
    let hits = float_values::check("quoted: '.5'\ntagged: !!float .5\nplain: .5\n", &resolved);

    assert_eq!(
        hits.len(),
        1,
        "only plain scalar without tag should be flagged"
    );
    assert_eq!(hits[0].line, 3);
    assert_eq!(hits[0].column, 8);
    assert_eq!(hits[0].message, "forbidden decimal missing 0 prefix \".5\"");
}

#[test]
fn scientific_notation_edge_cases() {
    let resolved = build_config(
        "rules:\n  float-values:\n    forbid-scientific-notation: true\n    require-numeral-before-decimal: true\n",
    );

    let buffer = "\
scientific_lower: .5e+2
scientific_upper: -.5E-2
missing_mantissa: e3
invalid_mantissa: a.e2
invalid_fraction: 1.ae2
missing_digits_after_dot: .e2
bare_decimal: .
missing_exponent: .5e
signed_without_digits: .5e+
invalid_exponent_chars: 1e+Q
";
    let hits = float_values::check(buffer, &resolved);
    let messages: Vec<_> = hits.iter().map(|d| d.message.as_str()).collect();

    assert_eq!(
        hits.len(),
        4,
        "expected two diagnostics per scientific value"
    );
    assert!(
        messages.contains(&"forbidden scientific notation \".5e+2\""),
        "messages: {messages:?}"
    );
    assert!(
        messages.contains(&"forbidden decimal missing 0 prefix \".5e+2\""),
        "messages: {messages:?}"
    );
    assert!(
        messages.contains(&"forbidden scientific notation \"-.5E-2\""),
        "messages: {messages:?}"
    );
    assert!(
        messages.contains(&"forbidden decimal missing 0 prefix \"-.5E-2\""),
        "messages: {messages:?}"
    );
    let forbidden = [
        "\"a.e2\"",
        "\"1.ae2\"",
        "\".e2\"",
        "\".5e\"",
        "\".5e+\"",
        "\"1e+Q\"",
    ];
    assert!(
        messages
            .iter()
            .all(|m| forbidden.iter().all(|value| !m.contains(value))),
        "unexpected diagnostics: {messages:?}"
    );
}
