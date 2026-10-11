//! A yamllint config and the TOML `--migrate-configs` writes for it report the same
//! findings over one corpus.

use std::fs;
use std::path::Path;

use tempfile::tempdir;

mod common;
use common::cli::{run, ryl};

const CORPUS: [(&str, &str); 8] = [
    ("a.yaml", "key: yes\nother: 'single'\nf: .5\nmode: 0755\n"),
    ("b.yml", "---\nlist:\n- x\n-  y\nmap: {a: 1,b: 2}\n\n\n"),
    (
        "long.yaml",
        "k: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
    ),
    ("dup.yaml", "a: 1\na: 2\nb: &x 1\nempty:\n#bad\nz: 1 \n"),
    ("order.yaml", "b: 1\na: 2\nnum: nan\n...\n"),
    ("molecule/m.yaml", "x: on\n"),
    ("files/core/c.yaml", "x: on\n"),
    ("files/core/.github/w.yaml", "x: on\n"),
];

const KITCHEN_SINK: &str = "extends: default
yaml-files:
  - '*.yaml'
  - '*.yml'
  - '!**/molecule/**'
ignore:
  - 'files/core/'
  - '!files/core/.github/'
locale: C.UTF-8
rules:
  truthy:
    allowed-values: ['true', 'false', 'on']
    check-keys: false
    level: error
  key-ordering:
    ignore: |
      files/
  quoted-strings:
    quote-type: double
    required: only-when-needed
    extra-allowed: ['^http']
  float-values:
    require-numeral-before-decimal: true
    forbid-nan: true
  empty-values: {forbid-in-block-mappings: true, forbid-in-flow-mappings: true}
  octal-values: enable
  line-length:
    max: 60
    allow-non-breakable-words: false
    ignore-from-file: .lineignore
  comments: {min-spaces-from-content: 1, ignore-shebangs: false}
  document-start: {present: false}
  document-end: {present: true, level: warning}
  indentation: {spaces: 4, indent-sequences: whatever}
  anchors: {forbid-unused-anchors: true}
  hyphens: false
  empty-lines: {max: 1, max-end: 0}
  colons: {max-spaces-after: -1}
";

fn sorted_findings(home: &Path, project: &Path) -> Vec<String> {
    let (code, stdout, stderr) = run(ryl(home)
        .current_dir(project)
        .args(["check", "-f", "parsable", "."]));
    assert!(code <= 1, "stdout={stdout} stderr={stderr}");
    let mut lines: Vec<String> = format!("{stdout}{stderr}")
        .lines()
        .filter(|line| line.contains(": ["))
        .map(str::to_owned)
        .collect();
    lines.sort();
    lines
}

fn assert_migration_preserves_findings(config: &str) {
    let home = tempdir().unwrap();
    let project = home.path().join("project");
    for (name, text) in CORPUS {
        let path = project.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    fs::write(project.join(".lineignore"), "long.yaml\n").unwrap();
    fs::write(project.join(".yamllint"), config).unwrap();
    let before = sorted_findings(home.path(), &project);
    assert!(
        !before.is_empty(),
        "the corpus must trip the config: {config}"
    );
    let (code, stdout, stderr) = run(ryl(home.path())
        .args(["--migrate-configs", "--migrate-write", "--migrate-root"])
        .arg(&project));
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(project.join(".ryl.toml").exists(), "{stdout}");
    assert_eq!(sorted_findings(home.path(), &project), before, "{config}");
}

#[test]
fn default_preset_migrates_to_equivalent_toml() {
    assert_migration_preserves_findings("extends: default\n");
}

#[test]
fn relaxed_preset_migrates_to_equivalent_toml() {
    assert_migration_preserves_findings("extends: relaxed\n");
}

#[test]
fn empty_preset_with_one_rule_migrates_to_equivalent_toml() {
    assert_migration_preserves_findings(
        "extends: empty\nrules:\n  key-duplicates: enable\n",
    );
}

#[test]
fn every_option_kind_migrates_to_equivalent_toml() {
    assert_migration_preserves_findings(KITCHEN_SINK);
}

#[test]
fn user_global_relative_ignore_from_file_is_refused_not_migrated() {
    let home = tempdir().unwrap();
    let xdg = home.path().join("xdg");
    fs::create_dir_all(xdg.join("yamllint")).unwrap();
    fs::create_dir_all(home.path().join("sub")).unwrap();
    fs::write(
        xdg.join("yamllint").join("config"),
        "rules: {key-duplicates: enable}\nignore-from-file: ignores.txt\n",
    )
    .unwrap();
    // Runtime reads `sub/ignores.txt` for `sub/a.yaml`, but a migration from the working
    // directory would inline `ignores.txt` instead.
    fs::write(home.path().join("ignores.txt"), "root.yaml\n").unwrap();
    fs::write(home.path().join("sub").join("ignores.txt"), "a.yaml\n").unwrap();
    fs::write(home.path().join("sub").join("a.yaml"), "a: 1\na: 2\n").unwrap();
    let check = || {
        run(ryl(home.path())
            .current_dir(home.path())
            .env("XDG_CONFIG_HOME", &xdg)
            .args(["check", "-f", "parsable", "sub/a.yaml"]))
    };
    assert_eq!(
        check().0,
        0,
        "the nested ignore file applies before migration"
    );
    let (code, stdout, stderr) = run(ryl(home.path())
        .current_dir(home.path())
        .env("XDG_CONFIG_HOME", &xdg)
        .args(["--migrate-user-config", "--migrate-write"]));
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(
        stderr.contains("relative ignore-from-file `ignores.txt`"),
        "{stderr}"
    );
    assert!(!xdg.join("ryl").join("ryl.toml").exists());
    assert_eq!(check().0, 0, "the refused config still governs the run");
}
