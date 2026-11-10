use std::path::Path;

use ryl::config::YamlLintConfig;
use ryl::format::{conflicts, format_str};
use ryl::lint::lint_str;

use crate::common::cli::ryl_on;
use crate::property::reference::check_values;

const REQUIRED: &str = "[lint.rules.quoted-strings]\nrequired = true\n";

fn formatted(input: &str, config: &str, expected: &str) {
    let cfg = YamlLintConfig::from_toml_str(config).unwrap();
    let output = format_str(input, &cfg, Path::new("a.yaml"), &[]);
    assert_eq!(output, expected);
    check_values(input, &output).unwrap();
    assert_eq!(format_str(&output, &cfg, Path::new("a.yaml"), &[]), output);
    assert!(lint_str(&output, Path::new("a.yaml"), &cfg, Path::new("")).is_empty());
}

#[test]
fn extra_required_quotes_flow_url() {
    for required in ["false", "'only-when-needed'"] {
        formatted(
            "args: [/health?ready=true]\n",
            &format!(
                "[lint.rules.quoted-strings]\nrequired = {required}\nextra-required = ['\\?']\n"
            ),
            "args: ['/health?ready=true']\n",
        );
    }
}

#[test]
fn required_quotes_values_not_keys() {
    formatted(
        "key: value\n'other': text\n",
        REQUIRED,
        "key: 'value'\nother: 'text'\n",
    );
}

#[test]
fn check_keys_quotes_keys() {
    formatted(
        "key: value\n",
        &format!("{REQUIRED}check-keys = true\n"),
        "'key': 'value'\n",
    );
}

#[test]
fn typed_scalars_stay_plain() {
    formatted(
        "a: [true, 8080, null, 011, .inf, on, 2026-01-01, text]\n",
        REQUIRED,
        "a: [true, 8080, null, 011, .inf, on, 2026-01-01, 'text']\n",
    );
    formatted(
        "a: [true, 8080, null, 011, .inf, on, 2026-01-01, text]\n",
        &format!("target-version = '1.2'\n{REQUIRED}"),
        "a: [true, 8080, null, 011, .inf, 'on', '2026-01-01', 'text']\n",
    );
}

#[test]
fn directives_override_target() {
    formatted(
        "%YAML 1.1\n---\na: [on, 2026-01-01, text]\n",
        &format!("target-version = '1.2'\n{REQUIRED}"),
        "%YAML 1.1\n---\na: [on, 2026-01-01, 'text']\n",
    );
    formatted(
        "%YAML 1.2\n---\na: [on, 2026-01-01, text]\n",
        REQUIRED,
        "%YAML 1.2\n---\na: ['on', '2026-01-01', 'text']\n",
    );
}

#[test]
fn extra_allowed_retains_without_adding() {
    formatted(
        "a: 'keep'\nb: keep\nc: 'drop'\n",
        "[lint.rules.quoted-strings]\nrequired = 'only-when-needed'\nextra-allowed = ['^keep$']\n",
        "a: 'keep'\nb: keep\nc: drop\n",
    );
}

#[test]
fn wrapped_sequence_quotes_survive_dash_spacing() {
    for style in ["single", "double"] {
        let quote = if style == "single" { '\'' } else { '"' };
        formatted(
            "a:\n-  first\n   second\n-   third\n    fourth\n",
            &format!("[format]\nquote-style = '{style}'\n{REQUIRED}"),
            &format!(
                "a:\n  - {quote}first\n     second{quote}\n  - {quote}third\n      fourth{quote}\n"
            ),
        );
    }
}

#[test]
fn wrapped_final_quotes_survive_collection_conversion() {
    for style in ["single", "double"] {
        for newline in ["\n", "\r", "\r\n"] {
            let quote = if style == "single" { '\'' } else { '"' };
            formatted(
                &format!("b: [a]{newline}a: first{newline}  café"),
                &format!(
                    "[format]\nquote-style = '{style}'\ndocument-end = 'add'\nsequence-expand = 'always'\n{REQUIRED}"
                ),
                &format!(
                    "b:\n  [\n    {quote}a{quote},\n  ]\na: {quote}first\n  café{quote}\n...\n"
                ),
            );
        }
    }
}

