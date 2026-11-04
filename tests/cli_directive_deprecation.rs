use std::fs;
use std::process::Command;

use crate::common::cli::{run, run_with_stdin, ryl};
use tempfile::{TempDir, tempdir};

const NOTICE: &str = "yamllint directive aliases are deprecated";
const CONFIG: &str = "[lint.rules]\ntruthy = \"enable\"\ncomments = \"enable\"\n";
const SOURCE: &str = "# yamllint enable\na: 1\n# yamllint disable-line rule:truthy\n";

fn project(config: &str) -> TempDir {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".ryl.toml"), config).unwrap();
    dir
}

fn command(dir: &TempDir) -> Command {
    let mut cmd = ryl(dir.path());
    cmd.current_dir(dir.path());
    cmd
}

#[test]
fn file_commands_warn_once_without_changing_clean_status_or_stdout() {
    for mode in [
        vec!["check", "--strict"],
        vec!["check", "--fix", "--strict"],
        vec!["check", "--diff"],
        vec!["format"],
        vec!["format", "--check"],
        vec!["format", "--diff"],
    ] {
        for no_cache in [false, true] {
            let dir = project(CONFIG);
            fs::write(dir.path().join("a.yaml"), SOURCE).unwrap();
            for _ in 0..2 {
                let mut cmd = command(&dir);
                cmd.args(&mode).arg("a.yaml");
                if no_cache {
                    cmd.arg("--no-cache");
                }
                let (code, stdout, stderr) = run(&mut cmd);
                assert_eq!(code, 0, "{mode:?} {stdout}{stderr}");
                assert_eq!(stderr.matches(NOTICE).count(), 1, "{mode:?} {stderr}");
                assert!(stderr.contains("a.yaml:1:"), "{stderr}");
                assert!(!stdout.contains(NOTICE), "{stdout}");
                cmd.arg("--no-warnings");
                let (quiet_code, _, quiet) = run(&mut cmd);
                assert_eq!(quiet_code, code, "{quiet}");
                assert!(!quiet.contains(NOTICE), "{quiet}");
            }
        }
    }
}

#[test]
fn stdin_commands_warn_once_and_keep_warnings_off_stdout() {
    for mode in [
        vec!["check", "--strict"],
        vec!["check", "--diff"],
        vec!["format"],
        vec!["format", "--check"],
        vec!["format", "--diff"],
    ] {
        let dir = project(CONFIG);
        for quiet in [false, true] {
            let mut cmd = command(&dir);
            cmd.args(&mode).arg("-");
            if quiet {
                cmd.arg("--no-warnings");
            }
            let (code, stdout, stderr) = run_with_stdin(&mut cmd, SOURCE.as_bytes());
            assert_eq!(code, 0, "{mode:?} {stdout}{stderr}");
            assert_eq!(
                stderr.matches(NOTICE).count(),
                usize::from(!quiet),
                "{stderr}"
            );
            assert!(!stdout.contains(NOTICE), "{stdout}");
            if !quiet {
                assert!(stderr.contains("<stdin>:1:"), "{stderr}");
            }
        }
    }
}

#[test]
fn fixes_warn_only_for_supported_aliases_in_the_original_input() {
    let dir = project(CONFIG);
    let file = dir.path().join("a.yaml");
    fs::write(&file, "a: 1\n#yamllint enable\n").unwrap();
    let (code, _, stderr) = run(command(&dir)
        .args(["check", "--fix", "--no-cache"])
        .arg(&file));
    assert_eq!(code, 0, "{stderr}");
    assert!(!stderr.contains(NOTICE), "{stderr}");
    assert!(
        fs::read_to_string(&file)
            .unwrap()
            .contains("# yamllint enable")
    );
    let (code, _, stderr) = run(command(&dir)
        .args(["check", "--fix", "--no-cache"])
        .arg(&file));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stderr.matches(NOTICE).count(), 1, "{stderr}");
}

#[test]
fn inert_comments_strings_native_directives_and_empty_files_do_not_warn() {
    for source in [
        "",
        "a: 1\n",
        "# ryl enable\na: 1\n",
        "# mentions yamllint enable in prose\na: 1\n",
        "a: '# yamllint enable'\n",
        "a: |\n  # yamllint enable\n",
        "# yamllint enable # explanation\na: 1\n",
    ] {
        let dir = project(CONFIG);
        fs::write(dir.path().join("a.yaml"), source).unwrap();
        let (code, _, stderr) =
            run(command(&dir).args(["check", "--no-cache", "a.yaml"]));
        assert_eq!(code, 0, "{stderr}");
        assert!(!stderr.contains(NOTICE), "{source:?} {stderr}");
    }
}

