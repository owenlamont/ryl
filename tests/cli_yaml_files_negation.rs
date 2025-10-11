use std::fs;
use std::process::Command;

use tempfile::tempdir;

fn run(cmd: &mut Command) -> (i32, String, String) {
    let out = cmd.output().expect("failed to run command");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

#[test]
fn yaml_files_negation_excludes_files_via_cli() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let cfg_path = root.join("config.yml");

    fs::write(
        &cfg_path,
        "rules:\n  truthy: enable\nyaml-files: ['*.yaml', '!skip.yaml']\n",
    )
    .unwrap();
    fs::write(root.join("keep.yaml"), "value: Yes\n").unwrap();
    fs::write(root.join("skip.yaml"), "value: Yes\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("-c").arg(&cfg_path).arg(root));

    assert_eq!(
        code, 1,
        "expected lint failure: stdout={stdout} stderr={stderr}"
    );
    let output = if stderr.is_empty() { stdout } else { stderr };
    assert!(
        output.contains("keep.yaml"),
        "expected keep.yaml diagnostics, got:\n{output}"
    );
    assert!(
        !output.contains("skip.yaml"),
        "unexpected diagnostics for skip.yaml:\n{output}"
    );
}
