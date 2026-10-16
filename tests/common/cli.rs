use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, Output, Stdio};

/// Bound project-config discovery at `home`.
#[allow(dead_code, reason = "not every test binary uses every helper")]
pub fn ryl(home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ryl"));
    cmd.env("HOME", home);
    cmd
}

/// Run `cmd` to completion, returning `(exit code, stdout, stderr)`.
#[allow(dead_code, reason = "not every test binary uses every helper")]
pub fn run(cmd: &mut Command) -> (i32, String, String) {
    let out = cmd.output().expect("process");
    output_tuple(out)
}

#[allow(dead_code, reason = "not every test binary uses every helper")]
pub fn output_tuple(out: Output) -> (i32, String, String) {
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy_owned(out.stdout);
    let stderr = String::from_utf8_lossy_owned(out.stderr);
    (code, stdout, stderr)
}

/// Whichever stream carried the diagnostics: `stderr` when non-empty (ryl prints
/// diagnostics there), otherwise `stdout`.
#[allow(dead_code, reason = "not every test binary uses every helper")]
pub fn command_output<'a>(stdout: &'a str, stderr: &'a str) -> &'a str {
    if stderr.is_empty() { stdout } else { stderr }
}

#[allow(dead_code, reason = "not every test binary uses every helper")]
pub fn stdin_output(
    cmd: &mut Command,
    input: &[u8],
    check_write: impl FnOnce(io::Result<()>),
) -> Output {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ryl");
    check_write(child.stdin.take().expect("stdin").write_all(input));
    child.wait_with_output().expect("wait")
}

#[allow(dead_code, reason = "not every test binary uses every helper")]
pub fn ryl_on(
    config: Option<&str>,
    input: &str,
    args: &[&str],
) -> (i32, String, String, String) {
    let dir = tempfile::tempdir().unwrap();
    if let Some(config) = config {
        fs::write(dir.path().join(".ryl.toml"), config).unwrap();
    }
    let file = dir.path().join(if args.contains(&"--markdown") {
        "a.md"
    } else {
        "a.yaml"
    });
    fs::write(&file, input).unwrap();
    let (code, stdout, stderr) = run(ryl(dir.path()).args(args).arg(&file));
    (code, stdout, stderr, fs::read_to_string(&file).unwrap())
}
