mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::cli::{run, ryl};
use tempfile::tempdir;

const LEGACY: &str = r#"ignore = ["vendor/**"]

[rules]
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

const NESTED: &str = r#"exclude = ["vendor/**"]

[lint]
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
    fs::create_dir(dir.path().join("vendor")).unwrap();
    fs::write(dir.path().join("vendor/v.yaml"), DOC).unwrap();
    dir
}

fn check(dir: &Path, extra: &[&str]) -> (i32, String, String) {
    run(ryl(dir).current_dir(dir).arg("check").args(extra).arg("."))
}

fn migrate(dir: &Path, extra: &[&str]) -> (i32, String, String) {
    run(ryl(dir)
        .args(["--migrate-configs", "--migrate-root"])
        .arg(dir)
        .args(extra))
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
    assert!(
        !nested_err.contains("v.yaml"),
        "exclude applies: {nested_err}"
    );
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
        ("ignore", "exclude"),
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
    let (code, stdout, stderr) = migrate(
        dir.path(),
        &["--migrate-write", "--migrate-rename-old", ".bak"],
    );
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
    assert!(
        migrated.contains("exclude = [") && !migrated.contains("ignore = ["),
        "{migrated}"
    );

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
    let (code, stdout, _) = migrate(dir.path(), &["--migrate-stdout"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("[lint.rules]"), "{stdout}");
    assert_eq!(
        fs::read_to_string(dir.path().join(".ryl.toml")).unwrap(),
        LEGACY
    );
}

#[test]
fn migrate_preserves_comments_in_issue_repro() {
    let original = "# project ryl config\n[rules]\n# keep docs tidy\n\
document-start = \"disable\"  # trailing note\n\n[fix]\nfixable = [\"ALL\"]\n";
    let dir = project(original);
    let (code, _, stderr) = migrate(dir.path(), &["--migrate-write"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        fs::read_to_string(dir.path().join(".ryl.toml")).unwrap(),
        "# project ryl config\n[lint.rules]\n# keep docs tidy\n\
document-start = \"disable\"  # trailing note\n\n[lint]\nfixable = [\"ALL\"]\n"
    );
}

#[test]
fn migrate_moves_fix_keys_into_existing_lint_table() {
    let original = "# config\n[lint]\nstrict = true\n# existing lint key\n\
unfixable = [\"truthy\"]\n\n# fix policy\n[fix]\nfixable = [\"ALL\"]  # move me\n\n\
[rules]\ntruthy = \"enable\"\n";
    let dir = project(original);
    let (code, _, stderr) = migrate(dir.path(), &["--migrate-write"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        fs::read_to_string(dir.path().join(".ryl.toml")).unwrap(),
        "# config\n[lint]\nstrict = true\n# existing lint key\n\
unfixable = [\"truthy\"]\n\n# fix policy\nfixable = [\"ALL\"]  # move me\n\n\
[lint.rules]\ntruthy = \"enable\"\n"
    );
}

#[test]
fn migrate_renames_top_level_keys_without_reordering_neighbors() {
    let original = "line-length = 88\n# files to skip\nignore = ['vendor/**']  # old name\n\
indent-width = 4\nignore-from-file = 'excluded.txt'\n\n[rules]\ntruthy = 'enable'\n";
    let dir = project(original);
    let (code, _, stderr) = migrate(dir.path(), &["--migrate-write"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        fs::read_to_string(dir.path().join(".ryl.toml")).unwrap(),
        "line-length = 88\n# files to skip\nexclude = ['vendor/**']  # old name\n\
indent-width = 4\nexclude-from-file = 'excluded.txt'\n\n[lint.rules]\ntruthy = 'enable'\n"
    );
}

#[test]
fn migrate_inline_lint_table_keeps_existing_keys() {
    let dir = project(
        "lint = { strict = true, unfixable = ['truthy'] } # existing inline lint\n\
fix = { fixable = ['ALL'] }\n[rules]\ntruthy = 'enable'\n",
    );
    let (code, _, stderr) = migrate(dir.path(), &["--migrate-write"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        fs::read_to_string(dir.path().join(".ryl.toml")).unwrap(),
        "[lint] # existing inline lint\nstrict = true\nunfixable = ['truthy']\n\
fixable = ['ALL']\n[lint.rules]\ntruthy = 'enable'\n"
    );
}

#[test]
fn migrate_keeps_comments_when_new_keys_override_deprecated_values() {
    let dir = project(
        "# config header\n# before old\nignore = ['old/**'] # old ignore note\n\
exclude = ['new/**']\n\n# old rules section\n[rules] # old rules header\n\
# old rule note\ntruthy = 'enable' # old truthy\n\n# old fix section\n\
[fix] # old fix header\n# old fix note\nfixable = ['truthy'] # old fixable\n\n\
[lint]\nfixable = ['ALL']\n\n\
[lint.rules]\ntrailing-spaces = 'enable'\n",
    );
    let (code, _, stderr) = migrate(dir.path(), &["--migrate-write"]);
    assert_eq!(code, 0, "{stderr}");
    let migrated = fs::read_to_string(dir.path().join(".ryl.toml")).unwrap();
    assert_eq!(
        migrated,
        "# config header\n# before old\n# old ignore note\nexclude = ['new/**']\n\n\
[lint]\n\n# old fix section\n# old fix header\n# old fix note\n\
# old fixable\nfixable = ['ALL']\n# old rules section\n\
# old rules header\n# old rule note\n# old truthy\n\n\
[lint.rules]\ntrailing-spaces = 'enable'\n"
    );
}

#[test]
fn migrate_keeps_comments_from_overridden_per_line_ignores() {
    let dir = project(
        "# old file setting\nignore-from-file = 'old.txt' # old file note\n\
exclude-from-file = 'new.txt'\n\n[[per-line-ignores]] # old line header\n\
# old regex note\nregex = 'OLD'\nrules = ['comments'] # old rules note\n\n\
[[lint.per-line-ignores]]\nregex = 'NEW'\nrules = ['comments']\n",
    );
    let (code, _, stderr) = migrate(dir.path(), &["--migrate-write"]);
    assert_eq!(code, 0, "{stderr}");
    let migrated = fs::read_to_string(dir.path().join(".ryl.toml")).unwrap();
    assert_eq!(
        migrated,
        "# old file setting\n# old file note\nexclude-from-file = 'new.txt'\n\
# old line header\n# old regex note\n# old rules note\n\n\
[[lint.per-line-ignores]]\nregex = 'NEW'\nrules = ['comments']\n"
    );
}

#[test]
fn migrate_keeps_comments_from_overridden_inline_per_file_ignores() {
    let dir = project(
        "# old inline\nper-file-ignores = { 'skip.yaml' = ['ALL'] } # old value\n\
[lint]\nper-file-ignores = { 'a.yaml' = ['ALL'] }\n",
    );
    let (code, _, stderr) = migrate(dir.path(), &["--migrate-write"]);
    assert_eq!(code, 0, "{stderr}");
    let migrated = fs::read_to_string(dir.path().join(".ryl.toml")).unwrap();
    assert_eq!(
        migrated,
        "[lint]\n# old inline\n# old value\n\
per-file-ignores = { 'a.yaml' = ['ALL'] }\n"
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
    let (code, stdout, stderr) = migrate(dir.path(), &["--migrate-write"]);
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
fn pyproject_deprecation_says_to_move_the_key_by_hand() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("pyproject.toml"),
        "[tool.ryl.rules]\ntruthy = \"enable\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("a.yaml"), DOC).unwrap();
    let (_, _, stderr) = check(dir.path(), &[]);
    assert!(
        stderr.contains(
            "`rules` is deprecated; use `lint.rules` instead (move it to \
             `tool.ryl.lint.rules` by hand)"
        ),
        "{stderr}"
    );
}

#[test]
fn deprecation_names_no_command_for_a_source_migration_cannot_rewrite() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lint.toml"), LEGACY).unwrap();
    fs::write(dir.path().join("a.yaml"), DOC).unwrap();
    for source in [["-d", LEGACY], ["-c", "lint.toml"]] {
        let (_, _, stderr) = check(dir.path(), &source);
        assert!(
            stderr.contains("`rules` is deprecated; use `lint.rules` instead\n"),
            "{source:?}: {stderr}"
        );
        assert!(!stderr.contains("--migrate"), "{source:?}: {stderr}");
    }
}

#[test]
fn deprecation_hint_uses_a_placeholder_for_a_path_with_a_space() {
    let dir = tempdir().unwrap();
    let project = dir.path().join("space dir");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join(".ryl.toml"), LEGACY).unwrap();
    fs::write(project.join("a.yaml"), DOC).unwrap();
    let (_, _, stderr) = check(&project, &[]);
    assert!(
        stderr.contains(
            "(run `ryl --migrate-configs --migrate-write --migrate-root <path to this \
             config>` to update)"
        ),
        "{stderr}"
    );
}

#[test]
fn deprecation_hint_migrates_a_config_outside_the_working_directory() {
    for (config, content) in [
        (".ryl.toml", LEGACY),
        (".yamllint", "rules:\n  trailing-spaces: enable\n"),
    ] {
        let dir = tempdir().unwrap();
        let project = dir.path().join("proj");
        let elsewhere = dir.path().join("elsewhere");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&elsewhere).unwrap();
        fs::write(project.join(config), content).unwrap();
        fs::write(project.join("a.yaml"), DOC).unwrap();
        let relative_config = format!("../proj/{config}");
        let (_, _, stderr) = run(ryl(dir.path()).current_dir(&elsewhere).args([
            "check",
            "-c",
            &relative_config,
            "../proj/a.yaml",
        ]));
        let hint = stderr
            .split('`')
            .find(|part| part.starts_with("ryl --migrate-configs"))
            .unwrap_or_else(|| panic!("no migrate command in: {stderr}"));
        let (code, stdout, stderr) = run(ryl(dir.path())
            .current_dir(dir.path())
            .args(hint.split_whitespace().skip(1)));
        assert_eq!(code, 0, "{hint}: stdout={stdout} stderr={stderr}");
        assert!(
            fs::read_to_string(project.join(".ryl.toml"))
                .unwrap()
                .contains("[lint.rules]"),
            "{config}: {hint}: {stdout}"
        );
    }
}

#[test]
fn migrate_skips_an_unparsable_toml_config_with_a_warning() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("ryl.toml"), "rules = [\n").unwrap();
    let (code, _, stderr) = migrate(dir.path(), &[]);
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
    let (code, _, stderr) = migrate(dir.path(), &[]);
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
    fs::write(dir.path().join("a.yaml"), DOC).unwrap();
    let (_, _, stderr) = check(dir.path(), &[]);
    assert!(
        stderr.contains(
            "instead (migration skips symlinks: edit the link's target by hand)"
        ),
        "{stderr}"
    );
    assert!(!stderr.contains("--migrate"), "{stderr}");
    let (code, _, stderr) = migrate(dir.path(), &["--migrate-write"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.contains("refusing to follow a symlink"), "{stderr}");
    assert_eq!(
        fs::read_to_string(dir.path().join("real.toml")).unwrap(),
        LEGACY
    );
}

#[cfg(unix)]
#[test]
fn migrate_rewrite_refuses_a_symlink_target() {
    use ryl::migrate::{MigrationEntry, SourceCleanup, apply_migration_entries};

    let dir = tempdir().unwrap();
    let payload = dir.path().join("payload.toml");
    let link = dir.path().join(".ryl.toml");
    fs::write(&payload, LEGACY).unwrap();
    std::os::unix::fs::symlink(&payload, &link).unwrap();
    let entry = MigrationEntry {
        source: link.clone(),
        target: link.clone(),
        toml: NESTED.to_string(),
    };

    let error =
        apply_migration_entries(&[entry], &[], &SourceCleanup::Keep).unwrap_err();
    assert!(error.contains("refusing to follow a symlink"), "{error}");
    assert_eq!(fs::read(&payload).unwrap(), LEGACY.as_bytes());
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[cfg(unix)]
#[test]
fn migrate_failed_rewrite_keeps_original_bytes() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let path = dir.path().join(".ryl.toml");
    fs::write(&path, LEGACY).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500)).unwrap();

    let (code, _, stderr) = migrate(dir.path(), &["--migrate-write"]);

    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(code, 2, "{stderr}");
    assert_eq!(fs::read(&path).unwrap(), LEGACY.as_bytes());
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[cfg(unix)]
#[test]
fn migrate_rewrite_and_backup_preserve_metadata() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt, chown};
    use std::process::Command;

    let dir = tempdir().unwrap();
    let path = dir.path().join(".ryl.toml");
    fs::write(&path, LEGACY).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    let initial_gid = fs::metadata(&path).unwrap().gid();
    let groups = Command::new("id").arg("-G").output().unwrap();
    assert!(groups.status.success());
    let other_gid = String::from_utf8(groups.stdout)
        .unwrap()
        .split_whitespace()
        .filter_map(|group| group.parse::<u32>().ok())
        .find(|&group| group != initial_gid);
    if let Some(gid) = other_gid {
        chown(&path, None, Some(gid)).unwrap();
    }
    let before = fs::metadata(&path).unwrap();

    let (code, _, stderr) = migrate(
        dir.path(),
        &["--migrate-write", "--migrate-rename-old", ".bak"],
    );

    assert_eq!(code, 0, "{stderr}");
    assert!(fs::read_to_string(&path).unwrap().contains("[lint.rules]"));
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert_eq!(fs::metadata(&path).unwrap().uid(), before.uid());
    assert_eq!(fs::metadata(&path).unwrap().gid(), before.gid());
    let backup = dir.path().join(".ryl.toml.bak");
    assert_eq!(fs::read(&backup).unwrap(), LEGACY.as_bytes());
    assert_eq!(
        fs::metadata(&backup).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert_eq!(fs::metadata(&backup).unwrap().uid(), before.uid());
    assert_eq!(fs::metadata(&backup).unwrap().gid(), before.gid());
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
    let (code, _, stderr) = migrate(
        dir.path(),
        &["--migrate-write", "--migrate-rename-old", ".bak"],
    );
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

/// A project dir under `home` with no project config, so discovery falls back to the
/// ryl user-global config at `<xdg>/ryl/ryl.toml`.
fn user_global(config: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let td = tempdir().unwrap();
    let project = td.path().join("home/project");
    let xdg = td.path().join("xdg");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(xdg.join("ryl")).unwrap();
    fs::write(xdg.join("ryl/ryl.toml"), config).unwrap();
    fs::write(project.join("a.yaml"), DOC).unwrap();
    (td, project, xdg)
}

fn user_global_check(td: &Path, project: &Path, xdg: &Path) -> (i32, String, String) {
    run(ryl(&td.join("home"))
        .env("XDG_CONFIG_HOME", xdg)
        .current_dir(project)
        .args(["check", "a.yaml"]))
}

fn diagnostics(stderr: &str) -> Vec<&str> {
    stderr
        .lines()
        .filter(|line| !line.starts_with("warning: "))
        .collect()
}

#[test]
fn user_global_deprecation_names_the_user_config_migration() {
    let (td, project, xdg) = user_global(LEGACY);
    let (_, _, stderr) = user_global_check(td.path(), &project, &xdg);
    assert!(
        stderr.contains(
            "`rules` is deprecated; use `lint.rules` instead (run \
             `ryl --migrate-user-config --migrate-write` to update)"
        ),
        "{stderr}"
    );
}

#[test]
fn migrate_user_config_rewrites_legacy_user_global_toml_in_place() {
    let (td, project, xdg) = user_global(LEGACY);
    let (_, legacy_out, legacy_err) = user_global_check(td.path(), &project, &xdg);
    let (code, stdout, stderr) = run(ryl(&td.path().join("home"))
        .env("XDG_CONFIG_HOME", &xdg)
        .args([
            "--migrate-user-config",
            "--migrate-write",
            "--migrate-delete-old",
        ]));
    assert_eq!(code, 0, "{stderr}");
    assert!(
        !stdout.contains("No user-global config migrated"),
        "{stdout}"
    );
    let migrated = fs::read_to_string(xdg.join("ryl/ryl.toml"))
        .expect("--migrate-delete-old never deletes a rewritten config");
    assert!(migrated.contains("[lint.rules]"), "{migrated}");
    let (_, migrated_out, migrated_err) = user_global_check(td.path(), &project, &xdg);
    assert_eq!(migrated_out, legacy_out);
    assert!(!migrated_err.contains("is deprecated"), "{migrated_err}");
    assert_eq!(diagnostics(&migrated_err), diagnostics(&legacy_err));
}

#[test]
fn user_global_toml_under_the_migrate_root_is_planned_once() {
    let (td, _, xdg) = user_global(LEGACY);
    let (code, stdout, stderr) = run(ryl(&td.path().join("home"))
        .env("XDG_CONFIG_HOME", &xdg)
        .args([
            "--migrate-configs",
            "--migrate-user-config",
            "--migrate-root",
        ])
        .arg(&xdg));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout.matches("ryl.toml -> ").count(), 1, "{stdout}");
}

#[test]
fn user_global_toml_reached_by_relative_root_keeps_original_backup() {
    let (td, _, xdg) = user_global(LEGACY);
    let user_dir = xdg.join("ryl");
    let (code, stdout, stderr) = run(ryl(&td.path().join("home"))
        .env("XDG_CONFIG_HOME", &xdg)
        .current_dir(&user_dir)
        .args([
            "--migrate-configs",
            "--migrate-root",
            ".",
            "--migrate-user-config",
            "--migrate-write",
            "--migrate-rename-old",
            ".bak",
        ]));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        fs::read_to_string(user_dir.join("ryl.toml.bak")).unwrap(),
        LEGACY,
        "the backup holds the original, not an already-migrated copy"
    );
    assert!(
        !stdout.contains("No user-global config migrated."),
        "the rewrite reached through the relative root is the user-global one: {stdout}"
    );
}
