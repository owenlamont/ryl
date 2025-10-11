use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::tempdir;

fn write_utf16le(path: &Path, content: &str) {
    let mut data = Vec::with_capacity(2 + content.len() * 2);
    data.extend_from_slice(&[0xFF, 0xFE]);
    for unit in content.encode_utf16() {
        let bytes = unit.to_le_bytes();
        data.extend_from_slice(&bytes);
    }
    fs::write(path, data).unwrap();
}

fn write_latin1(path: &Path, content: &str) {
    let mut data = Vec::with_capacity(content.len());
    for ch in content.chars() {
        let code = ch as u32;
        assert!(
            code <= 0xFF,
            "latin-1 helper only supports code points <= 0xFF"
        );
        data.push(code as u8);
    }
    fs::write(path, data).unwrap();
}

fn write_utf32le(path: &Path, content: &str) {
    let mut data = Vec::with_capacity(4 + content.len() * 4);
    data.extend_from_slice(&[0xFF, 0xFE, 0x00, 0x00]);
    for ch in content.chars() {
        let code = ch as u32;
        data.extend_from_slice(&code.to_le_bytes());
    }
    fs::write(path, data).unwrap();
}

fn run(cmd: &mut Command) -> (i32, String, String) {
    let out = cmd.output().expect("failed to run command");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

#[test]
fn cli_reads_utf16_config_file() {
    let dir = tempdir().unwrap();
    let cfg_path = dir.path().join("utf16-config.yml");
    let yaml_path = dir.path().join("bad.yaml");

    write_utf16le(&cfg_path, "rules:\n  truthy: enable\n");
    fs::write(&yaml_path, "value: Yes\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("-c").arg(&cfg_path).arg(&yaml_path));
    assert_eq!(
        code, 1,
        "expected lint failures: stdout={stdout} stderr={stderr}"
    );
    let output = if stderr.is_empty() { stdout } else { stderr };
    assert!(
        output.contains("truthy"),
        "expected truthy diagnostics in output, got:\n{output}"
    );
}

#[test]
fn cli_reads_utf16_yaml_input() {
    let dir = tempdir().unwrap();
    let cfg_path = dir.path().join("config.yml");
    let yaml_path = dir.path().join("utf16.yaml");

    write_utf16le(&cfg_path, "rules:\n  truthy: enable\n");
    write_utf16le(&yaml_path, "value: Yes\n");

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("-c").arg(&cfg_path).arg(&yaml_path));
    assert_eq!(
        code, 1,
        "expected lint failures: stdout={stdout} stderr={stderr}"
    );
    let output = if stderr.is_empty() { stdout } else { stderr };
    assert!(
        output.contains("truthy"),
        "expected truthy diagnostics in output, got:\n{output}"
    );
}

#[test]
fn cli_honors_yamllint_file_encoding_override() {
    let dir = tempdir().unwrap();
    let cfg_path = dir.path().join("latin-config.yml");
    let yaml_path = dir.path().join("latin.yaml");

    write_latin1(&cfg_path, "rules:\n  truthy: enable\n");
    write_latin1(&yaml_path, "acción: yes\n");

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .env("YAMLLINT_FILE_ENCODING", "latin-1")
        .arg("-c")
        .arg(&cfg_path)
        .arg(&yaml_path));

    assert_eq!(
        code, 1,
        "expected lint failures: stdout={stdout} stderr={stderr}"
    );
    let message = if stderr.is_empty() {
        stdout.clone()
    } else {
        stderr.clone()
    };
    assert!(
        message.contains("truthy"),
        "expected truthy diagnostics in output, got:\n{message}"
    );
    assert!(
        stderr.contains("YAMLLINT_FILE_ENCODING is meant for temporary workarounds"),
        "expected override warning on stderr, got:\n{stderr}"
    );
}

#[test]
fn cli_override_utf16_variants() {
    let dir = tempdir().unwrap();
    let yaml_path = dir.path().join("data.yaml");

    write_utf16le(&yaml_path, "value: Yes\n");

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .env("YAMLLINT_FILE_ENCODING", "UTF_16")
        .arg("--config-data")
        .arg("rules:\n  truthy: enable\n")
        .arg(&yaml_path));

    assert_eq!(
        code, 1,
        "expected lint failures: stdout={stdout} stderr={stderr}"
    );
    let message = if stderr.is_empty() { stdout } else { stderr };
    assert!(
        message.contains("truthy"),
        "expected truthy diagnostics, got:\n{message}"
    );
}

#[test]
fn cli_override_utf32_variants() {
    let dir = tempdir().unwrap();
    let yaml_path = dir.path().join("utf32.yaml");

    write_utf32le(&yaml_path, "value: Yes\n");

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .env("YAMLLINT_FILE_ENCODING", "utf32le")
        .arg("--config-data")
        .arg("rules:\n  truthy: enable\n")
        .arg(&yaml_path));

    assert_eq!(
        code, 1,
        "expected lint failures: stdout={stdout} stderr={stderr}"
    );
    let message = if stderr.is_empty() { stdout } else { stderr };
    assert!(
        message.contains("truthy"),
        "expected truthy diagnostics, got:\n{message}"
    );
}

#[test]
fn cli_override_unknown_label_errors() {
    let dir = tempdir().unwrap();
    let cfg_path = dir.path().join("config.yml");
    let yaml_path = dir.path().join("file.yaml");

    fs::write(&cfg_path, "rules:\n  truthy: enable\n").unwrap();
    fs::write(&yaml_path, "value: Yes\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .env("YAMLLINT_FILE_ENCODING", "unsupported-encoding")
        .arg("-c")
        .arg(&cfg_path)
        .arg(&yaml_path));

    assert_eq!(
        code, 2,
        "expected usage error: stdout={stdout} stderr={stderr}"
    );
    assert!(
        stderr.contains("unsupported label"),
        "expected unsupported label error, got:\n{stderr}"
    );
}
