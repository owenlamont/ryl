use std::process::Command;

mod common;
use common::cli::run;

/// `--generate-completions <SHELL>` emits a non-empty script (mentioning the
/// `ryl` binary) and exits 0 for every shell clap_complete supports.
#[test]
fn generate_completions_emits_a_script_per_shell() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        let (code, stdout, stderr) =
            run(Command::new(exe).arg("--generate-completions").arg(shell));
        assert_eq!(code, 0, "{shell}: should succeed: stderr={stderr}");
        assert!(
            stderr.trim().is_empty(),
            "{shell}: unexpected stderr: {stderr}"
        );
        assert!(
            stdout.contains("ryl"),
            "{shell}: completion script should mention the binary: {stdout}"
        );
    }
}

/// An unknown shell value is rejected by clap itself (usage error, exit 2).
#[test]
fn generate_completions_rejects_unknown_shell() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run(Command::new(exe)
        .arg("--generate-completions")
        .arg("nonsense-shell"));
    assert_eq!(code, 2, "unknown shell should be a usage error");
    assert!(
        !stderr.trim().is_empty(),
        "clap should explain the invalid value: {stderr}"
    );
}

/// The flag is `exclusive`: pairing it with a lint input or another action is a
/// usage error (exit 2) rather than silently emitting the script and skipping
/// the requested lint/fix.
#[test]
fn generate_completions_rejects_being_combined_with_other_args() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let combos: [&[&str]; 2] = [
        &["--generate-completions", "bash", "ignored.yaml"],
        &["--generate-completions", "bash", "--fix"],
    ];
    for args in combos {
        let (code, _stdout, stderr) = run(Command::new(exe).args(args));
        assert_eq!(code, 2, "combining {args:?} should be a usage error");
        assert!(
            !stderr.trim().is_empty(),
            "clap should explain the conflict for {args:?}"
        );
    }
}
