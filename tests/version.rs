use semver::Version;
use std::process::Command;

#[test]
fn cli_prints_name_and_semver_version() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let output = Command::new(exe)
        .arg("--version")
        .output()
        .expect("failed to run ryl binary");

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.trim();
    assert!(line.starts_with("ryl "), "unexpected output: {line} ");

    let ver = line.trim_start_matches("ryl ").trim();
    let v = Version::parse(ver).expect("invalid semver version");
    assert!(
        v.pre.is_empty() && v.build.is_empty(),
        "expected plain X.Y.Z with no pre-release/build: {ver}"
    );
}