#[test]
fn disable_file_alias_still_warns_without_lint_findings() {
    let dir = project(CONFIG);
    fs::write(
        dir.path().join("a.yaml"),
        "# yamllint disable-file\ninvalid: [\n",
    )
    .unwrap();
    let (code, stdout, stderr) =
        run(command(&dir).args(["check", "--strict", "a.yaml"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(stderr.matches(NOTICE).count(), 1, "{stderr}");
}

#[test]
fn markdown_warning_uses_first_active_region_host_line() {
    let dir = project(&format!("files = {{ markdown = [\"*.md\"] }}\n{CONFIG}"));
    let body = "# yamllint enable\n\n```text\n# yamllint enable\n```\n\n> ```yaml\n> # yamllint enable\n> a: 1\n> ```\n\n```yaml\n# yamllint enable\n```\n";
    fs::write(dir.path().join("a.md"), body).unwrap();
    for mode in [vec!["check", "--strict"], vec!["format", "--check"]] {
        let (code, _, stderr) = run(command(&dir).args(mode).arg("a.md"));
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(stderr.matches(NOTICE).count(), 1, "{stderr}");
        assert!(stderr.contains("a.md:8:"), "{stderr}");
    }
}

#[test]
fn markdown_disabled_regions_and_unsupported_carriage_returns_do_not_warn() {
    for (settings, source) in [
        (
            "front-matter = false",
            "---\n# yamllint enable\n---\nbody\n",
        ),
        ("fenced-blocks = false", "```yaml\n# yamllint enable\n```\n"),
        ("", "```yaml\r# yamllint enable\r```\r"),
    ] {
        let dir = project(&format!(
            "files = {{ markdown = [\"*.md\"] }}\n[markdown]\n{settings}\n{CONFIG}"
        ));
        fs::write(dir.path().join("a.md"), source).unwrap();
        let (code, _, stderr) = run(command(&dir).args(["check", "a.md"]));
        assert_eq!(code, i32::from(settings.is_empty()), "{stderr}");
        assert!(!stderr.contains(NOTICE), "{stderr}");
    }
}

#[test]
fn deprecation_is_separate_from_lint_status_and_machine_reports() {
    for (source, expected) in [
        ("# yamllint enable\na: ON\n", 1),
        ("# yamllint enable\na: [\n", 1),
    ] {
        let dir = project(CONFIG);
        fs::write(dir.path().join("a.yaml"), source).unwrap();
        let (code, stdout, stderr) =
            run(command(&dir)
                .args(["check", "--strict", "--format", "gitlab", "a.yaml"]));
        assert_eq!(code, expected, "{stdout}{stderr}");
        assert_eq!(stderr.matches(NOTICE).count(), 1, "{stderr}");
        let _: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        assert!(!stdout.contains(NOTICE));
    }
    let dir =
        project("[lint]\nstrict = true\n[lint.rules.truthy]\nlevel = \"warning\"\n");
    fs::write(dir.path().join("a.yaml"), "# yamllint enable\na: ON\n").unwrap();
    let (code, _, stderr) = run(command(&dir).args(["check", "a.yaml"]));
    assert_eq!(code, 2, "{stderr}");
    assert_eq!(stderr.matches(NOTICE).count(), 1, "{stderr}");
}

#[test]
fn encoded_files_and_stdin_warn_once_on_cache_hits_and_misses() {
    for bytes in [
        [b"\xef\xbb\xbf".as_slice(), SOURCE.as_bytes()].concat(),
        b"\xff\xfe"
            .iter()
            .copied()
            .chain(SOURCE.encode_utf16().flat_map(u16::to_le_bytes))
            .collect(),
    ] {
        let dir = project(CONFIG);
        fs::write(dir.path().join("a.yaml"), &bytes).unwrap();
        for _ in 0..2 {
            let (code, _, stderr) = run(command(&dir).args(["check", "a.yaml"]));
            assert_eq!(code, 0, "{stderr}");
            assert_eq!(stderr.matches(NOTICE).count(), 1, "{stderr}");
        }
        let (code, _, stderr) =
            run_with_stdin(command(&dir).args(["check", "-"]), &bytes);
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(stderr.matches(NOTICE).count(), 1, "{stderr}");
    }
}

#[test]
fn encoding_override_does_not_duplicate_existing_encoding_notice() {
    let dir = project(CONFIG);
    fs::write(dir.path().join("a.yaml"), SOURCE).unwrap();
    for mode in [vec!["check"], vec!["format", "--check"]] {
        for _ in 0..2 {
            let (code, _, stderr) = run(command(&dir)
                .env("YAMLLINT_FILE_ENCODING", "UTF-8")
                .args(&mode)
                .arg("a.yaml"));
            assert_eq!(code, 0, "{stderr}");
            assert_eq!(stderr.matches(NOTICE).count(), 1, "{stderr}");
            assert_eq!(
                stderr.matches("YAMLLINT_FILE_ENCODING").count(),
                3,
                "{stderr}"
            );
        }
    }
}

#[test]
fn excluded_inputs_ignored_stdin_and_templates_do_not_warn() {
    let dir = project(&format!("exclude = [\"ignored.yaml\"]\n{CONFIG}"));
    fs::write(dir.path().join("ignored.yaml"), SOURCE).unwrap();
    let (code, _, stderr) = run(command(&dir).args(["check", "ignored.yaml"]));
    assert_eq!(code, 0, "{stderr}");
    assert!(!stderr.contains(NOTICE), "{stderr}");
    let (_, _, stderr) = run_with_stdin(
        command(&dir).args(["format", "--stdin-filename", "ignored.yaml", "-"]),
        SOURCE.as_bytes(),
    );
    assert!(!stderr.contains(NOTICE), "{stderr}");
    fs::write(
        dir.path().join("template.yaml"),
        format!("#!jinja|yaml\n{SOURCE}"),
    )
    .unwrap();
    let (_, _, stderr) = run(command(&dir).args(["format", "."]));
    assert!(!stderr.contains(NOTICE), "{stderr}");
}

#[test]
fn stdin_markdown_and_explicit_filename_use_host_location() {
    let dir = project(CONFIG);
    let input = "heading\n\n```yaml\n# yamllint enable\na: 1\n```\n";
    let (code, _, stderr) = run_with_stdin(
        command(&dir).args([
            "check",
            "--markdown",
            "--stdin-filename",
            "article.md",
            "-",
        ]),
        input.as_bytes(),
    );
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.contains("article.md:4:"), "{stderr}");
    assert_eq!(stderr.matches(NOTICE).count(), 1, "{stderr}");
}

#[test]
fn duplicate_inputs_warn_once_and_failures_keep_their_original_status() {
    let dir = project(CONFIG);
    fs::write(dir.path().join("a.yaml"), SOURCE).unwrap();
    let (code, _, stderr) = run(command(&dir).args(["check", "a.yaml", "a.yaml"]));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stderr.matches(NOTICE).count(), 1, "{stderr}");
    for mode in [vec!["check", "--no-cache"], vec!["format", "--diff"]] {
        fs::write(dir.path().join("invalid.yaml"), b"\xff\xff").unwrap();
        let (code, _, stderr) = run(command(&dir).args(&mode).arg("invalid.yaml"));
        assert_eq!(code, if mode[0] == "check" { 1 } else { 2 }, "{stderr}");
        assert!(!stderr.contains(NOTICE), "{stderr}");
    }
}

#[cfg(unix)]
#[test]
fn warning_paths_cannot_emit_terminal_control_characters() {
    let dir = project(CONFIG);
    let file = "a\u{1b}[31m.yaml";
    fs::write(dir.path().join(file), SOURCE).unwrap();
    let (code, _, stderr) = run(command(&dir).args(["check", file]));
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.contains(NOTICE), "{stderr}");
    assert!(!stderr.contains('\u{1b}'), "{stderr}");
}

