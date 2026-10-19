use std::path::Path;

use ryl::config::YamlLintConfig;
use ryl::lint::lint_str;
use ryl::rules;

#[test]
fn shared_check_analysis_matches_relaxed_helpers_with_unicode() {
    let samples = [
        "café: { z :  Yes ,a: [ 1 ,2 ] }\n",
        "café: &a {x: 1}\ncopy: *a\nunused: &a 2\n",
        "café: *missing\na: True\n",
        "café: 1\ncafé: 2\n",
        "café: .5\noctal: 010\nempty:\nquoted: 'plain'\n",
        "café: !!omap []\nscalar: |\n  words  \n\n",
        "café: &a {x: 1}\ncopy: {<<: *a}\n",
        "%YAML 1.1\n---\ncafé: Yes\n...\n%YAML 1.2\n---\ncafé: True\n",
        "{café: &é 1, b: *é, c: &é 2, d: *absent}\n",
    ];
    for sample in samples {
        for ending in ["\n", "\r\n", "\r"] {
            let content = sample.replace('\n', ending);
            macro_rules! compare {
                ($rule:ident) => {{
                    let cfg = YamlLintConfig::from_toml_str(&format!(
                        "[lint.rules.{}]\n",
                        rules::$rule::ID
                    ))
                    .unwrap();
                    let expected: Vec<_> = rules::$rule::check(
                        &content,
                        &rules::$rule::Config::resolve(&cfg),
                    )
                    .into_iter()
                    .map(|hit| (hit.line, hit.column, hit.message))
                    .collect();
                    let actual: Vec<_> =
                        lint_str(&content, Path::new("in.yaml"), &cfg, Path::new("."))
                            .into_iter()
                            .filter(|hit| hit.rule == Some(rules::$rule::ID))
                            .map(|hit| (hit.line, hit.column, hit.message))
                            .collect();
                    assert_eq!(actual, expected, "{}: {content:?}", rules::$rule::ID);
                }};
            }
            compare!(anchors);
            compare!(braces);
            compare!(brackets);
            compare!(colons);
            compare!(commas);
            compare!(comments);
            compare!(document_start);
            compare!(document_end);
            compare!(empty_values);
            compare!(float_values);
            compare!(key_duplicates);
            compare!(octal_values);
            compare!(quoted_strings);
            compare!(tags);
            compare!(truthy);
        }
    }
}

#[test]
fn strict_syntax_errors_override_rules_and_disable_directives() {
    let cfg = YamlLintConfig::from_yaml_str("extends: default").unwrap();
    for content in [
        "# ryl disable\ncafé: [unfinished\n",
        "café: *missing\nb: [unfinished\n",
        "café: [first\nsecond]\n",
        "%YAML 2.0\n---\ncafé: True\n",
    ] {
        let problems = lint_str(content, Path::new("in.yaml"), &cfg, Path::new("."));
        assert_eq!(problems.len(), 1, "{content:?}: {problems:?}");
        assert_eq!(problems[0].rule, None);
        assert!(problems[0].message.starts_with("syntax error:"));
    }
}

#[test]
fn direct_helpers_keep_relaxed_input_diagnostics() {
    let cfg = YamlLintConfig::from_yaml_str("extends: default").unwrap();
    let content = "café: [True,\nFalse]\n";
    assert_eq!(
        rules::truthy::check(content, &rules::truthy::Config::resolve(&cfg)).len(),
        2
    );
    let problems = lint_str(content, Path::new("in.yaml"), &cfg, Path::new("."));
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].rule, None);
}
