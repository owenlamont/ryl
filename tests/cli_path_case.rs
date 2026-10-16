use std::io::Write;
use std::process::Stdio;

#[path = "common/mod.rs"]
mod common;

#[test]
fn cli_path_case_policy_applies_to_check_fix_format_and_stdin() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("SKIP")).unwrap();
    std::fs::write(
        dir.path().join(".ryl.toml"),
        "exclude = ['skip/**']\n[lint.rules.trailing-spaces]\n",
    )
    .unwrap();
    let path = dir.path().join("SKIP/x.yaml");
    let insensitive = cfg!(any(windows, target_os = "macos"));
    for args in [vec!["check"], vec!["check", "--fix"], vec!["format"]] {
        std::fs::write(&path, "a: [1,2]  \n").unwrap();
        let result = common::cli::ryl(dir.path())
            .args(&args)
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(
            result.status.code(),
            Some(i32::from(
                !insensitive && args.len() == 1 && args[0] == "check"
            ))
        );
        let expected = if insensitive || args == ["check"] {
            "a: [1,2]  \n"
        } else if args[0] == "format" {
            "a: [1, 2]\n"
        } else {
            "a: [1,2]\n"
        };
        assert_eq!(std::fs::read_to_string(&path).unwrap(), expected);
    }
    for command in ["check", "format"] {
        let mut child = common::cli::ryl(dir.path())
            .args([command, "--stdin-filename"])
            .arg(&path)
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"a: [1,2]  \n")
            .unwrap();
        let result = child.wait_with_output().unwrap();
        assert_eq!(
            result.status.code(),
            Some(i32::from(!insensitive && command == "check"))
        );
        if command == "format" && !insensitive {
            assert_eq!(result.stdout, b"a: [1, 2]\n");
        }
    }
    let path = dir.path().join("x.YAML");
    std::fs::write(&path, "a: 1\n").unwrap();
    for command in ["check", "format"] {
        let result = common::cli::ryl(dir.path())
            .arg(command)
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(if insensitive { 0 } else { 2 }));
    }
}
