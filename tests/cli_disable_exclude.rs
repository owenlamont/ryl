use std::fs;

use tempfile::tempdir;

use crate::common;
use common::cli::{output_tuple, run, ryl, ryl_on, stdin_output};

const RULES: &str = "[lint.rules]\ntruthy = 'enable'\ntrailing-spaces = 'enable'\n";
const DIRTY: &str = "a: yes   \n";

#[test]
fn disable_subtracts_from_discovered_file_and_inline_configs() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.yaml"), DIRTY).unwrap();
    let config = dir.path().join(".ryl.toml");
    fs::write(&config, RULES).unwrap();
    for source in [
        vec![],
        vec!["-c", config.to_str().unwrap()],
        vec!["-d", RULES],
        vec!["-d", "{rules: {truthy: enable, trailing-spaces: enable}}"],
    ] {
        let (code, stdout, stderr) = run(ryl(dir.path())
            .current_dir(dir.path())
            .arg("check")
            .args(&source)
            .args(["--disable", "truthy,anchors", "a.yaml"]));
        assert_eq!(code, 1, "{stdout}{stderr}");
        assert!(stderr.contains("trailing-spaces"), "{stderr}");
        assert!(!stderr.contains("truthy"), "{stderr}");
    }
}

#[test]
fn disable_runs_after_enable_in_either_argument_order() {
    for args in [
        vec![
            "check",
            "--enable",
            "ALL",
            "--disable",
            "truthy,document-start",
        ],
        vec!["--disable", "truthy,document-start", "--enable", "ALL"],
    ] {
        let (code, stdout, stderr, _) = ryl_on(None, DIRTY, &args);
        assert_eq!(code, 1, "{stdout}{stderr}");
        assert!(stderr.contains("trailing-spaces"), "{stderr}");
        assert!(!stderr.contains("truthy"), "{stderr}");
        assert!(!stderr.contains("document-start"), "{stderr}");
    }
}

