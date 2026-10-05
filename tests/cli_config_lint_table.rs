mod common;

use std::fs;
use std::path::Path;

use common::cli::{run, ryl};
use tempfile::tempdir;

const LEGACY: &str = r#"[rules]
trailing-spaces = "enable"
truthy = "enable"
comments = "enable"

[fix]
fixable = ["ALL"]
unfixable = ["truthy"]

[per-file-ignores]
"skip.yaml" = ["ALL"]

[[per-line-ignores]]
regex = "KEEP"
rules = ["comments"]
"#;

const NESTED: &str = r#"[lint]
fixable = ["ALL"]
unfixable = ["truthy"]

[lint.rules]
trailing-spaces = "enable"
truthy = "enable"
comments = "enable"

[lint.per-file-ignores]
"skip.yaml" = ["ALL"]

[[lint.per-line-ignores]]
regex = "KEEP"
rules = ["comments"]
"#;

const DOC: &str = "a: yes \nb: 1 #KEEP\n";

fn project(config: &str) -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".ryl.toml"), config).unwrap();
    fs::write(dir.path().join("a.yaml"), DOC).unwrap();
    fs::write(dir.path().join("skip.yaml"), DOC).unwrap();
    dir
}

fn check(dir: &Path, extra: &[&str]) -> (i32, String, String) {
    run(ryl(dir).current_dir(dir).arg("check").args(extra).arg("."))
}

#[test]
fn legacy_and_nested_shapes_lint_and_fix_identically() {
    let legacy = project(LEGACY);
    let nested = project(NESTED);
    let (legacy_code, legacy_out, legacy_err) = check(legacy.path(), &[]);
    let (nested_code, nested_out, nested_err) = check(nested.path(), &[]);
    assert_eq!(legacy_code, nested_code);
    assert_eq!(legacy_out, nested_out);
    assert!(
        nested_err.contains("1:7"),
        "trailing-spaces still fires: {nested_err}"
    );
    assert!(
        !nested_err.contains("2:"),
        "per-line ignore applies: {nested_err}"
    );
    assert!(!nested_err.contains("is deprecated"), "{nested_err}");
    let diagnostics = |stderr: &str| -> Vec<String> {
        stderr
            .lines()
            .filter(|line| !line.starts_with("warning: "))
            .map(str::to_string)
            .collect()
    };
    assert_eq!(diagnostics(&legacy_err), diagnostics(&nested_err));

    check(legacy.path(), &["--fix"]);
    check(nested.path(), &["--fix"]);
    assert_eq!(
        fs::read_to_string(legacy.path().join("a.yaml")).unwrap(),
        "a: yes\nb: 1 #KEEP\n",
        "fix policy (truthy unfixable) carried over from [fix]"
    );
    assert_eq!(
        fs::read_to_string(legacy.path().join("a.yaml")).unwrap(),
        fs::read_to_string(nested.path().join("a.yaml")).unwrap()
    );
}

#[test]
fn each_legacy_key_warns_once_naming_its_replacement() {
    let dir = project(LEGACY);
    let (_, _, stderr) = check(dir.path(), &[]);
    for (key, replacement) in [
        ("rules", "lint.rules"),
        ("fix.fixable", "lint.fixable"),
        ("fix.unfixable", "lint.unfixable"),
        ("per-file-ignores", "lint.per-file-ignores"),
        ("per-line-ignores", "lint.per-line-ignores"),
    ] {
        let warning = format!("`{key}` is deprecated; use `{replacement}` instead");
        assert_eq!(stderr.matches(&warning).count(), 1, "{warning}: {stderr}");
    }
}

#[test]
fn no_warnings_silences_deprecation_warnings() {
    let dir = project(LEGACY);
    let (_, _, stderr) = check(dir.path(), &["--no-warnings"]);
    assert!(!stderr.contains("is deprecated"), "{stderr}");
}

#[test]
fn nested_key_wins_over_its_legacy_location() {
    let dir = project(
        "[rules]\ntruthy = \"enable\"\n\n[lint.rules]\ntrailing-spaces = \"enable\"\n",
    );
    let (code, _, stderr) = check(dir.path(), &[]);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("trailing-spaces"), "{stderr}");
    assert!(
        !stderr.contains("truthy"),
        "legacy [rules] is ignored: {stderr}"
    );
    assert!(
        stderr.contains(
            "`rules` is deprecated and ignored because `lint.rules` is also set"
        ),
        "{stderr}"
    );
}

#[test]
fn migrate_rewrites_legacy_toml_in_place_to_a_config_that_does_not_warn() {
    let dir = project(LEGACY);
    let root = dir.path().to_str().unwrap();
    let (code, stdout, stderr) = run(ryl(dir.path()).args([
        "--migrate-configs",
        "--migrate-root",
        root,
        "--migrate-write",
        "--migrate-rename-old",
        ".bak",
    ]));
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains(".ryl.toml -> "), "{stdout}");
    assert_eq!(
        fs::read_to_string(dir.path().join(".ryl.toml.bak")).unwrap(),
        LEGACY,
        "--migrate-rename-old keeps the original as a backup"
    );
    let migrated = fs::read_to_string(dir.path().join(".ryl.toml")).unwrap();
    assert!(migrated.contains("[lint.rules]"), "{migrated}");
    assert!(!migrated.contains("[fix]"), "{migrated}");

    let (_, migrated_out, migrated_err) = check(dir.path(), &[]);
    let (_, nested_out, nested_err) = check(project(NESTED).path(), &[]);
    assert_eq!(migrated_out, nested_out);
    assert_eq!(
        migrated_err, nested_err,
        "the migrated config raises no warning"
    );
}