#[cfg(unix)]
#[test]
fn rewrite_previews_do_not_inspect_refused_symlink_contents() {
    let dir = project(CONFIG);
    let target = dir.path().join("target.yaml");
    fs::write(&target, SOURCE).unwrap();
    std::os::unix::fs::symlink(&target, dir.path().join("link.yaml")).unwrap();
    for mode in [
        vec!["format"],
        vec!["format", "--check"],
        vec!["format", "--diff"],
        vec!["check", "--diff"],
    ] {
        let (_, _, stderr) = run(command(&dir).args(mode).arg("link.yaml"));
        assert!(stderr.contains("refusing to follow a symlink"), "{stderr}");
        assert!(!stderr.contains(NOTICE), "{stderr}");
        assert_eq!(fs::read_to_string(&target).unwrap(), SOURCE);
    }
}

#[test]
fn rewrite_warning_probe_does_not_replace_missing_file_errors() {
    let dir = project(CONFIG);
    for mode in [vec!["format", "--diff"], vec!["check", "--diff"]] {
        let (code, _, stderr) = run(command(&dir).args(mode).arg("missing.yaml"));
        assert_eq!(code, 2, "{stderr}");
        assert!(!stderr.contains(NOTICE), "{stderr}");
        assert!(stderr.contains("missing.yaml"), "{stderr}");
    }
}

