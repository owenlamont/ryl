use std::fs;
use std::process::Command;

use tempfile::tempdir;

fn run(cmd: &mut Command) -> (i32, String, String) {
    let out = cmd.output().expect("failed to run helper");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

#[test]
fn dc_error_on_env_config_parse_error() {
    let td = tempdir().unwrap();
    let cfg = td.path().join("envcfg.yml");
    fs::write(&cfg, "rules: {\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_discover_config_bin");
    let (code, _out, err) = run(Command::new(exe)
        .env("YAMLLINT_CONFIG_FILE", &cfg)
        .arg(td.path()));
    assert_eq!(code, 2, "expected exit 2: {err}");
    assert!(err.contains("failed to parse config data"));
}