#[test]
fn migrate_dry_run_leaves_legacy_toml_untouched() {
    let dir = project(LEGACY);
    let root = dir.path().to_str().unwrap();
    let (code, stdout, _) = run(ryl(dir.path()).args([
        "--migrate-configs",
        "--migrate-root",
        root,
        "--migrate-stdout",
    ]));
    assert_eq!(code, 0);
    assert!(stdout.contains("[lint.rules]"), "{stdout}");
    assert_eq!(
        fs::read_to_string(dir.path().join(".ryl.toml")).unwrap(),
        LEGACY
    );
}

#[test]
fn migrate_reports_legacy_pyproject_without_rewriting_it() {
    let dir = tempdir().unwrap();
    let pyproject = "# keep me\n[tool.ryl.rules]\ntruthy = \"enable\"\n";
    fs::write(dir.path().join("pyproject.toml"), pyproject).unwrap();
    fs::write(dir.path().join("ryl.toml"), NESTED).unwrap();
    fs::create_dir(dir.path().join("other")).unwrap();
    fs::write(
        dir.path().join("other/pyproject.toml"),
        "[project]\nname = 'x'\n",
    )
    .unwrap();
    let root = dir.path().to_str().unwrap();
    let (code, stdout, stderr) = run(ryl(dir.path()).args([
        "--migrate-configs",
        "--migrate-root",
        root,
        "--migrate-write",
    ]));
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stderr.contains("in [tool.ryl], move `rules` to `lint.rules`"),
        "{stderr}"
    );
    assert!(
        stdout.contains("No legacy config files migrated"),
        "{stdout}"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("pyproject.toml")).unwrap(),
        pyproject
    );
}

#[test]
fn migrate_skips_an_unparsable_toml_config_with_a_warning() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("ryl.toml"), "rules = [\n").unwrap();
    let root = dir.path().to_str().unwrap();
    let (code, _, stderr) =
        run(ryl(dir.path()).args(["--migrate-configs", "--migrate-root", root]));
    assert_eq!(code, 0, "a broken TOML config does not block the migration");
    assert!(
        stderr.contains("ryl.toml: failed to parse config data"),
        "{stderr}"
    );
}

#[test]
fn migrate_skips_an_unreadable_toml_config_with_a_warning() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("ryl.toml"), [0xff, 0xfe]).unwrap();
    let root = dir.path().to_str().unwrap();
    let (code, _, stderr) =
        run(ryl(dir.path()).args(["--migrate-configs", "--migrate-root", root]));
    assert_eq!(code, 0);
    assert!(
        stderr.contains("ryl.toml: failed to read config"),
        "{stderr}"
    );
}

#[cfg(unix)]
#[test]
fn migrate_skips_a_symlinked_legacy_toml() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("real.toml"), LEGACY).unwrap();
    std::os::unix::fs::symlink(
        dir.path().join("real.toml"),
        dir.path().join("ryl.toml"),
    )
    .unwrap();
    let root = dir.path().to_str().unwrap();
    let (code, _, stderr) = run(ryl(dir.path()).args([
        "--migrate-configs",
        "--migrate-root",
        root,
        "--migrate-write",
    ]));
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.contains("refusing to follow a symlink"), "{stderr}");
    assert_eq!(
        fs::read_to_string(dir.path().join("real.toml")).unwrap(),
        LEGACY
    );
}

#[test]
fn yaml_config_rejects_the_toml_only_lint_table() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.yaml"), "a: 1\n").unwrap();
    let (code, _, stderr) = run(ryl(dir.path()).args([
        "check",
        "-d",
        "lint:\n  rules: {}\n",
        dir.path().join("a.yaml").to_str().unwrap(),
    ]));
    assert_eq!(code, 2);
    assert!(
        stderr.contains("lint is only supported in TOML configuration"),
        "{stderr}"
    );
}

#[test]
fn migrate_rewrite_refuses_to_overwrite_an_existing_backup() {
    let dir = project(LEGACY);
    fs::write(dir.path().join(".ryl.toml.bak"), "old backup").unwrap();
    let root = dir.path().to_str().unwrap();
    let (code, _, stderr) = run(ryl(dir.path()).args([
        "--migrate-configs",
        "--migrate-root",
        root,
        "--migrate-write",
        "--migrate-rename-old",
        ".bak",
    ]));
    assert_eq!(code, 2, "{stderr}");
    assert!(
        stderr.contains("refusing to overwrite existing backup"),
        "{stderr}"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join(".ryl.toml")).unwrap(),
        LEGACY
    );
}
