use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use tempfile::tempdir;

mod common;
use common::cli::{command_output, run};

const HOOK_KEYS: &str = r#"keys = ["alias", "name", "description", "args", "env"]"#;
const HOOK: &str = "repos:
  - repo: local
    hooks:
      - id: example
        args: [--check]
        # Display name for this hook.
        name: Example hook
        alias: example-check
        description: Check example files.
        env:
          b: 1
          a: 2
";

fn hooks_config(extra: &str) -> String {
    format!(
        "[rules.key-ordering]\n\n[[rules.key-ordering.orders]]\n\
         files = [\".pre-commit-config.yaml\"]\npath = \"$.repos[*].hooks[*]\"\n\
         {HOOK_KEYS}\n{extra}"
    )
}

fn check(config: &str, name: &str, body: &str, args: &[&str]) -> (String, String) {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join(".ryl.toml");
    fs::write(&config_path, config).unwrap();
    let file = dir.path().join(name);
    fs::write(&file, body).unwrap();
    let (_, stdout, stderr) = run(Command::new(env!("CARGO_BIN_EXE_ryl"))
        .arg("check")
        .arg("-c")
        .arg(&config_path)
        .args(args)
        .arg(&file));
    (
        command_output(&stdout, &stderr).to_owned(),
        fs::read_to_string(&file).unwrap(),
    )
}

fn fixed(config: &str, name: &str, body: &str) -> String {
    check(config, name, body, &["--fix"]).1
}

#[test]
fn unlisted_keys_sort_after_listed_keys_by_default() {
    let (out, _) = check(&hooks_config(""), ".pre-commit-config.yaml", HOOK, &[]);
    for position in ["5:9", "7:9", "8:9", "9:9", "10:9", "12:11"] {
        assert!(out.contains(position), "{position} missing: {out}");
    }
    assert_eq!(
        fixed(&hooks_config(""), ".pre-commit-config.yaml", HOOK),
        "repos:
  - hooks:
      - alias: example-check
        # Display name for this hook.
        name: Example hook
        description: Check example files.
        args: [--check]
        env:
          a: 2
          b: 1
        id: example
    repo: local
"
    );
}

#[test]
fn unlisted_keep_holds_unlisted_keys_in_their_slots() {
    let config = hooks_config("unlisted = \"keep\"\n");
    let body = "- id: example\n  args: [--check]\n  # Display name for this hook.\n  \
                name: Example hook\n  alias: example-check\n  \
                description: Check example files.\n";
    let config = config.replace("$.repos[*].hooks[*]", "$[*]");
    assert_eq!(
        fixed(&config, ".pre-commit-config.yaml", body),
        "- id: example\n  alias: example-check\n  # Display name for this hook.\n  \
         name: Example hook\n  description: Check example files.\n  args: [--check]\n"
    );
}

#[test]
fn orders_apply_only_where_files_and_path_both_match() {
    let (listed, alphabetical) =
        ("name: 1\n  description: 2\n", "description: 2\n  name: 1\n");
    let config = hooks_config("").replace("$.repos[*].hooks[*]", "$.x");
    let body = format!("x:\n  {alphabetical}y:\n  {listed}");
    assert_eq!(
        fixed(&config, ".pre-commit-config.yaml", &body),
        format!("x:\n  {listed}y:\n  {alphabetical}")
    );
    assert_eq!(
        fixed(&config, "other.yaml", &body),
        format!("x:\n  {alphabetical}y:\n  {alphabetical}")
    );
}

#[test]
fn first_matching_entry_wins_and_excluded_files_fall_back() {
    let config = "[rules.key-ordering]\n\n[[rules.key-ordering.orders]]\n\
                  files = [\"*.yaml\", \"!skip.yaml\"]\npath = \"$\"\nkeys = [\"z\", \"a\"]\n\n\
                  [[rules.key-ordering.orders]]\nfiles = [\"*\"]\npath = \"$\"\n\
                  keys = [\"a\", \"z\"]\n";
    assert_eq!(fixed(config, "doc.yaml", "a: 1\nz: 2\n"), "z: 2\na: 1\n");
    assert_eq!(fixed(config, "skip.yaml", "z: 2\na: 1\n"), "a: 1\nz: 2\n");
    let only_excludes = config.replace("\"*.yaml\", ", "");
    assert_eq!(
        fixed(&only_excludes, "doc.yml", "a: 1\nz: 2\n"),
        "z: 2\na: 1\n"
    );
}

