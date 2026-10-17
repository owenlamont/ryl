use std::fs;
use std::process::Command;

use tempfile::tempdir;

mod common;
use common::cli::ryl;

#[test]
fn format_diff_keeps_input_order_across_many_files() {
    let dir = tempdir().unwrap();
    let files: Vec<_> = (0..64)
        .map(|index| {
            let path = dir.path().join(format!("{index:03}.yaml"));
            fs::write(&path, format!("key{index}:  value\n")).unwrap();
            path
        })
        .collect();
    let mut expected = Vec::new();
    for path in files.iter().rev() {
        let output = ryl(dir.path())
            .args(["format", "--diff", "--no-warnings"])
            .arg(path)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        expected.extend(output.stdout);
    }

    let output = ryl(dir.path())
        .args(["format", "--diff", "--no-warnings"])
        .args(files.iter().rev())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        output.stdout == expected,
        "parallel diff order differs from serial output"
    );
}

#[test]
fn check_fix_reads_hard_link_after_prior_write() {
    let dir = tempdir().unwrap();
    let first = dir.path().join("first");
    let second = dir.path().join("second");
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&second).unwrap();
    fs::write(first.join(".ryl.toml"), "[lint.rules]\ncolons = 'enable'\n").unwrap();
    fs::write(
        second.join(".ryl.toml"),
        "[lint.rules]\ntrailing-spaces = 'enable'\n",
    )
    .unwrap();
    let first_file = first.join("shared.yaml");
    let second_file = second.join("shared.yaml");
    fs::write(&first_file, "key:  value  \n").unwrap();
    fs::hard_link(&first_file, &second_file).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_ryl"))
        .args(["check", "--fix"])
        .arg(&first_file)
        .arg(&second_file)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(fs::read_to_string(&first_file).unwrap(), "key: value\n");
    assert_eq!(fs::read_to_string(&second_file).unwrap(), "key: value\n");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("2 fixed, 0 remaining"),
        "{output:?}"
    );
}

#[cfg(unix)]
#[test]
fn format_keeps_symlink_and_encoding_warnings_in_input_order() {
    use std::os::unix::fs::symlink;

    for mode in [
        &["format", "--no-warnings"][..],
        &["format", "--diff", "--no-warnings"],
    ] {
        let dir = tempdir().unwrap();
        let target = dir.path().join("target.yaml");
        let link = dir.path().join("link.yaml");
        let regular = dir.path().join("regular.yaml");
        fs::write(&target, "key:  value\n").unwrap();
        fs::write(&regular, "key:  value\n").unwrap();
        symlink(&target, &link).unwrap();

        let output = ryl(dir.path())
            .args(mode)
            .env("YAMLLINT_FILE_ENCODING", "utf-8")
            .arg(&link)
            .arg(&regular)
            .output()
            .unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        let symlink_notice = stderr.find("refusing to follow a symlink").unwrap();
        let encoding_notice = stderr.find("YAMLLINT_FILE_ENCODING").unwrap();

        assert!(
            symlink_notice < encoding_notice,
            "{mode:?} warnings are out of input order: {stderr}"
        );
    }
}

#[cfg(unix)]
#[test]
fn rewrite_notices_follow_input_order_with_read_parse_and_symlink_skips() {
    use std::os::unix::fs::symlink;

    for mode in [
        &["format", "--no-warnings"][..],
        &["check", "--fix", "--no-warnings"],
        &["format", "--diff", "--no-warnings"],
        &["check", "--diff", "--no-warnings"],
    ] {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join(".ryl.toml"),
            "[lint.rules]\ncolons = 'enable'\n",
        )
        .unwrap();
        let invalid = dir.path().join("invalid.yaml");
        let parse = dir.path().join("parse.yaml");
        let valid = dir.path().join("valid.yaml");
        let link = dir.path().join("link.yaml");
        fs::write(&invalid, [0xff]).unwrap();
        fs::write(&parse, "a: [\n").unwrap();
        fs::write(&valid, "k:  v\n").unwrap();
        symlink(&valid, &link).unwrap();

        let orders: Vec<Vec<_>> = if mode.contains(&"--diff") {
            vec![
                vec![&parse, &link, &valid, &invalid],
                vec![&link, &parse, &valid, &invalid],
            ]
        } else {
            vec![
                vec![&invalid, &link, &parse, &valid],
                vec![&parse, &link, &valid, &invalid],
            ]
        };
        for order in orders {
            let output = ryl(dir.path()).args(mode).args(&order).output().unwrap();
            let stderr = String::from_utf8(output.stderr).unwrap();
            let expected: Vec<String> = order
                .iter()
                .filter_map(|path| {
                    if **path == invalid && !mode.contains(&"--fix") {
                        Some(format!("failed to read {}:", invalid.display()))
                    } else if **path == link {
                        Some("refusing to follow a symlink".to_string())
                    } else if **path == parse {
                        Some("skipped by".to_string())
                    } else {
                        None
                    }
                })
                .collect();
            let positions: Vec<_> = expected
                .iter()
                .filter_map(|notice| stderr.find(notice))
                .collect();
            assert_eq!(positions.len(), expected.len(), "{mode:?}: {stderr}");
            assert!(
                positions.windows(2).all(|pair| pair[0] < pair[1]),
                "{mode:?}: {stderr}"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn fix_read_error_keeps_lint_read_error_of_skipped_symlink() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[lint.rules]\ncolons = 'enable'\n",
    )
    .unwrap();
    fs::write(dir.path().join("invalid.yaml"), [0xff]).unwrap();
    fs::write(dir.path().join("target.yaml"), [0xfe]).unwrap();
    std::os::unix::fs::symlink("target.yaml", dir.path().join("link.yaml")).unwrap();

    let output = ryl(dir.path())
        .current_dir(dir.path())
        .args(["check", "--fix", "invalid.yaml", "link.yaml"])
        .output()
        .unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert_eq!(output.status.code(), Some(2), "{stderr}");
    for path in ["invalid.yaml", "link.yaml"] {
        assert_eq!(
            stderr.matches(&format!("failed to read {path}")).count(),
            1,
            "{stderr}"
        );
    }
    let symlink_notice = stderr.find("refusing to follow a symlink").unwrap();
    let invalid_report = stderr.find("error: failed to read invalid.yaml").unwrap();
    let link_report = stderr.find("error: failed to read link.yaml").unwrap();
    assert!(
        symlink_notice < invalid_report && invalid_report < link_report,
        "{stderr}"
    );
}