#[test]
fn wrapped_plain_keeps_breaks_and_value() {
    for style in ["single", "double"] {
        for input in [
            "a: first word\n  second word\n",
            "a: first 'word' \\\"x\\\"\n\n  second ''word''\n",
            "a: ${{ first &&\n  second }}\n",
            "flow: [first\n  second]\n",
        ] {
            let cfg = format!("[format]\nquote-style = '{style}'\n{REQUIRED}");
            let config = YamlLintConfig::from_toml_str(&cfg).unwrap();
            let output = format_str(input, &config, Path::new("a.yaml"), &[]);
            assert_ne!(output, input);
            assert_eq!(input.lines().count(), output.lines().count(), "{output:?}");
            formatted(input, &cfg, &output);
        }
    }
    formatted(
        "a: first word\r\n  second word\r\n",
        &format!("[format]\nline-ending = 'cr-lf'\n{REQUIRED}"),
        "a: 'first word\r\n  second word'\r\n",
    );
}

#[test]
fn required_quotes_follow_prose_and_collection_targets() {
    for mode in ["always", "never", "preserve"] {
        let cfg =
            format!("line-length = 18\n[format]\nprose-wrap = '{mode}'\n{REQUIRED}");
        let config = YamlLintConfig::from_toml_str(&cfg).unwrap();
        let input = "a: first word\n  second word and third word\n";
        let output = format_str(input, &config, Path::new("a.yaml"), &[]);
        formatted(input, &cfg, &output);
    }
    formatted(
        "a:\n  - text\n",
        &format!("[format]\nsequence-style = 'flow'\n{REQUIRED}"),
        "a: ['text']\n",
    );
    formatted(
        "a: [text]\n",
        &format!("[format]\nsequence-style = 'block'\n{REQUIRED}"),
        "a:\n  - 'text'\n",
    );
}

#[test]
fn requirements_honor_suppression_and_tags() {
    formatted(
        "a: !!str text\nb: &x text\nc: *x\nd: !local text\ne: |\n  text\n",
        REQUIRED,
        "a: !!str text\nb: &x 'text'\nc: *x\nd: !local 'text'\ne: |\n  text\n",
    );
    let cfg = YamlLintConfig::from_toml_str(REQUIRED).unwrap();
    for input in [
        "a: text  # ryl disable-line rule:quoted-strings\n",
        "# ryl format-ignore\na: text\n",
        "# ryl disable-file\na: text\n",
    ] {
        assert_eq!(format_str(input, &cfg, Path::new("a.yaml"), &[]), input);
    }
}

#[test]
fn quote_conflicts_remain_specific() {
    for required in ["false", "'only-when-needed'"] {
        let cfg = YamlLintConfig::from_toml_str(&format!("[lint.rules.quoted-strings]\nrequired = {required}\nextra-required = ['secret']\nallow-double-quotes-for-escaping = true\n")).unwrap();
        assert!(conflicts(&cfg).is_empty());
    }
    let cfg = YamlLintConfig::from_toml_str(&format!(
        "[format]\nquote-style = 'double'\n{REQUIRED}quote-type = 'single'\n"
    ))
    .unwrap();
    let warnings = conflicts(&cfg);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("format.quote-style wins"));
    assert!(!warnings[0].contains("required = false"));
    let cfg = YamlLintConfig::from_toml_str("[format]\nkeep-quotes = true\n[lint.rules.quoted-strings]\nrequired = 'only-when-needed'\n").unwrap();
    assert!(conflicts(&cfg)[0].contains("format.keep-quotes wins"));
    let cfg = YamlLintConfig::from_toml_str(&format!(
        "[format]\nquote-style = 'preserve'\n{REQUIRED}"
    ))
    .unwrap();
    assert!(conflicts(&cfg).is_empty());
    assert_eq!(
        format_str("a: text\n", &cfg, Path::new("a.yaml"), &[]),
        "a: text\n"
    );
}

