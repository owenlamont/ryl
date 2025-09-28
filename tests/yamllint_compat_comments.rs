use std::fs;

use tempfile::tempdir;

#[path = "common/compat.rs"]
mod compat;

use compat::{
    SCENARIOS, build_ryl_command, build_yamllint_command, capture_with_env,
    ensure_yamllint_installed,
};

#[test]
fn comments_rule_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();

    let default_cfg = dir.path().join("comments-default.yml");
    fs::write(
        &default_cfg,
        "rules:\n  document-start: disable\n  comments:\n    require-starting-space: true\n    ignore-shebangs: true\n    min-spaces-from-content: 2\n",
    )
    .unwrap();

    let no_require_cfg = dir.path().join("comments-no-require.yml");
    fs::write(
        &no_require_cfg,
        "rules:\n  document-start: disable\n  comments:\n    require-starting-space: false\n    ignore-shebangs: true\n    min-spaces-from-content: 2\n",
    )
    .unwrap();

    let shebang_cfg = dir.path().join("comments-shebang.yml");
    fs::write(
        &shebang_cfg,
        "rules:\n  document-start: disable\n  comments:\n    require-starting-space: true\n    ignore-shebangs: false\n",
    )
    .unwrap();

    let bad_file = dir.path().join("bad.yaml");
    fs::write(
        &bad_file,
        "#comment\nkey: value # comment\nvalue: foo #bar\n",
    )
    .unwrap();

    let inline_file = dir.path().join("inline.yaml");
    fs::write(&inline_file, "key: value # comment\n").unwrap();

    let shebang_file = dir.path().join("shebang.yaml");
    fs::write(&shebang_file, "#!/usr/bin/env foo\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");

    for scenario in SCENARIOS {
        let mut ryl_default = build_ryl_command(exe, scenario.ryl_format);
        ryl_default.arg("-c").arg(&default_cfg).arg(&bad_file);
        let (ryl_code, ryl_output) = capture_with_env(ryl_default, scenario.envs);

        let mut yam_default = build_yamllint_command(scenario.yam_format);
        yam_default.arg("-c").arg(&default_cfg).arg(&bad_file);
        let (yam_code, yam_output) = capture_with_env(yam_default, scenario.envs);

        assert_eq!(ryl_code, 1, "ryl default exit ({})", scenario.label);
        assert_eq!(yam_code, 1, "yamllint default exit ({})", scenario.label);
        assert_eq!(
            ryl_output, yam_output,
            "default diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_no_require = build_ryl_command(exe, scenario.ryl_format);
        ryl_no_require
            .arg("-c")
            .arg(&no_require_cfg)
            .arg(&inline_file);
        let (ryl_nr_code, ryl_nr_output) = capture_with_env(ryl_no_require, scenario.envs);

        let mut yam_no_require = build_yamllint_command(scenario.yam_format);
        yam_no_require
            .arg("-c")
            .arg(&no_require_cfg)
            .arg(&inline_file);
        let (yam_nr_code, yam_nr_output) = capture_with_env(yam_no_require, scenario.envs);

        assert_eq!(ryl_nr_code, 1, "ryl no-require exit ({})", scenario.label);
        assert_eq!(
            yam_nr_code, 1,
            "yamllint no-require exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_nr_output, yam_nr_output,
            "no-require diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_shebang = build_ryl_command(exe, scenario.ryl_format);
        ryl_shebang.arg("-c").arg(&shebang_cfg).arg(&shebang_file);
        let (ryl_sh_code, ryl_sh_output) = capture_with_env(ryl_shebang, scenario.envs);

        let mut yam_shebang = build_yamllint_command(scenario.yam_format);
        yam_shebang.arg("-c").arg(&shebang_cfg).arg(&shebang_file);
        let (yam_sh_code, yam_sh_output) = capture_with_env(yam_shebang, scenario.envs);

        assert_eq!(ryl_sh_code, 1, "ryl shebang exit ({})", scenario.label);
        assert_eq!(yam_sh_code, 1, "yamllint shebang exit ({})", scenario.label);
        assert_eq!(
            ryl_sh_output, yam_sh_output,
            "shebang diagnostics mismatch ({})",
            scenario.label
        );
    }
}