#[test]
fn ignored_keys_hold_their_slots_inside_a_listed_mapping() {
    let config = "[rules.key-ordering]\nignored-keys = [\"^x$\"]\n\n\
                  [[rules.key-ordering.orders]]\nfiles = [\"*\"]\npath = \"$\"\n\
                  keys = [\"b\", \"a\"]\n";
    assert_eq!(
        fixed(config, "doc.yaml", "a: 1\nx: 0\nc: 3\nb: 2\n"),
        "b: 2\nx: 0\na: 1\nc: 3\n"
    );
}

#[test]
fn paths_name_quoted_keys_items_and_each_document_root() {
    let config = "[rules.key-ordering]\n\n[[rules.key-ordering.orders]]\n\
                  files = [\"*\"]\npath = \"$['my-key'][\\\"it's\\\"].*[*]\"\n\
                  keys = [\"b\", \"a\"]\n";
    let body = "my-key:\n  it's:\n    k:\n      - a: 1\n        b: 2\n---\n\
                my-key:\n  it's:\n    - - a: 1\n        b: 2\n";
    assert_eq!(
        fixed(config, "doc.yaml", body),
        body.replace("a: 1\n        b: 2", "b: 2\n        a: 1")
    );
    let escaped = config.replace("['my-key']", r"['my\\'key']");
    let body = "my'key:\n  it's:\n    k:\n      - a: 1\n        b: 2\n";
    assert_eq!(
        fixed(&escaped, "doc.yaml", body),
        body.replace("a: 1\n        b: 2", "b: 2\n        a: 1")
    );
}

#[test]
fn values_of_non_scalar_keys_are_never_selected() {
    let config = "[rules.key-ordering]\n\n[[rules.key-ordering.orders]]\n\
                  files = [\"*\"]\npath = \"$.*\"\nkeys = [\"b\", \"a\"]\n";
    let body = "k: &x v\n*x : {a: 1, b: 2}\n? [k]\n: a: 1\n  b: 2\n";
    let (out, after) = check(config, "doc.yaml", body, &["--fix"]);
    assert!(!out.contains("key-ordering"), "{out}");
    assert_eq!(after, body);
}

#[test]
fn stdin_needs_a_filename_to_match_files() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join(".ryl.toml");
    fs::write(
        &config_path,
        "[rules.key-ordering]\n\n[[rules.key-ordering.orders]]\n\
         files = [\"*\"]\npath = \"$\"\nkeys = [\"b\", \"a\"]\n",
    )
    .unwrap();
    let lint = |extra: &[&str]| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_ryl"))
            .arg("check")
            .arg("-c")
            .arg(&config_path)
            .args(extra)
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
            .write_all(b"b: 1\na: 2\n")
            .unwrap();
        let output = child.wait_with_output().unwrap();
        String::from_utf8_lossy(&output.stdout).into_owned()
            + &String::from_utf8_lossy(&output.stderr)
    };
    assert!(
        lint(&[]).contains("2:1"),
        "no filename sorts alphabetically"
    );
    assert!(
        !lint(&["--stdin-filename", "doc.yaml"]).contains("key-ordering"),
        "the filename selects the listed order"
    );
}

#[test]
fn markdown_matches_files_against_the_host_path() {
    let config = "[rules.key-ordering]\n\n[[rules.key-ordering.orders]]\n\
                  files = [\"README.md\"]\npath = \"$\"\nkeys = [\"b\", \"a\"]\n";
    let body = "# T\n\n```yaml\na: 1\nb: 2\n```\n";
    assert_eq!(
        check(config, "README.md", body, &["--markdown", "--fix"]).1,
        "# T\n\n```yaml\nb: 2\na: 1\n```\n"
    );
}
