use std::fs;

use tempfile::tempdir;

use crate::common;
use common::cli::{command_output, run, run_with_stdin, ryl};

#[test]
fn relative_input_from_subdirectory_finds_ancestor_config() {
    let root = tempdir().unwrap();
    let sub = root.path().join("sub");
    fs::create_dir(&sub).unwrap();
    fs::write(
        root.path().join(".ryl.toml"),
        "[lint.rules.truthy]\nlevel = \"error\"\n",
    )
    .unwrap();
    fs::write(sub.join("a.yml"), "---\non: push\n").unwrap();

    for input in [".", "a.yml", "./a.yml"] {
        let (code, stdout, stderr) =
            run(ryl(root.path()).current_dir(&sub).args(["check", input]));
        assert_eq!(code, 1, "input {input}: stdout={stdout} stderr={stderr}");
        let output = command_output(&stdout, &stderr);
        assert!(output.contains("truthy"), "input {input}: {output}");
    }
}

#[test]
fn anchored_exclusions_normalize_config_and_input_spellings() {
    let root = tempdir().unwrap();
    fs::create_dir_all(root.path().join("sub/sub")).unwrap();
    fs::write(
        root.path().join("cfg.toml"),
        "exclude = ['/skip.yaml']\n[lint.rules.trailing-spaces]\n",
    )
    .unwrap();
    fs::write(
        root.path().join("sub/cfg.toml"),
        "exclude = ['/sub/skip.yaml']\n[lint.rules.trailing-spaces]\n",
    )
    .unwrap();
    let input = "a:    b  \n";
    for name in ["skip.yaml", "sub/sub/skip.yaml", "keep.yaml"] {
        fs::write(root.path().join(name), input).unwrap();
    }
    let absolute = |name: &str| root.path().join(name).display().to_string();
    let skip = absolute("skip.yaml");
    let nested = absolute("sub/sub/skip.yaml");
    let config = absolute("cfg.toml");
    for (mode, config, filename, stdin, expected) in [
        ("check", "cfg.toml", skip.as_str(), false, 0),
        ("format", "cfg.toml", skip.as_str(), false, 0),
        ("check", config.as_str(), "sub/../skip.yaml", false, 0),
        ("check", "sub/cfg.toml", "sub/sub/skip.yaml", false, 0),
        ("check", "sub/cfg.toml", nested.as_str(), false, 0),
        ("check", "sub/cfg.toml", "sub/sub/./skip.yaml", false, 0),
        ("check", config.as_str(), "sub//../skip.yaml", false, 0),
        ("check", "cfg.toml", skip.as_str(), true, 0),
        ("check", "cfg.toml", "keep.yaml", false, 1),
        ("format", "cfg.toml", "keep.yaml", false, 1),
    ] {
        let mut cmd = ryl(root.path());
        cmd.current_dir(root.path()).args([mode, "-c", config]);
        if mode == "format" {
            cmd.args(["--no-cache", "--check"]);
        }
        let (code, stdout, stderr) = if stdin {
            run_with_stdin(
                cmd.args(["--stdin-filename", filename, "-"]),
                input.as_bytes(),
            )
        } else {
            run(cmd.arg(filename))
        };
        assert_eq!(
            code, expected,
            "{mode} -c {config} {filename} stdin={stdin}: {stdout} {stderr}"
        );
    }
}
