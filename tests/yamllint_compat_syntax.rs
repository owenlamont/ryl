use std::fs;
use std::path::PathBuf;
use std::process::Command;

use tempfile::tempdir;

fn run_cmd(cmd: &mut Command) -> (i32, String, String) {
    let out = cmd.output().expect("failed to spawn process");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

fn run_with_env(mut cmd: Command, envs: &[(&str, Option<&str>)]) -> (i32, String, String) {
    cmd.env_remove("GITHUB_ACTIONS");
    cmd.env_remove("GITHUB_WORKFLOW");
    cmd.env_remove("CI");
    for (key, value) in envs {
        if let Some(val) = value {
            cmd.env(key, val);
        } else {
            cmd.env_remove(key);
        }
    }
    run_cmd(&mut cmd)
}

fn write_file(dir: &std::path::Path, name: &str, content: &str) -> PathBuf {
    let p = dir.join(name);
    fs::write(&p, content).expect("write file");
    p
}

fn ensure_yamllint_installed() {
    let ok = Command::new("yamllint")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(
        ok,
        "yamllint must be installed and in PATH for parity tests"
    );
}

#[test]
fn yamllint_exit_behavior_matches_for_syntax_only() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();
    let ok = write_file(dir.path(), "ok.yaml", "a: 1\n");
    let bad = write_file(dir.path(), "bad.yaml", "a: [1, 2\n");

    // Disable yamllint rules so we only compare syntax behavior.
    let cfg = write_file(dir.path(), ".yamllint.yml", "rules: {}\n");

    let ryl = env!("CARGO_BIN_EXE_ryl");

    const STANDARD_ENV: &[(&str, Option<&str>)] = &[];
    const GITHUB_ENV: &[(&str, Option<&str>)] = &[
        ("GITHUB_ACTIONS", Some("true")),
        ("GITHUB_WORKFLOW", Some("test-workflow")),
        ("CI", Some("true")),
    ];

    #[derive(Clone, Copy)]
    enum OutputKind {
        Standard,
        Github,
    }

    fn parse_loc(msg: &str, kind: OutputKind) -> Option<(String, usize, usize)> {
        match kind {
            OutputKind::Standard => {
                let mut lines = msg.lines().filter(|l| !l.trim().is_empty());
                let file = lines.next()?.trim().to_string();
                let second = lines.next()?.trim_start();
                let pos = second.split_whitespace().next()?;
                let mut it = pos.split(':');
                let line = it.next()?.parse().ok()?;
                let col = it.next()?.parse().ok()?;
                Some((file, line, col))
            }
            OutputKind::Github => {
                let mut lines = msg.lines().filter(|l| !l.trim().is_empty());
                let header = lines.next()?.trim();
                let file = header.strip_prefix("::group::")?.to_string();
                let detail = lines.next()?.trim();
                let detail = detail.strip_prefix("::error ")?;
                let mut parts = detail.splitn(2, "::");
                let meta = parts.next()?; // file=...,line=...,col=...
                let mut meta_parts = meta.split(',');
                let file_part = meta_parts.next()?;
                let line_part = meta_parts.next()?;
                let col_part = meta_parts.next()?;
                let file_path = file_part.strip_prefix("file=")?;
                let line_str = line_part.strip_prefix("line=")?;
                let col_str = col_part.strip_prefix("col=")?;
                let line = line_str.parse().ok()?;
                let col = col_str.parse().ok()?;
                // Ensure metadata file matches header file after trimming.
                if !file_path.ends_with(&file) {
                    return None;
                }
                Some((file_path.to_string(), line, col))
            }
        }
    }

    let scenarios = [
        ("standard", STANDARD_ENV, OutputKind::Standard, true),
        ("github", GITHUB_ENV, OutputKind::Github, false),
    ];

    for (label, envs, kind, use_standard_format) in scenarios {
        // Valid file should pass in both tools.
        let mut ryl_ok_cmd = Command::new(ryl);
        ryl_ok_cmd.arg(&ok);
        let (ryl_ok, _, _) = run_with_env(ryl_ok_cmd, envs);
        assert_eq!(ryl_ok, 0, "ryl should succeed for valid yaml ({label})");

        let mut yam_ok_cmd = Command::new("yamllint");
        if use_standard_format {
            yam_ok_cmd.arg("-f").arg("standard");
        }
        yam_ok_cmd.arg("-c");
        yam_ok_cmd.arg(&cfg);
        yam_ok_cmd.arg(&ok);
        let (yam_ok, _, yam_ok_err) = run_with_env(yam_ok_cmd, envs);
        assert_eq!(
            yam_ok, 0,
            "yamllint should succeed for valid yaml ({label}): {yam_ok_err}"
        );

        // Invalid file should fail; capture whichever stream has content.
        let mut ryl_bad_cmd = Command::new(ryl);
        ryl_bad_cmd.arg(&bad);
        let (ryl_bad_code, ryl_bad_out, ryl_bad_err) = run_with_env(ryl_bad_cmd, envs);
        let mut yam_bad_cmd = Command::new("yamllint");
        if use_standard_format {
            yam_bad_cmd.arg("-f").arg("standard");
        }
        yam_bad_cmd.arg("-c");
        yam_bad_cmd.arg(&cfg);
        yam_bad_cmd.arg(&bad);
        let (yam_bad_code, yam_bad_out, yam_bad_err) = run_with_env(yam_bad_cmd, envs);

        assert_ne!(
            ryl_bad_code, 0,
            "ryl should fail for invalid yaml ({label})"
        );
        assert_ne!(
            yam_bad_code, 0,
            "yamllint should fail for invalid yaml ({label})"
        );

        let r_msg = if !ryl_bad_err.is_empty() {
            ryl_bad_err
        } else {
            ryl_bad_out
        };
        let y_msg = if !yam_bad_out.is_empty() {
            yam_bad_out
        } else {
            yam_bad_err
        };

        if let (Some((rf, rl, rc)), Some((yf, yl, yc))) =
            (parse_loc(&r_msg, kind), parse_loc(&y_msg, kind))
        {
            assert!(
                rf.ends_with("bad.yaml"),
                "ryl location should reference bad.yaml ({label}): {rf}"
            );
            assert!(
                yf.ends_with("bad.yaml"),
                "yamllint location should reference bad.yaml ({label}): {yf}"
            );
            assert_eq!(rl, yl, "line numbers should match ({label})");
            assert_eq!(rc, yc, "column numbers should match ({label})");
        } else {
            panic!("could not parse location ({label})\nryl:\n{r_msg}\nyamllint:\n{y_msg}");
        }

        assert!(
            r_msg.contains("syntax error"),
            "ryl output should mention syntax error ({label}): {r_msg}"
        );
        assert!(
            y_msg.contains("syntax error"),
            "yamllint output should mention syntax error ({label}): {y_msg}"
        );
    }
}

