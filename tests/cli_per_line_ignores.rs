//! CLI coverage for the ryl-only `per-line-ignores` TOML config table: a regex/glob
//! that suppresses chosen rules on matching lines, implemented as virtual
//! `disable-line` directives so lint, `--fix`, and embedded markdown all honour it.
//! Assertions are format-agnostic (bare `line:col` + bare rule id) per the repo's
//! CI-output convention.

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use tempfile::tempdir;

mod common;
use common::cli::{command_output, run};

fn run_lint(config: &str, file_name: &str, body: &str) -> (i32, String) {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join(".ryl.toml");
    fs::write(&config_path, config).unwrap();
    let file = dir.path().join(file_name);
    fs::write(&file, body).unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config_path).arg(&file));
    (code, command_output(&stdout, &stderr).to_string())
}

#[test]
fn regex_suppresses_rule_only_on_matching_lines() {
    // `#cloud-config` keeps its exact spelling (no starting space), so without an
    // exemption `comments` flags it; a second `#bad` comment must still fire.
    let config = "[rules.comments]\n\n[[per-line-ignores]]\n\
                  regex = '^#cloud-config$'\nrules = [\"comments\"]\n";
    let (code, out) = run_lint(config, "doc.yaml", "#cloud-config\nkey: value  #bad\n");
    assert_eq!(code, 1, "second comment should still fire: {out}");
    assert!(
        out.contains("2:14") && out.contains("comments"),
        "line 2 comment should be reported: {out}"
    );
    assert!(
        !out.contains("1:2"),
        "the #cloud-config line must be suppressed: {out}"
    );
}

#[test]
fn path_and_regex_are_anded_to_scope_an_entry() {
    let dir = tempdir().unwrap();
    let config = dir.path().join(".ryl.toml");
    fs::write(
        &config,
        "[rules.comments]\n\n[[per-line-ignores]]\npath = \"*.tpl.yaml\"\n\
         regex = '^#cloud-config$'\nrules = [\"comments\"]\n",
    )
    .unwrap();
    fs::write(dir.path().join("a.tpl.yaml"), "#cloud-config\n").unwrap();
    fs::write(dir.path().join("b.yaml"), "#cloud-config\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(dir.path()));
    let out = command_output(&stdout, &stderr);
    assert_eq!(code, 1, "only the non-template file should fire: {out}");
    assert!(
        out.contains("b.yaml") && out.contains("comments"),
        "b.yaml is outside the path glob, so comments fires: {out}"
    );
    assert!(
        !out.contains("a.tpl.yaml"),
        "a.tpl.yaml matches path+regex, so it is suppressed: {out}"
    );
}

#[test]
fn negated_path_glob_applies_outside_the_pattern() {
    // `!src/**` negates like per-file-ignores: the entry applies to files NOT under
    // src/, so the outside file is suppressed and the inside file still fires.
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    fs::create_dir(&src).unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules.comments]\n\n[[per-line-ignores]]\npath = \"!src/**\"\n\
         regex = '^#cloud-config$'\nrules = [\"comments\"]\n",
    )
    .unwrap();
    fs::write(dir.path().join("outside.yaml"), "#cloud-config\n").unwrap();
    fs::write(src.join("inside.yaml"), "#cloud-config\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("-c")
        .arg(dir.path().join(".ryl.toml"))
        .arg(dir.path()));
    let out = command_output(&stdout, &stderr);
    assert_eq!(code, 1, "the in-src file should still fire: {out}");
    assert!(
        out.contains("inside.yaml") && out.contains("comments"),
        "src/inside.yaml is excluded by the negated glob, so comments fires: {out}"
    );
    assert!(
        !out.contains("outside.yaml"),
        "outside.yaml matches the negated glob, so it is suppressed: {out}"
    );
}

#[test]
fn path_only_entry_suppresses_a_rule_file_wide() {
    // No regex → the entry applies to every line of a matching file (recorded once as a
    // file-wide disable, not materialized per line). Multiple comment violations across
    // lines are all suppressed; a non-matching file still fires.
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules.comments]\n\n[[per-line-ignores]]\npath = \"gen-*.yaml\"\n\
         rules = [\"comments\"]\n",
    )
    .unwrap();
    fs::write(dir.path().join("gen-a.yaml"), "#one\nkey: value  #two\n").unwrap();
    fs::write(dir.path().join("normal.yaml"), "#three\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("-c")
        .arg(dir.path().join(".ryl.toml"))
        .arg(dir.path()));
    let out = command_output(&stdout, &stderr);
    assert_eq!(code, 1, "the non-matching file should still fire: {out}");
    assert!(
        out.contains("normal.yaml") && out.contains("comments"),
        "normal.yaml is outside the path glob, so comments fires: {out}"
    );
    assert!(
        !out.contains("gen-a.yaml"),
        "every comment in gen-a.yaml is suppressed file-wide: {out}"
    );
}