#[test]
fn disable_keeps_remaining_rule_options_and_levels() {
    let config = "[lint.rules]\ntruthy = 'enable'\n[lint.rules.line-length]\nmax = 5\nlevel = 'warning'\n";
    let (code, _, stderr, _) =
        ryl_on(Some(config), DIRTY, &["check", "--disable", "truthy"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stderr.contains("1:6") && stderr.contains("line-length"),
        "{stderr}"
    );
}

#[test]
fn disable_applies_to_each_discovered_project_config() {
    let dir = tempdir().unwrap();
    for (sub, config) in [
        ("one", RULES),
        (
            "two",
            "[lint.rules]\ntruthy = 'enable'\nanchors = 'enable'\n",
        ),
    ] {
        fs::create_dir(dir.path().join(sub)).unwrap();
        fs::write(dir.path().join(sub).join(".ryl.toml"), config).unwrap();
        fs::write(dir.path().join(sub).join("a.yaml"), DIRTY).unwrap();
    }
    let (code, stdout, stderr) = run(ryl(dir.path()).current_dir(dir.path()).args([
        "check",
        "--disable",
        "truthy",
        ".",
    ]));
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(stderr.contains("trailing-spaces"), "{stderr}");
    assert!(!stderr.contains("truthy"), "{stderr}");
}

#[test]
fn disable_rejects_unknown_rules_and_repeated_flags() {
    for args in [
        vec!["--disable", "nope"],
        vec!["--disable", "truthy,nope"],
        vec!["--disable", "truthy", "--disable", "anchors"],
        vec!["--exclude", "a.yaml", "--exclude", "b.yaml"],
    ] {
        let (code, _, stderr, _) = ryl_on(None, DIRTY, &args);
        assert_eq!(code, 2, "{args:?}: {stderr}");
        assert!(
            stderr.contains("no such rule") || stderr.contains("multiple times"),
            "{stderr}"
        );
    }
}

#[test]
fn disable_does_not_implicitly_enable_rules_and_rejects_empty_selection() {
    for (config, args, message) in [
        (
            None,
            vec!["check", "--disable", "truthy"],
            "no configuration found",
        ),
        (
            Some(RULES),
            vec!["check", "--disable", "ALL"],
            "enables no rules",
        ),
        (
            None,
            vec!["check", "--enable", "truthy", "--disable", "truthy"],
            "enables no rules",
        ),
        (
            Some(RULES),
            vec!["check", "--enable", "truthy", "--disable", "truthy"],
            "enables no rules",
        ),
        (
            None,
            vec![
                "check",
                "-d",
                "[lint]",
                "--enable",
                "ALL",
                "--disable",
                "ALL",
            ],
            "enables no rules",
        ),
    ] {
        let (code, _, stderr, _) = ryl_on(config, DIRTY, &args);
        assert_eq!(code, 2, "{stderr}");
        assert!(stderr.contains(message), "{stderr}");
    }
}

#[test]
fn disable_limits_safe_fix_and_diff_edits() {
    for mode in ["--fix", "--diff"] {
        let (code, stdout, stderr, contents) = ryl_on(
            Some(
                "[lint.rules]\ndocument-start = 'enable'\ntrailing-spaces = 'enable'\n",
            ),
            DIRTY,
            &["check", mode, "--disable", "trailing-spaces"],
        );
        assert_eq!(code, i32::from(mode == "--diff"), "{stdout}{stderr}");
        if mode == "--fix" {
            assert_eq!(contents, format!("---\n{DIRTY}"));
        } else {
            assert_eq!(contents, DIRTY);
            assert!(stdout.contains("+---"), "{stdout}");
            assert!(!stdout.contains("-a: yes"), "{stdout}");
        }
    }
}

#[test]
fn exclude_replaces_config_patterns_for_walks_and_explicit_paths() {
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("skip")).unwrap();
    for file in [
        "a.yaml",
        "configured.yaml",
        "flagged.yaml",
        "skip/nested.yaml",
    ] {
        fs::write(dir.path().join(file), DIRTY).unwrap();
    }
    let config = dir.path().join(".ryl.toml");
    let data = format!("exclude = ['configured.yaml']\n{RULES}");
    fs::write(&config, &data).unwrap();
    for source in [
        vec![],
        vec!["-c", config.to_str().unwrap()],
        vec!["-d", &data],
        vec!["-d", "{ignore: [configured.yaml], rules: {truthy: enable}}"],
    ] {
        for (paths, patterns) in [
            (vec!["."], "flag*.yaml,skip/**"),
            (
                vec![
                    "a.yaml",
                    "configured.yaml",
                    "flagged.yaml",
                    "skip/nested.yaml",
                ],
                "flagged.yaml,skip/",
            ),
        ] {
            let (code, stdout, stderr) = run(ryl(dir.path())
                .current_dir(dir.path())
                .args(["check", "--list-files"])
                .args(&source)
                .args(["--exclude", patterns])
                .args(&paths));
            assert_eq!(code, 0, "{stdout}{stderr}");
            assert!(
                stdout.contains("a.yaml") && stdout.contains("configured.yaml"),
                "{stdout}"
            );
            assert!(
                !stdout.contains("flagged.yaml") && !stdout.contains("nested.yaml"),
                "{stdout}"
            );
        }
    }
}

#[test]
fn exclude_uses_cwd_across_nested_configs_and_replaces_exclude_from_file() {
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("sub/a.yaml"), DIRTY).unwrap();
    fs::write(dir.path().join("sub/kept.yaml"), DIRTY).unwrap();
    fs::write(dir.path().join("sub/patterns"), "kept.yaml\n").unwrap();
    fs::write(
        dir.path().join("sub/.ryl.toml"),
        format!("exclude-from-file = 'patterns'\n{RULES}"),
    )
    .unwrap();
    for source in [vec![], vec!["-c", "sub/.ryl.toml"]] {
        let (code, stdout, stderr) = run(ryl(dir.path())
            .current_dir(dir.path())
            .args(["check", "--list-files", "--exclude", "sub/a.yaml"])
            .args(&source)
            .arg("sub"));
        assert_eq!(code, 0, "{stdout}{stderr}");
        assert!(
            stdout.contains("kept.yaml") && !stdout.contains("a.yaml"),
            "{stdout}"
        );
    }
}

