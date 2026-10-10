mod common;

use std::fs;
use std::path::Path;

use common::cli::{run, ryl};
use tempfile::tempdir;

const TOML_CANDIDATES: [&str; 5] = [
    ".ryl.toml",
    "ryl.toml",
    ".config/.ryl.toml",
    ".config/ryl.toml",
    "pyproject.toml",
];

fn write_native(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let table = if path.file_name().unwrap() == "pyproject.toml" {
        "tool.ryl.lint.rules"
    } else {
        "lint.rules"
    };
    fs::write(path, format!("[{table}]\nanchors = \"enable\"\n")).unwrap();
}

fn assert_shadowed_migration(
    candidate: &str,
    ancestor: bool,
    yaml: &str,
    flags: &[&str],
) {
    let td = tempdir().unwrap();
    let project = td.path().join("project");
    fs::create_dir(&project).unwrap();
    let native = if ancestor { td.path() } else { &project }.join(candidate);
    write_native(&native);
    let original = fs::read(&native).unwrap();
    fs::write(project.join(yaml), "rules: {truthy: enable}\n").unwrap();
    let sibling = if yaml == ".yamllint.yml" {
        ".yamllint"
    } else {
        ".yamllint.yml"
    };
    fs::write(project.join(sibling), "rules: {truthy: enable}\n").unwrap();
    let input = project.join("a.yaml");
    fs::write(&input, "a: yes\n").unwrap();
    assert_eq!(
        run(ryl(td.path()).args(["check", "--no-warnings"]).arg(&input)).0,
        0
    );
    let (code, stdout, stderr) = run(ryl(td.path())
        .args(["--migrate-configs", "--migrate-root"])
        .arg(&project)
        .args(flags));
    assert_eq!(code, 0, "{candidate} ancestor={ancestor}: {stdout}{stderr}");
    assert!(stderr.contains("skipping migration"), "{stderr}");
    assert!(stderr.contains(&native.display().to_string()), "{stderr}");
    assert_eq!(fs::read(&native).unwrap(), original);
    if candidate != ".ryl.toml" || ancestor {
        assert!(
            !project.join(".ryl.toml").exists(),
            "{candidate} ancestor={ancestor}"
        );
    }
    assert!(project.join(yaml).exists());
    assert!(project.join(sibling).exists());
    assert!(!project.join(format!("{yaml}.bak")).exists());
    assert!(!project.join(format!("{sibling}.bak")).exists());
    assert_eq!(
        run(ryl(td.path()).args(["check", "--no-warnings"]).arg(&input)).0,
        0
    );
}

#[test]
fn migration_preserves_every_discovered_toml_config_and_shadowed_yaml() {
    for candidate in TOML_CANDIDATES {
        for ancestor in [false, true] {
            for yaml in [".yamllint", ".yamllint.yaml", ".yamllint.yml"] {
                for flags in [
                    vec![],
                    vec!["--migrate-write"],
                    vec!["--migrate-write", "--migrate-delete-old"],
                    vec!["--migrate-write", "--migrate-rename-old", ".bak"],
                ] {
                    assert_shadowed_migration(candidate, ancestor, yaml, &flags);
                }
            }
        }
    }
}

#[test]
fn migration_uses_candidate_and_ancestor_precedence() {
    for (index, selected) in TOML_CANDIDATES.iter().enumerate() {
        let td = tempdir().unwrap();
        let project = td.path().join("project");
        fs::create_dir(&project).unwrap();
        write_native(&td.path().join(".ryl.toml"));
        for candidate in &TOML_CANDIDATES[index..] {
            write_native(&project.join(candidate));
        }
        fs::write(project.join(".yamllint"), "rules: {truthy: enable}\n").unwrap();
        let (code, _, stderr) = run(ryl(td.path())
            .args(["--migrate-configs", "--migrate-write", "--migrate-root"])
            .arg(&project));
        assert_eq!(code, 0, "{stderr}");
        assert!(
            stderr.contains(&project.join(selected).display().to_string()),
            "{stderr}"
        );
    }
}

