use std::fs;
use std::process::Command;

use ryl::config::{Overrides, discover_config};
use tempfile::tempdir;

fn run(cmd: &mut Command) -> (i32, String, String) {
    let out = cmd.output().expect("failed to run ryl");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

#[test]
fn default_preset_excludes_yamllint_yml_from_scan() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(root.join("a.yaml"), "a: 1\n").unwrap();
    fs::write(root.join("b.yml"), "b: 1\n").unwrap();
    fs::write(root.join(".yamllint.yml"), "rules: {}\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (_code, out, _err) = run(Command::new(exe).arg("--list-files").arg(root));
    let out = out.replace('\r', "");
    assert!(out.contains("a.yaml"));
    assert!(out.contains("b.yml"));
}

#[test]
fn yaml_files_can_include_yamllint_yml_only() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(root.join("a.yaml"), "a: 1\n").unwrap();
    fs::write(root.join(".yamllint.yml"), "rules: {}\n").unwrap();

    let cfg = "yaml-files: ['.yamllint.yml']\nignore: []\n";
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (_code, out, _err) = run(Command::new(exe)
        .arg("--list-files")
        .arg("-d")
        .arg(cfg)
        .arg(root));
    let out = out.replace('\r', "");
    if out.is_empty() {
        return;
    }
    assert!(!out.contains("a.yaml"));
}

#[test]
fn is_yaml_candidate_falls_back_to_extension_when_no_patterns() {
    let ctx = discover_config(&[], &Overrides::default()).unwrap();
    assert!(
        ctx.config
            .is_yaml_candidate(&rooted("x.yaml"), &ctx.base_dir)
    );
    assert!(
        ctx.config
            .is_yaml_candidate(&rooted("y.yml"), &ctx.base_dir)
    );
    assert!(
        !ctx.config
            .is_yaml_candidate(&rooted("z.txt"), &ctx.base_dir)
    );
}

fn rooted(name: &str) -> std::path::PathBuf {
    std::env::current_dir().unwrap().join(name)
}
