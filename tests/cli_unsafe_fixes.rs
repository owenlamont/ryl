use std::fs;

use ryl::config::{SourceVersion, YamlLintConfig};
use tempfile::tempdir;

use crate::common;
use common::cli::{command_output, run, ryl};

const RULES: &str = "[lint.rules]\ntruthy = \"enable\"\noctal-values = \"enable\"\n\
                     schema-dependent-values = \"enable\"\n";

const DIVERGENT: &str = "a: yes\nb: 0777\nc: 22:22\nd: 2026-10-10\ne: 1_000\nf: 0o17\n\
                         g: 08\nh: y\ni: 1:30.5\non: 1\nj: -0777\n";

fn ryl_with(config: &str, content: &str, args: &[&str]) -> (i32, String, String) {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".ryl.toml"), config).unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, content).unwrap();
    let (code, stdout, stderr) = run(ryl(dir.path()).args(args).arg(&file));
    let output = command_output(&stdout, &stderr).to_string();
    (code, output, fs::read_to_string(&file).unwrap())
}

#[test]
fn the_nearest_config_sets_source_version_per_subtree() {
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("legacy")).unwrap();
    fs::write(dir.path().join(".ryl.toml"), RULES).unwrap();
    fs::write(
        dir.path().join("legacy/.ryl.toml"),
        format!("source-version = \"1.1\"\n{RULES}"),
    )
    .unwrap();
    let files = [dir.path().join("a.yaml"), dir.path().join("legacy/a.yaml")];
    for file in &files {
        fs::write(file, "a: yes\n").unwrap();
    }
    run(ryl(dir.path())
        .args(["check", "--fix", "--unsafe-fixes"])
        .args(&files));
    let fixed = files.map(|file| fs::read_to_string(file).unwrap());
    assert_eq!(fixed, ["a: \"yes\"\n", "a: true\n"]);
}

#[test]
fn source_1_2_quotes_strings_and_writes_core_integers_in_decimal() {
    let (_, output, fixed) = ryl_with(
        &format!("unsafe-fixes = true\n{RULES}"),
        DIVERGENT,
        &["check", "--fix"],
    );
    assert_eq!(
        fixed,
        "a: \"yes\"\nb: 777\nc: \"22:22\"\nd: \"2026-10-10\"\ne: \"1_000\"\nf: 15\ng: 8\n\
         h: \"y\"\ni: \"1:30.5\"\n\"on\": 1\nj: -777\n",
        "{output}"
    );
}

#[test]
fn source_1_1_writes_the_1_1_value_and_declines_where_1_1_readers_disagree() {
    let (code, output, fixed) = ryl_with(
        &format!("source-version = \"1.1\"\n{RULES}"),
        DIVERGENT,
        &["check", "--fix", "--unsafe-fixes"],
    );
    assert_eq!(
        fixed,
        "a: true\nb: 511\nc: 1342\nd: 2026-10-10\ne: 1000\nf: 0o17\ng: 08\nh: y\n\
         i: 90.5\ntrue: 1\nj: -511\n"
    );
    assert_eq!(code, 1, "{output}");
    assert!(
        output.contains("Found 11 problems (7 fixed, 4 remaining)."),
        "{output}"
    );
}

#[test]
fn a_float_no_shared_spelling_carries_is_declined() {
    let base_60_past_1e16 = "a: 1:00:00:00:00:00:00:00:00:00.5\n";
    let (_, output, fixed) = ryl_with(
        &format!("source-version = \"1.1\"\nunsafe-fixes = true\n{RULES}"),
        base_60_past_1e16,
        &["check", "--fix"],
    );
    assert_eq!(fixed, base_60_past_1e16, "{output}");
}

#[test]
fn a_plain_scalar_spanning_lines_is_declined() {
    let folded = "a: 2001-12-14\n  21:59:43.10\n";
    let (_, output, fixed) = ryl_with(
        &format!("unsafe-fixes = true\n{RULES}"),
        folded,
        &["check", "--fix"],
    );
    assert_eq!(fixed, folded);
    assert!(output.contains("timestamp in YAML 1.1  ("), "{output}");
}

#[test]
fn only_an_undeclared_document_under_target_1_1_is_fixed() {
    let content = "%YAML 1.2\n---\na: 0777\n...\n%YAML 1.1\n---\nb: yes\n";
    let (_, output, fixed) = ryl_with(
        &format!("unsafe-fixes = true\n{RULES}"),
        content,
        &["check", "--fix"],
    );
    assert_eq!(fixed, content, "{output}");
    let (_, output, fixed) = ryl_with(
        &format!("unsafe-fixes = true\ntarget-version = \"1.2\"\n{RULES}"),
        "a: 0777\n",
        &["check", "--fix"],
    );
    assert_eq!(fixed, "a: 0777\n", "{output}");
}

#[test]
fn messages_name_the_fix_only_once_the_config_enables_it() {
    let (_, output, _) = ryl_with(RULES, "a: yes\n", &["check"]);
    assert!(
        output.contains("truthy value should be one of [false, true]  (truthy)"),
        "{output}"
    );
    let (_, output, _) = ryl_with(RULES, "a: yes\n", &["check", "--unsafe-fixes"]);
    assert!(
        output.contains("; unsafe fix: yes -> \"yes\"  (truthy)"),
        "{output}"
    );
}

