use ryl::config::YamlLintConfig;
use ryl::rules::braces::{Config as BracesConfig, Forbid};
use ryl::rules::brackets::Config as BracketsConfig;

struct ConfigSuite<C> {
    rule_name: &'static str,
    resolve: fn(&YamlLintConfig) -> C,
    forbid: fn(&C) -> Forbid,
    min_spaces_inside: fn(&C) -> i64,
    max_spaces_inside: fn(&C) -> i64,
    effective_min_empty: fn(&C) -> i64,
    effective_max_empty: fn(&C) -> i64,
}

fn parse_config(input: &str) -> YamlLintConfig {
    YamlLintConfig::from_yaml_str(input).expect("config should parse")
}

fn run_config_suite<C>(suite: ConfigSuite<C>) {
    let cfg = parse_config(&format!(
        "rules:\n  {}:\n    forbid: non-empty\n    min-spaces-inside: 1\n    max-spaces-inside: 2\n    min-spaces-inside-empty: 3\n    max-spaces-inside-empty: 4\n",
        suite.rule_name
    ));
    let rule_cfg = (suite.resolve)(&cfg);
    assert_eq!((suite.forbid)(&rule_cfg), Forbid::NonEmpty);
    assert_eq!((suite.min_spaces_inside)(&rule_cfg), 1);
    assert_eq!((suite.max_spaces_inside)(&rule_cfg), 2);
    assert_eq!((suite.effective_min_empty)(&rule_cfg), 3);
    assert_eq!((suite.effective_max_empty)(&rule_cfg), 4);

    let cfg = parse_config(&format!(
        "rules:\n  {}:\n    min-spaces-inside: 2\n    max-spaces-inside: 3\n",
        suite.rule_name
    ));
    let rule_cfg = (suite.resolve)(&cfg);
    assert_eq!((suite.effective_min_empty)(&rule_cfg), 2);
    assert_eq!((suite.effective_max_empty)(&rule_cfg), 3);

    let err = YamlLintConfig::from_yaml_str(&format!(
        "rules:\n  {}:\n    forbid: maybe\n",
        suite.rule_name
    ))
    .expect_err("config should fail");
    assert!(
        err.contains(&format!(
            "option \"forbid\" of \"{}\" should be bool or \"non-empty\"",
            suite.rule_name
        )),
        "unexpected error: {err}"
    );

    for option in [
        "min-spaces-inside",
        "max-spaces-inside",
        "min-spaces-inside-empty",
        "max-spaces-inside-empty",
    ] {
        let err = YamlLintConfig::from_yaml_str(&format!(
            "rules:\n  {}:\n    {}: foo\n",
            suite.rule_name, option
        ))
        .expect_err("config should fail");
        assert!(
            err.contains(&format!(
                "option \"{}\" of \"{}\" should be int",
                option, suite.rule_name
            )),
            "unexpected error: {err}"
        );
    }

    let err = YamlLintConfig::from_yaml_str(&format!(
        "rules:\n  {}:\n    unexpected-option: true\n",
        suite.rule_name
    ))
    .expect_err("config should fail");
    assert!(
        err.contains(&format!(
            "invalid config: unknown option \"unexpected-option\" for rule \"{}\"",
            suite.rule_name
        )),
        "unexpected error: {err}"
    );

    let err = YamlLintConfig::from_yaml_str(&format!(
        "rules:\n  {}:\n    1: true\n",
        suite.rule_name
    ))
    .expect_err("config should fail");
    assert!(
        err.contains(&format!(
            "invalid config: unknown option \"1\" for rule \"{}\"",
            suite.rule_name
        )),
        "unexpected error: {err}"
    );

    let cfg = parse_config(&format!(
        "rules:\n  {}:\n    forbid: false\n",
        suite.rule_name
    ));
    let rule_cfg = (suite.resolve)(&cfg);
    assert_eq!((suite.forbid)(&rule_cfg), Forbid::None);
}

#[test]
fn braces_config_suite() {
    run_config_suite(ConfigSuite {
        rule_name: "braces",
        resolve: BracesConfig::resolve,
        forbid: BracesConfig::forbid,
        min_spaces_inside: BracesConfig::min_spaces_inside,
        max_spaces_inside: BracesConfig::max_spaces_inside,
        effective_min_empty: BracesConfig::effective_min_empty,
        effective_max_empty: BracesConfig::effective_max_empty,
    });
}

#[test]
fn brackets_config_suite() {
    run_config_suite(ConfigSuite {
        rule_name: "brackets",
        resolve: BracketsConfig::resolve,
        forbid: BracketsConfig::forbid,
        min_spaces_inside: BracketsConfig::min_spaces_inside,
        max_spaces_inside: BracketsConfig::max_spaces_inside,
        effective_min_empty: BracketsConfig::effective_min_empty,
        effective_max_empty: BracketsConfig::effective_max_empty,
    });
}
