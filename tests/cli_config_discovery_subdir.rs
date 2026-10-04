use std::fs;

use tempfile::tempdir;

mod common;
use common::cli::{command_output, run, ryl};

#[test]
fn relative_input_from_subdirectory_finds_ancestor_config() {
    let root = tempdir().unwrap();
    let sub = root.path().join("sub");
    fs::create_dir(&sub).unwrap();
    fs::write(
        root.path().join(".ryl.toml"),
        "[rules.truthy]\nlevel = \"error\"\n",
    )
    .unwrap();
    fs::write(sub.join("a.yml"), "---\non: push\n").unwrap();

    for input in [".", "a.yml", "./a.yml"] {
        let (code, stdout, stderr) =
            run(ryl(root.path()).current_dir(&sub).args(["check", input]));
        assert_eq!(code, 1, "input {input}: stdout={stdout} stderr={stderr}");
        let output = command_output(&stdout, &stderr);
        assert!(output.contains("(truthy)"), "input {input}: {output}");
    }
}