#[test]
fn long_implicit_key_quote_refused() {
    let cfg = format!("{REQUIRED}check-keys = true\n");
    for key in [
        "a".repeat(1023),
        format!("{}\\''", "a".repeat(1019)),
        format!("&a {}", "é".repeat(1020)),
    ] {
        let input = format!("{key}: text\n");
        let (code, _, stderr, output) =
            ryl_on(Some(&cfg), &input, &["format", "--no-cache"]);
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(output, format!("{key}: 'text'\n"));
        assert!(
            stderr.contains("implicit key would exceed 1024 characters"),
            "{stderr}"
        );
        check_values(&input, &output).unwrap();
    }
    let key = "é".repeat(1022);
    formatted(
        &format!("{key}: text\n"),
        &cfg,
        &format!("'{key}': 'text'\n"),
    );
    let key = "a".repeat(1023);
    formatted(
        &format!("? {key}\n: text\n"),
        &cfg,
        &format!("? '{key}'\n: 'text'\n"),
    );
}

#[test]
fn jinja_header_stays_skipped() {
    let input = "#!jinja|yaml\na: {{ value }}\n";
    let (code, _, _, output) = ryl_on(Some(REQUIRED), input, &["format", "--no-cache"]);
    assert_eq!(code, 2);
    assert_eq!(output, input);
}

#[test]
fn implicit_collection_keys_skip_required_quotes() {
    let config = format!("{REQUIRED}check-keys = true\n");
    for key in [
        format!("[{}]", "a".repeat(1021)),
        format!("[{}, b]", "a".repeat(1017)),
        format!("{{{}: b}}", "a".repeat(1017)),
    ] {
        let input = format!("{key}: 1\n");
        let (code, _, stderr, output) =
            ryl_on(Some(&config), &input, &["format", "--no-cache"]);
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(output, input);
        let (code, _, _, again) =
            ryl_on(Some(&config), &output, &["format", "--no-cache"]);
        assert_eq!(code, 0);
        assert_eq!(again, output);
    }
    for input in ["[a, b]: text\n", "{a: [b]}: text\n", "[[a], b]: text\n"] {
        let (_, _, stderr, output) =
            ryl_on(Some(&config), input, &["format", "--no-cache"]);
        assert_eq!(output, input.replace("text", "'text'"));
        assert!(!stderr.contains("implicit key would exceed"));
        check_values(input, &output).unwrap();
    }
    formatted("? [a, b]\n: text\n", &config, "? ['a', 'b']\n: 'text'\n");
}

#[test]
fn implicit_collection_key_quote_refusals_are_reported() {
    for (config, expected) in [
        (format!("{REQUIRED}check-keys = true\n"), "[a?b]: 'value'\n"),
        ("[lint.rules.quoted-strings]\nrequired = false\nextra-required-flow = ['\\?']\n".to_owned(), "[a?b]: value\n"),
    ] {
        let (_, _, stderr, output) = ryl_on(Some(&config), "[a?b]: value\n", &["format", "--no-cache"]);
        assert_eq!(output, expected);
        assert!(stderr.contains("cannot add quotes inside an implicit collection key"), "{stderr}");
    }
}

#[test]
fn formatter_requirements_ignore_lint_fix_filters() {
    formatted(
        "a: text\n",
        "[lint]\nfixable = []\nunfixable = ['quoted-strings']\n[lint.rules.quoted-strings]\nrequired = true\nignore = ['*.yaml']\n",
        "a: 'text'\n",
    );
}

#[test]
fn quote_retention_and_style_conflicts_are_both_named() {
    let cfg = YamlLintConfig::from_toml_str("[format]\nkeep-quotes = true\nquote-style = 'double'\n[lint.rules.quoted-strings]\nrequired = 'only-when-needed'\nquote-type = 'single'\n").unwrap();
    let warnings = conflicts(&cfg);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("format.keep-quotes wins"));
    assert!(warnings[0].contains("format.quote-style wins"));
}

#[test]
fn key_requirements_keep_alias_and_empty_keys_typed() {
    let cfg = format!("{REQUIRED}check-keys = true\n");
    formatted(
        "base: &a text\n*a : value\n: text\n",
        &cfg,
        "'base': &a 'text'\n*a : 'value'\n: 'text'\n",
    );
    formatted("text\n", &cfg, "'text'\n");
    formatted("{a}\n", &cfg, "{'a'}\n");
}