#[test]
fn all_selector_suppresses_every_rule_on_the_line() {
    let config = "[rules.comments]\n[rules.line-length]\nmax = 10\n\n\
                  [[per-line-ignores]]\nregex = 'GENERATED'\nrules = [\"ALL\"]\n";
    // Line 1 trips both comments and line-length but is fully exempt; line 2 stays.
    let body = "k: value  #GENERATED filler over the limit\nother: value-too-long\n";
    let (code, out) = run_lint(config, "doc.yaml", body);
    assert_eq!(code, 1, "line 2 should still fire: {out}");
    assert!(
        out.contains("2:11") && out.contains("line-length"),
        "line 2 line-length should be reported: {out}"
    );
    // Line 1 would trip line-length (1:11) and comments (1:12); ALL must suppress both.
    // Assert the full `line:col` (not a bare `1:`, which collides with the GitHub
    // format's `col=11::` rendering of the line-2 diagnostic).
    assert!(
        !out.contains("1:11") && !out.contains("1:12"),
        "every rule on the GENERATED line must be suppressed: {out}"
    );
}

#[test]
fn fix_leaves_suppressed_lines_byte_identical() {
    let dir = tempdir().unwrap();
    let config = dir.path().join(".ryl.toml");
    fs::write(
        &config,
        "[rules.comments]\n\n[[per-line-ignores]]\nregex = '^#cloud-config$'\n\
         rules = [\"comments\"]\n",
    )
    .unwrap();
    let file = dir.path().join("doc.yaml");
    fs::write(&file, "#cloud-config\nkey: value  #bad\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (_code, _stdout, _stderr) = run(Command::new(exe)
        .arg("--fix")
        .arg("-c")
        .arg(&config)
        .arg(&file));
    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(
        fixed, "#cloud-config\nkey: value  # bad\n",
        "the suppressed comment is untouched while the other is fixed"
    );
}

#[test]
fn fix_still_applies_when_a_per_line_entry_matches_no_current_line() {
    // The entry targets a fixable rule but its regex matches nothing in the input, so
    // reconciliation is engaged (a fixer could create a matching line) yet reverts
    // nothing: the trailing spaces are still fixed. Exercises the per-line `guarded`
    // path where no original line matched.
    let dir = tempdir().unwrap();
    let config = dir.path().join(".ryl.toml");
    fs::write(
        &config,
        "[rules.trailing-spaces]\n\n[[per-line-ignores]]\nregex = 'NEVER_MATCHES'\n\
         rules = [\"trailing-spaces\"]\n",
    )
    .unwrap();
    let file = dir.path().join("doc.yaml");
    fs::write(&file, "key: value   \n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let _ = run(Command::new(exe)
        .arg("--fix")
        .arg("-c")
        .arg(&config)
        .arg(&file));
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "key: value\n",
        "a non-matching per-line entry must not block fixing the rule elsewhere"
    );
}

#[test]
fn regex_matches_embedded_yaml_while_path_matches_host_markdown() {
    let dir = tempdir().unwrap();
    let config = dir.path().join(".ryl.toml");
    fs::write(
        &config,
        "[rules.comments]\n\n[files]\nmarkdown = [\"*.md\"]\n\n\
         [[per-line-ignores]]\npath = \"*.md\"\nregex = '^#cloud-config$'\n\
         rules = [\"comments\"]\n",
    )
    .unwrap();
    let file = dir.path().join("doc.md");
    fs::write(
        &file,
        "# Title\n\n```yaml\n#cloud-config\nkey: value\n```\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(
        code,
        0,
        "regex matches the embedded YAML line, path matches the host .md: {}",
        command_output(&stdout, &stderr)
    );
}

#[test]
fn pure_regex_entry_applies_to_unlabeled_stdin() {
    // Without --stdin-filename ryl drops path-based filtering, but a content-regex
    // entry has no path and must still apply.
    let dir = tempdir().unwrap();
    let config = dir.path().join(".ryl.toml");
    fs::write(
        &config,
        "[rules.comments]\n\n[[per-line-ignores]]\nregex = '^#cloud-config$'\n\
         rules = [\"comments\"]\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let mut child = Command::new(exe)
        .current_dir(dir.path())
        .arg("-c")
        .arg(&config)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"#cloud-config\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(
        out.status.code().unwrap_or(-1),
        0,
        "content-regex entry should suppress the stdin diagnostic"
    );
}