#[test]
fn flow_style_multiline_sequence_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();
    let file = write_file(
        dir.path(),
        "flow.yaml",
        "---\nroot:\n  command: [\n    'mysqld',\n    '--option',\n  ]\n",
    );

    let ryl = env!("CARGO_BIN_EXE_ryl");
    let mut ryl_cmd = Command::new(ryl);
    ryl_cmd.arg(&file);
    let (ryl_code, ryl_out, ryl_err) = run_cmd(&mut ryl_cmd);
    assert_eq!(ryl_code, 0, "ryl reported diagnostics: {ryl_out}{ryl_err}");

    let mut yam_cmd = Command::new("yamllint");
    yam_cmd.arg(&file);
    let (yam_code, yam_out, yam_err) = run_cmd(&mut yam_cmd);
    assert_eq!(
        yam_code, 0,
        "yamllint reported diagnostics: {yam_out}{yam_err}"
    );
}

#[test]
fn double_quoted_multiline_scalar_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();
    let cfg = write_file(
        dir.path(),
        "cfg.yaml",
        "rules:\n  document-start: disable\n  line-length: disable\n",
    );
    let file = write_file(
        dir.path(),
        "quoted.yaml",
        "value: \"CreateContainerConfigError,ErrImagePull,CreateContainerError,ImageInspectError,\\\n        InvalidImageName\"\n",
    );

    let ryl = env!("CARGO_BIN_EXE_ryl");
    let mut ryl_cmd = Command::new(ryl);
    ryl_cmd.arg("-c").arg(&cfg).arg(&file);
    let (ryl_code, ryl_out, ryl_err) = run_cmd(&mut ryl_cmd);
    assert_eq!(ryl_code, 0, "ryl reported diagnostics: {ryl_out}{ryl_err}");

    let mut yam_cmd = Command::new("yamllint");
    yam_cmd.arg("-c").arg(&cfg).arg(&file);
    let (yam_code, yam_out, yam_err) = run_cmd(&mut yam_cmd);
    assert_eq!(
        yam_code, 0,
        "yamllint reported diagnostics: {yam_out}{yam_err}"
    );
}
