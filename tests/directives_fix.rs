//! Full-fidelity `--fix`: a directive that disables a rule must keep the fixer from
//! rewriting the lines it covers, for replace, insert, and delete style fixers alike.

use std::path::Path;

use ryl::config::YamlLintConfig;
use ryl::fix::apply_safe_fixes;

fn fix(input: &str, config_yaml: &str) -> String {
    let config = YamlLintConfig::from_yaml_str(config_yaml).expect("config parses");
    apply_safe_fixes(input, &config, Path::new("in.yaml"), Path::new("."))
}

#[test]
fn replace_fixer_skips_block_disabled_lines() {
    let input = "# ryl disable rule:trailing-spaces\na: 1   \n\
        # ryl enable rule:trailing-spaces\nb: 2   \n";
    assert_eq!(
        fix(input, "rules:\n  trailing-spaces: enable\n"),
        "# ryl disable rule:trailing-spaces\na: 1   \n\
         # ryl enable rule:trailing-spaces\nb: 2\n",
        "disabled line keeps its trailing spaces; enabled line is trimmed"
    );
}

#[test]
fn replace_fixer_skips_inline_disabled_line() {
    let input = "a: [1,2 ,3]  # ryl disable-line rule:commas\nb: [4,5 ,6]\n";
    assert_eq!(
        fix(input, "rules:\n  commas: enable\n"),
        "a: [1,2 ,3]  # ryl disable-line rule:commas\nb: [4, 5, 6]\n",
        "inline-disabled line is untouched; the next line is fixed"
    );
}

#[test]
fn insert_fixer_skips_disabled_document_start() {
    let disabled = "# ryl disable rule:document-start\na: 1\n";
    assert_eq!(
        fix(disabled, "rules:\n  document-start: enable\n"),
        disabled,
        "document-start insertion is suppressed when the rule is disabled"
    );
    assert_eq!(
        fix("a: 1\n", "rules:\n  document-start: enable\n"),
        "---\na: 1\n",
        "control: document-start inserts `---` without a directive"
    );
}

#[test]
fn tail_insert_fixer_skips_disabled_document_end() {
    let disabled = "a: 1\n# ryl disable rule:document-end\n";
    assert_eq!(
        fix(disabled, "rules:\n  document-end: enable\n"),
        disabled,
        "document-end's trailing insertion is suppressed when disabled"
    );
    assert_eq!(
        fix("a: 1\n", "rules:\n  document-end: enable\n"),
        "a: 1\n...\n",
        "control: document-end appends `...` without a directive"
    );
}

#[test]
fn delete_fixer_reverts_disabled_blank_lines() {
    let config =
        "rules:\n  empty-lines:\n    max: 1\n    max-start: 0\n    max-end: 0\n";
    let disabled = "a: 1\n# ryl disable rule:empty-lines\n\n\n";
    assert_eq!(
        fix(disabled, config),
        disabled,
        "blank lines under a block disable are preserved"
    );
    assert_eq!(
        fix("a: 1\n\n\n", config),
        "a: 1\n",
        "control: trailing blank lines are removed without a directive"
    );
}

#[test]
fn fixer_runs_normally_when_a_different_rule_is_disabled() {
    let input = "# ryl disable rule:colons\n[1,2]\n";
    assert_eq!(
        fix(input, "rules:\n  commas: enable\n"),
        "# ryl disable rule:colons\n[1, 2]\n",
        "a directive for an unrelated rule does not gate the commas fixer"
    );
}

#[test]
fn disable_file_makes_fix_a_noop() {
    let body = "a: 1   \nb: [1,2 ,3]\n";
    let config = "rules:\n  trailing-spaces: enable\n  commas: enable\n";
    assert_ne!(
        fix(body, config),
        body,
        "control: the body is fixable without a disable-file directive"
    );
    let input = format!("# ryl disable-file\n{body}");
    assert_eq!(
        fix(&input, config),
        input,
        "a first-line disable-file leaves the whole file untouched"
    );
}

#[test]
fn fix_is_idempotent_with_directives() {
    let input = "# ryl disable rule:trailing-spaces\na: 1   \n\
        # ryl enable rule:trailing-spaces\nb: 2   \n";
    let config = "rules:\n  trailing-spaces: enable\n";
    let once = fix(input, config);
    assert_eq!(fix(&once, config), once, "a second --fix changes nothing");
}