#[test]
fn migration_ignores_pyproject_without_ryl_and_toml_above_home_or_below_source() {
    let td = tempdir().unwrap();
    write_native(&td.path().join(".ryl.toml"));
    let home = td.path().join("home");
    fs::create_dir(&home).unwrap();
    fs::write(home.join("pyproject.toml"), "[tool.other]\nvalue = true\n").unwrap();
    write_native(&home.join("child/.ryl.toml"));
    fs::write(home.join(".yamllint"), "rules: {truthy: enable}\n").unwrap();
    let (code, stdout, stderr) = run(ryl(&home)
        .args(["--migrate-configs", "--migrate-write", "--migrate-root"])
        .arg(&home));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(home.join(".ryl.toml").exists(), "{stdout}{stderr}");
}

#[test]
fn migration_skips_empty_ryl_pyproject_but_continues_past_unrelated_pyproject() {
    for pyproject in ["[tool.ryl.lint.rules]\n", "[tool.other]\n"] {
        let td = tempdir().unwrap();
        write_native(&td.path().join("ryl.toml"));
        let project = td.path().join("project");
        fs::create_dir(&project).unwrap();
        fs::write(project.join("pyproject.toml"), pyproject).unwrap();
        fs::write(project.join(".yamllint"), "rules: {truthy: enable}\n").unwrap();
        let (code, _, stderr) = run(ryl(td.path())
            .args(["--migrate-configs", "--migrate-write", "--migrate-root"])
            .arg(project.join(".yamllint")));
        assert_eq!(code, 0, "{stderr}");
        let expected = if pyproject.contains("tool.ryl") {
            project.join("pyproject.toml")
        } else {
            td.path().join("ryl.toml")
        };
        assert!(stderr.contains(&expected.display().to_string()), "{stderr}");
        assert!(!project.join(".ryl.toml").exists());
    }
}

#[test]
fn migration_of_explicitly_named_yaml_is_not_blocked_by_project_discovery() {
    let td = tempdir().unwrap();
    write_native(&td.path().join("pyproject.toml"));
    fs::write(td.path().join("custom.yaml"), "rules: {truthy: enable}\n").unwrap();
    let (code, stdout, stderr) = run(ryl(td.path())
        .args(["--migrate-configs", "--migrate-write", "--migrate-root"])
        .arg(td.path().join("custom.yaml")));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(td.path().join("custom.toml").exists());
}

#[test]
fn migration_does_not_write_when_pyproject_discovery_fails() {
    let td = tempdir().unwrap();
    fs::write(td.path().join("pyproject.toml"), "[tool.ryl\n").unwrap();
    fs::write(td.path().join(".yamllint"), "rules: {truthy: enable}\n").unwrap();
    let (code, _, stderr) = run(ryl(td.path())
        .args(["--migrate-configs", "--migrate-write", "--migrate-root"])
        .arg(td.path()));
    assert_eq!(code, 2, "{stderr}");
    assert!(!td.path().join(".ryl.toml").exists());
    assert!(td.path().join(".yamllint").exists());
}

#[test]
fn user_migration_is_not_blocked_by_project_toml_in_config_directory_ancestors() {
    let td = tempdir().unwrap();
    write_native(&td.path().join(".ryl.toml"));
    let config_dir = td.path().join("xdg");
    fs::create_dir_all(config_dir.join("yamllint")).unwrap();
    fs::write(
        config_dir.join("yamllint/config"),
        "rules: {truthy: enable}\n",
    )
    .unwrap();
    let (code, stdout, stderr) = run(ryl(td.path())
        .env("XDG_CONFIG_HOME", &config_dir)
        .args(["--migrate-user-config", "--migrate-write"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(config_dir.join("ryl/ryl.toml").exists());
}

#[test]
fn project_migration_is_not_blocked_by_lower_precedence_user_toml() {
    let td = tempdir().unwrap();
    let config_dir = td.path().join("xdg");
    write_native(&config_dir.join("ryl/ryl.toml"));
    fs::write(td.path().join(".yamllint"), "rules: {truthy: enable}\n").unwrap();
    let (code, stdout, stderr) = run(ryl(td.path())
        .env("XDG_CONFIG_HOME", &config_dir)
        .args(["--migrate-configs", "--migrate-write", "--migrate-root"])
        .arg(td.path()));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(td.path().join(".ryl.toml").exists());
}
