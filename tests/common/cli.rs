//! Shared CLI test harness: invoke the `ryl` binary and read its output. The
//! `cli_*` integration tests use these instead of each re-defining `run` /
//! `command_output`. Items are `#[allow(dead_code)]` because every test binary
//! that does `mod common;` compiles the whole module, but each uses only the
//! helpers it needs (the same pattern `fake_env` follows).

use std::process::Command;

/// Run `cmd` to completion, returning `(exit code, stdout, stderr)`.
#[allow(dead_code)]
pub fn run(cmd: &mut Command) -> (i32, String, String) {
    let out = cmd.output().expect("process");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

/// Whichever stream carried the diagnostics: `stderr` when non-empty (ryl prints
/// diagnostics there), otherwise `stdout`.
#[allow(dead_code)]
pub fn command_output<'a>(stdout: &'a str, stderr: &'a str) -> &'a str {
    if stderr.is_empty() { stdout } else { stderr }
}