#[test]
fn check_hints_at_unsafe_fixes_unless_turned_off() {
    let hint = "3 unsafe fixes available with `--fix --unsafe-fixes`.";
    let content = "a: yes\nb: 0777\nc: 22:22\n";
    let (_, output, _) = ryl_with(RULES, content, &["check"]);
    assert!(output.contains(hint), "{output}");
    let (_, output, fixed) = ryl_with(RULES, content, &["check", "--fix"]);
    assert_eq!(fixed, content);
    assert!(output.contains(hint), "{output}");
    let (_, output, _) = ryl_with(RULES, "a: yes\n", &["check"]);
    assert!(
        output.contains("1 unsafe fix available with `--fix --unsafe-fixes`."),
        "{output}"
    );
    for (config, args) in [
        (RULES.to_owned(), &["check", "--format", "parsable"][..]),
        (format!("unsafe-fixes = false\n{RULES}"), &["check"]),
        (
            format!("unsafe-fixes = true\n{RULES}"),
            &["check", "--no-unsafe-fixes"],
        ),
        (RULES.to_owned(), &["check", "--unsafe-fixes"]),
    ] {
        let (_, output, _) = ryl_with(&config, content, args);
        assert!(
            !output.contains("available with"),
            "{config} {args:?}: {output}"
        );
    }
}

#[test]
fn extend_safe_fixes_promotes_one_rule_and_unfixable_blocks_it() {
    let content = "a: yes\nb: 0777\n";
    let promoted = format!("[lint]\nextend-safe-fixes = [\"octal-values\"]\n{RULES}");
    let (_, output, fixed) = ryl_with(&promoted, content, &["check", "--fix"]);
    assert_eq!(fixed, "a: yes\nb: 777\n", "{output}");
    let blocked =
        format!("unsafe-fixes = true\n[lint]\nunfixable = [\"octal-values\"]\n{RULES}");
    let (_, output, fixed) = ryl_with(&blocked, content, &["check", "--fix"]);
    assert_eq!(fixed, "a: \"yes\"\nb: 0777\n", "{output}");
    assert!(!output.contains("0777 -> 777"), "{output}");
    let hinted = blocked.replace("unsafe-fixes = true\n", "");
    let (_, output, _) = ryl_with(&hinted, content, &["check"]);
    assert!(
        output.contains("1 unsafe fix available"),
        "a blocked rule's fix is not advertised: {output}"
    );
}

#[test]
fn quotes_follow_quoted_strings_single_and_truthy_allowed_spellings() {
    let config = format!(
        "unsafe-fixes = true\n{RULES}quoted-strings = {{ level = \"error\", quote-type = \
         \"single\", required = \"only-when-needed\" }}\n"
    );
    let (_, output, fixed) = ryl_with(&config, "a: yes\n", &["check", "--fix"]);
    assert_eq!(fixed, "a: 'yes'\n", "{output}");
    let recased = "source-version = \"1.1\"\nunsafe-fixes = true\n[lint.rules]\n\
                   truthy = { level = \"error\", allowed-values = [\"True\", \"False\"] }\n";
    let (_, output, fixed) = ryl_with(recased, "a: off\n", &["check", "--fix"]);
    assert_eq!(fixed, "a: False\n", "{output}");
    let unspellable = recased.replace("\"True\", \"False\"", "\"on\"");
    let (_, output, fixed) = ryl_with(&unspellable, "a: off\n", &["check", "--fix"]);
    assert_eq!(fixed, "a: off\n", "no allowed boolean spelling: {output}");
}

#[test]
fn format_never_applies_an_unsafe_fix() {
    let (_, output, fixed) = ryl_with(
        &format!("unsafe-fixes = true\n{RULES}"),
        "a: yes\nb: 0777\n",
        &["format"],
    );
    assert_eq!(fixed, "a: yes\nb: 0777\n", "{output}");
}

#[test]
fn diff_previews_the_unsafe_fixes_it_would_apply() {
    let (code, output, fixed) =
        ryl_with(RULES, "a: yes\n", &["check", "--diff", "--unsafe-fixes"]);
    assert_eq!(code, 1);
    assert_eq!(fixed, "a: yes\n");
    assert!(output.contains("-a: yes\n+a: \"yes\"\n"), "{output}");
}

#[test]
fn effective_config_round_trips_the_unsafe_fix_settings() {
    let original = YamlLintConfig::from_toml_str(&format!(
        "source-version = \"1.1\"\nunsafe-fixes = false\n[lint]\n\
         fixable = [\"octal-values\", \"schema-dependent-values\"]\n\
         extend-safe-fixes = [\"truthy\", \"schema-dependent-values\"]\n{RULES}"
    ))
    .unwrap();
    let reloaded = YamlLintConfig::from_toml_str(&original.to_toml_string()).unwrap();
    assert_eq!(reloaded.source_version(), SourceVersion::Yaml11);
    assert!(!reloaded.hints_unsafe_fixes());
    assert!(reloaded.applies_unsafe_fix("schema-dependent-values"));
    assert!(
        !reloaded.applies_unsafe_fix("octal-values"),
        "fixable, not promoted"
    );
    assert!(
        !reloaded.applies_unsafe_fix("truthy"),
        "promoted, not fixable"
    );
    assert!(reloaded.fix().allows_rule("schema-dependent-values"));
    assert!(!reloaded.fix().allows_rule("truthy"));
    let error =
        YamlLintConfig::from_toml_str("[lint]\nextend-safe-fixes = [\"commas\"]\n")
            .unwrap_err();
    assert!(
        error.contains("commas"),
        "a rule with no unsafe fix: {error}"
    );
}
