//! Shared CLI test harness: invoke the `ryl` binary and read its output. The
//! `cli_*` integration tests use these instead of each re-defining `run` /
//! `command_output`. Items are `#[allow(dead_code)]` because every test binary
//! that does `mod common;` compiles the whole module, but each uses only the
//! helpers it needs (the same pattern `fake_env` follows).

use std::path::Path;
use std::process::Command;

/// A `ryl` command whose config discovery is isolated from the shared temp root. Project
/// discovery climbs from each input through its ancestors up to `HOME`, so a test whose
/// inputs live under the system temp dir would otherwise walk into that shared dir, where
/// a stray `ryl.toml`/`.yamllint`/etc. (left by another test, a concurrent process, or a
/// manual smoke run) gets discovered and silently overrides the test's own setup — and
/// TOML candidates outrank a tempdir's `.yamllint`, so an adjacent YAML config is not
/// enough to shield it. Setting `HOME` to `home` stops the walk there. Any test that
/// exercises discovery (does NOT pass `-c`/`-d`, and has no adjacent TOML config in its
/// input's directory) must build its command with this, passing its own tempdir.
#[allow(dead_code)]
pub fn ryl(home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ryl"));
    cmd.env("HOME", home);
    cmd
}

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
