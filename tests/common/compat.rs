use std::process::Command;

pub fn run(cmd: &mut Command) -> (i32, String, String) {
    let is_ryl = cmd.get_program() == env!("CARGO_BIN_EXE_ryl");
    let out = cmd.output().expect("process");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy_owned(out.stdout);
    let stderr = String::from_utf8_lossy_owned(out.stderr);
    if is_ryl {
        return (code, stdout, without_legacy_yaml_notice(&stderr));
    }
    (code, stdout, stderr)
}

/// ryl's stderr minus its one warning that the YAML config is deprecated, which yamllint
/// has no counterpart to.
fn without_legacy_yaml_notice(stderr: &str) -> String {
    let (notices, rest): (Vec<&str>, Vec<&str>) = stderr
        .split_inclusive('\n')
        .partition(|line| line.contains("yamllint YAML config is deprecated"));
    assert_eq!(
        notices.len(),
        1,
        "expected one deprecation notice: {stderr}"
    );
    rest.concat()
}

pub fn ensure_yamllint_installed() {
    let ok = Command::new("yamllint")
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false);
    assert!(ok, "yamllint must be installed for compatibility tests");
}

pub fn normalize_output(stdout: String, stderr: String) -> String {
    let output = if stderr.is_empty() { stdout } else { stderr };
    output
        .replace("\r\n", "\n")
        .split_inclusive('\n')
        .map(|line| {
            if let Some((command, rest)) = line.split_once(" file=")
                && matches!(command, "::error" | "::warning")
                && let Some((path, metadata)) = rest.split_once(",line=")
            {
                format!("{command} file={},line={metadata}", github_file_path(path))
            } else {
                line.to_owned()
            }
        })
        .collect()
}

pub fn github_file_path(path: &str) -> String {
    if path.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        && path.get(1..4) == Some("%3A")
    {
        format!("{}:{}", &path[..1], &path[4..])
    } else {
        path.to_owned()
    }
}

pub fn capture_with_env(
    mut cmd: Command,
    envs: &[(&str, Option<&str>)],
) -> (i32, String) {
    cmd.env_remove("GITHUB_ACTIONS");
    cmd.env_remove("GITHUB_WORKFLOW");
    cmd.env_remove("CI");
    cmd.env_remove("FORCE_COLOR");
    cmd.env_remove("NO_COLOR");
    for (key, value) in envs {
        if let Some(v) = value {
            cmd.env(key, v);
        } else {
            cmd.env_remove(key);
        }
    }
    let (code, stdout, stderr) = run(&mut cmd);
    (code, normalize_output(stdout, stderr))
}

#[derive(Clone, Copy)]
pub struct Scenario {
    pub label: &'static str,
    pub envs: &'static [(&'static str, Option<&'static str>)],
    pub ryl_format: Option<&'static str>,
    pub yam_format: Option<&'static str>,
}

pub const STANDARD_ENV: &[(&str, Option<&str>)] = &[];
pub const GITHUB_ENV: &[(&str, Option<&str>)] = &[
    ("GITHUB_ACTIONS", Some("true")),
    ("GITHUB_WORKFLOW", Some("test-workflow")),
    ("CI", Some("true")),
];

pub const SCENARIOS: &[Scenario] = &[
    Scenario {
        label: "auto-standard",
        envs: STANDARD_ENV,
        ryl_format: None,
        yam_format: None,
    },
    Scenario {
        label: "auto-github",
        envs: GITHUB_ENV,
        ryl_format: None,
        yam_format: None,
    },
    Scenario {
        label: "format-standard",
        envs: STANDARD_ENV,
        ryl_format: Some("standard"),
        yam_format: Some("standard"),
    },
    Scenario {
        label: "format-colored",
        envs: STANDARD_ENV,
        ryl_format: Some("colored"),
        yam_format: Some("colored"),
    },
    Scenario {
        label: "format-github",
        envs: STANDARD_ENV,
        ryl_format: Some("github"),
        yam_format: Some("github"),
    },
    Scenario {
        label: "format-parsable",
        envs: STANDARD_ENV,
        ryl_format: Some("parsable"),
        yam_format: Some("parsable"),
    },
];

pub fn build_ryl_command(exe: &str, format: Option<&str>) -> Command {
    let mut cmd = Command::new(exe);
    cmd.arg("check");
    if let Some(fmt) = format {
        cmd.arg("--format").arg(fmt);
    }
    cmd
}

pub fn build_yamllint_command(format: Option<&str>) -> Command {
    let mut cmd = Command::new("yamllint");
    if let Some(fmt) = format {
        cmd.arg("-f").arg(fmt);
    }
    cmd
}
