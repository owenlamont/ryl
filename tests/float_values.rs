use ryl::config::YamlLintConfig;
use ryl::rules::float_values::{Config, check};

fn resolve_config(yaml: &str) -> Config {
    let cfg = YamlLintConfig::from_yaml_str(yaml).expect("valid yaml config");
    Config::resolve(&cfg)
}

#[test]
fn float_values_flags_special_variants() {
    let config = resolve_config(
        "---\nrules:\n  float-values:\n    forbid-nan: true\n    forbid-inf: true\n    forbid-scientific-notation: true\n    require-numeral-before-decimal: true\n",
    );

    let buffer = "a: .NaN\nb: -.INF\nc: +.inf\nd: 1e2\ne: +.5\n";
    let diagnostics = check(buffer, &config);
    let messages: Vec<_> = diagnostics.iter().map(|d| d.message.as_str()).collect();
    assert!(
        messages.contains(&"forbidden not a number value \".NaN\""),
        "messages: {:?}",
        messages
    );
    assert!(
        messages.contains(&"forbidden infinite value \"-.INF\""),
        "messages: {:?}",
        messages
    );
    assert!(
        messages.contains(&"forbidden infinite value \"+.inf\""),
        "messages: {:?}",
        messages
    );
    assert!(
        messages.contains(&"forbidden scientific notation \"1e2\""),
        "messages: {:?}",
        messages
    );
    assert!(
        messages.contains(&"forbidden decimal missing 0 prefix \"+.5\""),
        "messages: {:?}",
        messages
    );
}
