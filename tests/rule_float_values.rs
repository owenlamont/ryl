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
