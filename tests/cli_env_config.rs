use std::fs;
use std::process::Command;

use tempfile::tempdir;

fn run(cmd: &mut Command) -> (i32, String, String) {
    let out = cmd.output().expect("process");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

#[test]
fn env_config_file_is_honored() {
    let dir = tempdir().unwrap();
    let cfg = dir.path().join("env-config.yml");
    fs::write(
        &cfg,
        "rules:\n  new-line-at-end-of-file: disable\n  document-start: disable\n",
    )
    .unwrap();
    let file = dir.path().join("no_newline.yaml");
    fs::write(&file, "key: value").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .env("YAMLLINT_CONFIG_FILE", cfg.display().to_string())
        .arg(&file));
    assert_eq!(
        code, 0,
        "env-config should disable rule: stdout={stdout} stderr={stderr}"
    );
}