#[test]
fn invalid_regex_is_a_config_error() {
    let (code, out) = run_lint(
        "[rules.comments]\n[[per-line-ignores]]\nregex = '('\nrules = [\"comments\"]\n",
        "doc.yaml",
        "k: v\n",
    );
    assert_eq!(code, 2, "invalid regex must be rejected: {out}");
    assert!(out.contains("per-line-ignores `regex`"), "{out}");
}

#[test]
fn invalid_path_glob_is_a_config_error() {
    let (code, out) = run_lint(
        "[rules.comments]\n[[per-line-ignores]]\npath = '['\nrules = [\"comments\"]\n",
        "doc.yaml",
        "k: v\n",
    );
    assert_eq!(code, 2, "invalid glob must be rejected: {out}");
    assert!(out.contains("per-line-ignores `path`"), "{out}");
}

#[test]
fn entry_without_regex_or_path_is_a_config_error() {
    let (code, out) = run_lint(
        "[rules.comments]\n[[per-line-ignores]]\nrules = [\"comments\"]\n",
        "doc.yaml",
        "k: v\n",
    );
    assert_eq!(code, 2, "an unscoped entry must be rejected: {out}");
    assert!(out.contains("at least one of"), "{out}");
}

#[test]
fn empty_rules_list_is_a_config_error() {
    let (code, out) = run_lint(
        "[rules.comments]\n[[per-line-ignores]]\nregex = 'x'\nrules = []\n",
        "doc.yaml",
        "k: v\n",
    );
    assert_eq!(code, 2, "an empty rules list must be rejected: {out}");
    assert!(out.contains("empty `rules`"), "{out}");
}

#[test]
fn unknown_rule_name_is_a_config_error() {
    let (code, out) = run_lint(
        "[rules.comments]\n[[per-line-ignores]]\nregex = 'x'\nrules = [\"nope\"]\n",
        "doc.yaml",
        "k: v\n",
    );
    assert_eq!(code, 2, "an unknown rule name must be rejected: {out}");
}

#[test]
fn path_glob_matches_a_relative_cli_path() {
    // Invoked with a relative path so `path_matches` joins it onto the base dir
    // (the non-absolute branch), mirroring per-file-ignores' relative matching.
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules.comments]\n\n[[per-line-ignores]]\npath = \"*.yaml\"\n\
         regex = '^#cloud-config$'\nrules = [\"comments\"]\n",
    )
    .unwrap();
    fs::write(dir.path().join("rel.yaml"), "#cloud-config\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .current_dir(dir.path())
        .arg("-c")
        .arg(".ryl.toml")
        .arg("rel.yaml"));
    assert_eq!(
        code,
        0,
        "relative path should match the glob and suppress comments: {}",
        command_output(&stdout, &stderr)
    );
}

#[test]
fn effective_config_round_trips_per_line_ignores_to_toml() {
    // `to_toml_string` must preserve per-line-ignores so the rendered effective config
    // is not lossy. A regex-only and a path-only entry exercise both optional fields.
    let cfg = ryl::config::YamlLintConfig::from_toml_str(
        "[rules.comments]\n\n[[per-line-ignores]]\nregex = '#x'\nrules = [\"comments\"]\n\n\
         [[per-line-ignores]]\npath = \"*.yaml\"\nrules = [\"comments\"]\n",
    )
    .unwrap();
    let rendered = cfg.to_toml_string();
    assert!(
        rendered.contains("per-line-ignores")
            && rendered.contains("regex")
            && rendered.contains("#x")
            && rendered.contains("path")
            && rendered.contains("*.yaml")
            && rendered.contains("comments"),
        "rendered TOML should keep both per-line-ignores entries: {rendered}"
    );
}

#[test]
fn per_line_ignores_is_rejected_in_yaml_config() {
    let dir = tempdir().unwrap();
    let config = dir.path().join(".ryl.yaml");
    fs::write(
        &config,
        "rules:\n  comments: {}\nper-line-ignores:\n  - regex: x\n    rules: [comments]\n",
    )
    .unwrap();
    let file = dir.path().join("doc.yaml");
    fs::write(&file, "k: v\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    let out = command_output(&stdout, &stderr);
    assert_eq!(code, 2, "per-line-ignores is TOML-only: {out}");
    assert!(
        out.contains("per-line-ignores is only supported in TOML"),
        "{out}"
    );
}