#[test]
fn parallel_lint_notices_follow_input_order_in_baselines_and_cache_hits() {
    let dir = project(CONFIG);
    let files: Vec<_> = (0..9).map(|index| format!("file-{index}.yaml")).collect();
    for (index, file) in files.iter().enumerate() {
        let filler = "# filler\n".repeat(if index == 0 { 10000 } else { 0 });
        fs::write(
            dir.path().join(file),
            format!("# yamllint enable\n{filler}a: 1\n"),
        )
        .unwrap();
    }
    for mode in [
        vec!["check", "--no-cache"],
        vec!["check"],
        vec!["check"],
        vec!["check", "--fix"],
    ] {
        let (code, _, stderr) = run(command(&dir)
            .env("RAYON_NUM_THREADS", "4")
            .args(mode)
            .args(&files));
        assert_eq!(code, 0, "{stderr}");
        let actual: Vec<_> = stderr
            .lines()
            .filter(|line| line.contains(NOTICE))
            .collect();
        assert_eq!(actual.len(), files.len(), "{stderr}");
        for (line, file) in actual.iter().zip(&files) {
            assert!(line.contains(&format!("{file}:1:")), "{stderr}");
        }
    }
}

#[test]
fn rewrite_notices_keep_input_order_with_parse_skips_and_cached_neighbors() {
    let dir = project(CONFIG);
    fs::write(dir.path().join("first.yaml"), "a: [\n").unwrap();
    fs::write(dir.path().join("second.yaml"), SOURCE).unwrap();
    for mode in [
        vec!["format"],
        vec!["format", "--check"],
        vec!["format", "--diff"],
        vec!["check", "--diff"],
    ] {
        for _ in 0..2 {
            let (_, _, stderr) = run(command(&dir)
                .args(&mode)
                .args(["first.yaml", "second.yaml"]));
            assert_eq!(stderr.matches(NOTICE).count(), 1, "{stderr}");
            let first = stderr
                .find("first.yaml")
                .expect("first input has a parse skip");
            let second = stderr
                .find("second.yaml")
                .expect("second input has an alias notice");
            assert!(first < second, "{stderr}");
        }
    }
}

#[test]
fn rewrite_notices_keep_input_order_between_cache_hits_and_misses() {
    let dir = project(CONFIG);
    fs::write(dir.path().join("hit.yaml"), SOURCE).unwrap();
    let (code, _, stderr) = run(command(&dir).args(["format", "hit.yaml"]));
    assert_eq!(code, 0, "{stderr}");
    for mode in [vec!["format", "--check"], vec!["format"]] {
        for inputs in [["hit.yaml", "miss.yaml"], ["miss.yaml", "hit.yaml"]] {
            fs::write(dir.path().join("miss.yaml"), format!("{SOURCE}b:   2\n"))
                .unwrap();
            let (_, _, stderr) = run(command(&dir).args(&mode).args(inputs));
            let notice_at = |file: &str| {
                stderr
                    .find(&format!("{file}:1: {NOTICE}"))
                    .unwrap_or_else(|| panic!("{mode:?} {inputs:?}: {stderr}"))
            };
            assert!(notice_at(inputs[0]) < notice_at(inputs[1]), "{stderr}");
            assert_eq!(stderr.matches(NOTICE).count(), 2, "{stderr}");
        }
    }
}

#[cfg(unix)]
#[test]
fn rewrite_notice_probe_preserves_one_shot_named_pipe_input() {
    use std::io::Write;
    use std::process::{Child, Stdio};
    use std::time::{Duration, Instant};

    let finish = |child: &mut Child| {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Some(status) = child.try_wait().unwrap() {
                return Some(status);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        child.kill().unwrap();
        child.wait().unwrap();
        None
    };
    for mode in [
        vec!["format", "--check"],
        vec!["format", "--diff"],
        vec!["check", "--diff"],
    ] {
        let dir = project("[lint.rules]\ncolons = \"enable\"\n");
        let pipe = dir.path().join("input.md");
        let mut maker = Command::new("mkfifo").arg(&pipe).spawn().unwrap();
        assert_eq!(finish(&mut maker).and_then(|status| status.code()), Some(0));
        let mut writer = Command::new("sh")
            .args(["-c", "cat > \"$1\"", "stream-writer"])
            .arg(&pipe)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        writer
            .stdin
            .take()
            .unwrap()
            .write_all(b"```yaml\n# yamllint enable\na:  1\n```\n")
            .unwrap();
        let mut reader = command(&dir)
            .args(&mode)
            .args(["--markdown", "--no-cache"])
            .arg(&pipe)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let reader_status = finish(&mut reader);
        let writer_status = finish(&mut writer);
        assert_eq!(
            reader_status.and_then(|status| status.code()),
            Some(1),
            "{mode:?} must process the single input without a second writer"
        );
        assert_eq!(writer_status.and_then(|status| status.code()), Some(0));
        let mut stderr = String::new();
        std::io::Read::read_to_string(&mut reader.stderr.take().unwrap(), &mut stderr)
            .unwrap();
        assert!(
            !stderr.contains(NOTICE),
            "a non-regular input never warns: {stderr}"
        );
    }
}
