use std::process::Command;

pub fn run(cmd: &mut Command) -> (i32, String, String) {
    let out = cmd.output().expect("process");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
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
    // Normalize line endings to LF for cross-platform compatibility
    output.replace("\r\n", "\n")
}

pub fn capture_with_env(mut cmd: Command, envs: &[(&str, Option<&str>)]) -> (i32, String) {
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