#[test]
fn exclude_alone_needs_rules_and_empty_exclude_clears_config_patterns() {
    let (code, _, stderr, _) =
        ryl_on(None, DIRTY, &["check", "--exclude", "other.yaml"]);
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("no configuration found"), "{stderr}");
    let (code, _, stderr, _) = ryl_on(
        Some(&format!("exclude = ['a.yaml']\n{RULES}")),
        DIRTY,
        &["check", "--exclude", ""],
    );
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("truthy"), "{stderr}");
}

#[test]
fn invalid_exclude_globs_fail_for_discovered_global_and_stdin_configs() {
    for (config, args) in [
        (None, vec!["check", "--enable", "ALL", "--exclude", "["]),
        (Some(RULES), vec!["check", "--exclude", "["]),
        (None, vec!["check", "-d", RULES, "--exclude", "["]),
    ] {
        let (code, _, stderr, _) = ryl_on(config, DIRTY, &args);
        assert_eq!(code, 2, "{stderr}");
        assert!(stderr.contains("pattern '[' is invalid"), "{stderr}");
    }
    let dir = tempdir().unwrap();
    let (code, _, stderr) = output_tuple(stdin_output(
        ryl(dir.path()).current_dir(dir.path()).args([
            "check",
            "--enable",
            "ALL",
            "--exclude",
            "[",
            "-",
        ]),
        DIRTY.as_bytes(),
        |result| result.unwrap(),
    ));
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("pattern '[' is invalid"), "{stderr}");
}

#[test]
fn overrides_apply_to_stdin_with_optional_filename() {
    let dir = tempdir().unwrap();
    for (extra, expected) in [(vec![], 1), (vec!["--stdin-filename", "a.yaml"], 0)] {
        let (code, _, stderr) = output_tuple(stdin_output(
            ryl(dir.path())
                .current_dir(dir.path())
                .args([
                    "check",
                    "--enable",
                    "ALL",
                    "--disable",
                    "truthy,document-start",
                    "--exclude",
                    "a.yaml",
                    "-",
                ])
                .args(&extra),
            DIRTY.as_bytes(),
            |result| result.unwrap(),
        ));
        assert_eq!(code, expected, "{stderr}");
        assert!(
            !stderr.contains("truthy") && !stderr.contains("document-start"),
            "{stderr}"
        );
    }
    let (code, _, stderr) = output_tuple(stdin_output(
        ryl(dir.path()).current_dir(dir.path()).args([
            "check",
            "--enable",
            "truthy",
            "--disable",
            "ALL",
            "-",
        ]),
        DIRTY.as_bytes(),
        |result| result.unwrap(),
    ));
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("enables no rules"), "{stderr}");
}

#[test]
fn exclude_applies_to_lint_fix_diff_bare_and_all_format_modes_without_config() {
    for args in [
        vec!["check", "--enable", "ALL"],
        vec!["check", "--enable", "ALL", "--fix"],
        vec!["check", "--enable", "ALL", "--diff"],
        vec!["--enable", "ALL"],
        vec!["format"],
        vec!["format", "--check"],
        vec!["format", "--diff"],
    ] {
        let mut flags = args.clone();
        flags.extend(["--exclude", "a.yaml"]);
        let (code, stdout, stderr, contents) = ryl_on(None, DIRTY, &flags);
        assert_eq!(code, 0, "{args:?}: {stdout}{stderr}");
        assert!(stdout.is_empty(), "{stdout}");
        assert_eq!(contents, DIRTY);
    }
}

#[test]
fn format_disable_silences_lint_conflicts_without_disabling_formatting() {
    let config = "[lint.rules]\nquoted-strings = 'enable'\n";
    let (_, _, stderr, _) = ryl_on(Some(config), "a: 'word'   \n", &["format"]);
    assert!(stderr.contains("incompatible"), "{stderr}");
    let (code, _, stderr, contents) = ryl_on(
        Some(config),
        "a: 'word'   \n",
        &["format", "--disable", "quoted-strings,trailing-spaces"],
    );
    assert_eq!(code, 0, "{stderr}");
    assert!(!stderr.contains("incompatible"), "{stderr}");
    assert_eq!(contents, "a: word\n");
}