#[test]
fn required_key_quotes_respect_skipped_and_suppressed_spacing() {
    let config = format!("{REQUIRED}check-keys = true\n");
    let cfg = YamlLintConfig::from_toml_str(&config).unwrap();
    assert_eq!(
        format_str(
            "a: [one,two]\n",
            &cfg,
            Path::new("a.yaml"),
            &["colons", "commas", "braces"]
        ),
        "'a': ['one','two']\n"
    );
    assert_eq!(
        format_str(
            "[a,b]: text  # ryl disable-line rule:commas\n",
            &cfg,
            Path::new("a.yaml"),
            &[]
        ),
        "[a,b]: 'text'  # ryl disable-line rule:commas\n"
    );
    let input = "a: 'text'  # ryl disable-line rule:quoted-strings\n";
    let cfg = YamlLintConfig::from_toml_str("[lint.rules.quoted-strings]\nrequired = 'only-when-needed'\ncheck-keys = true\n").unwrap();
    assert_eq!(format_str(input, &cfg, Path::new("a.yaml"), &[]), input);
}

#[test]
fn double_quote_key_refusal_is_reported_in_previews() {
    let config =
        format!("[format]\nquote-style = 'double'\n{REQUIRED}check-keys = true\n");
    let key = "a".repeat(1023);
    let input = format!("{key}: 1\n");
    for flag in ["--check", "--diff"] {
        let (code, _, stderr, output) =
            ryl_on(Some(&config), &input, &["format", "--no-cache", flag]);
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(output, input);
        assert!(
            stderr.contains("implicit key would exceed 1024 characters"),
            "{stderr}"
        );
    }
}

#[test]
fn optional_escaped_quotes_warn_about_escape_avoidance() {
    let config = "[lint.rules.quoted-strings]\nrequired = 'only-when-needed'\n";
    let cfg = YamlLintConfig::from_toml_str(config).unwrap();
    let warnings = conflicts(&cfg);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("allow-double-quotes-for-escaping = false"));
    assert!(warnings[0].contains("format.quote-style wins"));
    assert!(!warnings[0].contains("format.keep-quotes wins"));
    formatted(
        "a: \"back\\\\slash\"\n",
        &format!("{config}allow-double-quotes-for-escaping = true\n"),
        "a: \"back\\\\slash\"\n",
    );
}

#[test]
fn retention_warning_does_not_blame_permitted_quote_styles() {
    let config = "[format]\nkeep-quotes = true\n[lint.rules.quoted-strings]\nrequired = 'only-when-needed'\nallow-double-quotes-for-escaping = true\n";
    let cfg = YamlLintConfig::from_toml_str(config).unwrap();
    let warnings = conflicts(&cfg);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("format.keep-quotes wins"));
    assert!(!warnings[0].contains("format.quote-style wins"));
    let cfg =
        YamlLintConfig::from_toml_str(&format!("{config}extra-allowed = ['.*']\n"))
            .unwrap();
    assert!(conflicts(&cfg).is_empty());
}

#[test]
fn adding_quotes_avoids_escapes_in_plain_source() {
    let config = format!("[format]\nquote-style = 'double'\n{REQUIRED}");
    formatted(
        "a: say \"hi\" \\ path\n",
        &config,
        "a: 'say \"hi\" \\ path'\n",
    );
    formatted(
        "a: say \"hi\" \\ path\n  tail\n",
        &config,
        "a: 'say \"hi\" \\ path\n  tail'\n",
    );
}

#[test]
fn consistent_quotes_warn_when_the_first_style_opposes_the_formatter() {
    for (style, first) in [("single", "\"it's: x\""), ("double", "'say \"hi\": x'")] {
        for required in ["true", "false", "'only-when-needed'"] {
            let cfg = YamlLintConfig::from_toml_str(&format!("[format]\nquote-style = '{style}'\nkeep-quotes = true\n[lint.rules.quoted-strings]\nquote-type = 'consistent'\nrequired = {required}\ncheck-keys = true\nallow-quoted-quotes = true\nallow-double-quotes-for-escaping = true\n")).unwrap();
            let input = format!("- {first}\n- 'a: b'\n");
            let output = format_str(&input, &cfg, Path::new("a.yaml"), &[]);
            check_values(&input, &output).unwrap();
            assert!(
                !lint_str(&output, Path::new("a.yaml"), &cfg, Path::new("")).is_empty()
            );
            let warnings = conflicts(&cfg);
            assert_eq!(warnings.len(), 1);
            assert!(warnings[0].contains("quote-type = \"consistent\""));
            assert!(warnings[0].contains("format.quote-style wins"));
        }
    }
}
