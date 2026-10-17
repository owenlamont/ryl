mod common;

use std::fs;
use std::path::Path;

use tempfile::tempdir;

use common::cli::{run, ryl};

fn entries(root: &Path) -> Vec<std::path::PathBuf> {
    fs::read_dir(root.join(env!("CARGO_PKG_VERSION")))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect()
}

#[test]
fn check_reuses_diagnostics_and_invalidates_content_and_config() {
    let dir = tempdir().unwrap();
    let config = dir.path().join("ryl.toml");
    let file = dir.path().join("a.yaml");
    fs::write(
        &config,
        "[lint.rules]\nnew-line-at-end-of-file = 'enable'\n",
    )
    .unwrap();
    fs::write(&file, "a: b").unwrap();
    let run_check = || {
        run(ryl(dir.path())
            .args(["check", "-c"])
            .arg(&config)
            .arg(&file))
    };
    let first = run_check();
    assert_eq!(first.0, 1);
    assert_eq!(run_check(), first);
    let root = dir.path().join(".ryl_cache");
    assert_eq!(entries(&root).len(), 1);
    assert_eq!(fs::read_to_string(root.join(".gitignore")).unwrap(), "*\n");
    assert!(root.join("CACHEDIR.TAG").exists());

    let uncached = run(ryl(dir.path())
        .args(["check", "--no-cache", "-c"])
        .arg(&config)
        .arg(&file));
    assert_eq!(uncached, first);
    let modified = fs::metadata(&file).unwrap().modified().unwrap();
    fs::write(&file, "a: c").unwrap();
    fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
    assert_eq!(run_check(), first);
    assert_eq!(entries(&root).len(), 2);
    fs::write(&config, "[lint.rules]\ntrailing-spaces = 'enable'\n").unwrap();
    assert_eq!(run_check().0, 0);
    assert_eq!(entries(&root).len(), 3);
}

#[test]
fn corrupt_entry_is_rebuilt_and_version_directory_isolated() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("a.yaml");
    fs::write(&file, "a: b\n").unwrap();
    let root = dir.path().join(".ryl_cache");
    let command = || {
        run(ryl(dir.path())
            .args(["check", "--enable", "ALL", "--cache-dir"])
            .arg(&root)
            .arg(&file))
    };
    let first = command();
    let entry = entries(&root).pop().unwrap();
    fs::write(&entry, b"broken JSON").unwrap();
    assert_eq!(command(), first);
    assert_ne!(fs::read(&entry).unwrap(), b"broken JSON");
    fs::write(
        &entry,
        r#"[{"line":1,"column":1,"level":"bad","message":"x","rule":null}]"#,
    )
    .unwrap();
    assert_eq!(command(), first);
    fs::write(
        &entry,
        r#"[{"line":1,"column":1,"level":"error","message":"x","rule":"unknown"}]"#,
    )
    .unwrap();
    assert_eq!(command(), first);
    fs::create_dir(root.join("other-version")).unwrap();
    fs::write(
        root.join("other-version").join(entry.file_name().unwrap()),
        b"broken JSON",
    )
    .unwrap();
    assert_eq!(command(), first);
}

#[test]
fn controls_stdin_and_unusable_directory() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("a.yaml");
    fs::write(&file, "a: b\n").unwrap();
    let cache = dir.path().join("custom");
    let (code, _, _) = run(ryl(dir.path())
        .args(["check", "--enable", "ALL", "--cache-dir"])
        .arg(&cache)
        .arg(&file));
    assert_ne!(code, 2);
    assert_eq!(entries(&cache).len(), 1);
    let bypass = dir.path().join("bypass");
    let _ = run(ryl(dir.path())
        .env("RYL_NO_CACHE", "1")
        .args(["check", "--enable", "ALL", "--cache-dir"])
        .arg(&bypass)
        .arg(&file));
    assert!(!bypass.exists());
    let disabled = dir.path().join("disabled");
    let _ = run(ryl(dir.path())
        .args(["check", "--enable", "ALL", "--no-cache", "--cache-dir"])
        .arg(&disabled)
        .arg(&file));
    assert!(!disabled.exists());
    let stdin_cache = dir.path().join("stdin-cache");
    let stdin = common::cli::stdin_output(
        ryl(dir.path())
            .env("RYL_CACHE_DIR", &stdin_cache)
            .args(["check", "--enable", "ALL", "--stdin-filename"])
            .arg(&file)
            .arg("-"),
        b"a: b\n",
        Result::unwrap,
    );
    assert_ne!(stdin.status.code(), Some(2));
    assert!(!stdin_cache.exists());
    let blocked = dir.path().join("blocked");
    fs::write(&blocked, "file").unwrap();
    let (code, _, stderr) = run(ryl(dir.path())
        .args(["check", "--enable", "ALL", "--cache-dir"])
        .arg(&blocked)
        .arg(&file));
    assert_eq!(code, 2);
    assert!(stderr.contains("cache"));
    let (code, _, stderr) = run(ryl(dir.path())
        .args(["format", "--cache-dir"])
        .arg(&blocked)
        .arg(&file));
    assert_eq!(code, 2, "{stderr}");
}

#[test]
fn malformed_entry_destination_reports_cache_error() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("a.yaml");
    fs::write(&file, "a: b\n").unwrap();
    let root = dir.path().join("cache");
    let check = || {
        run(ryl(dir.path())
            .args(["check", "--enable", "ALL", "--cache-dir"])
            .arg(&root)
            .arg(&file))
    };
    let _ = check();
    let entry = entries(&root).pop().unwrap();
    fs::remove_file(&entry).unwrap();
    fs::create_dir(&entry).unwrap();
    assert_eq!(check().0, 2);

    let format_root = dir.path().join("format-cache");
    let format = |check: bool| {
        let mut cmd = ryl(dir.path());
        cmd.args(["format", "--cache-dir"]).arg(&format_root);
        if check {
            cmd.arg("--check");
        }
        run(cmd.arg(&file))
    };
    assert_eq!(format(true).0, 0);
    let entry = entries(&format_root).pop().unwrap();
    fs::remove_file(&entry).unwrap();
    fs::create_dir(&entry).unwrap();
    assert_eq!(format(true).0, 2);
    assert_eq!(format(false).0, 2);
}

#[cfg(unix)]
#[test]
fn unwritable_cache_directory_exits_two() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let file = dir.path().join("a.yaml");
    fs::write(&file, "a: b\n").unwrap();
    let root = dir.path().join("cache");
    let version = root.join(env!("CARGO_PKG_VERSION"));
    fs::create_dir_all(&version).unwrap();
    fs::set_permissions(&version, fs::Permissions::from_mode(0o555)).unwrap();
    let (code, _, stderr) = run(ryl(dir.path())
        .args(["check", "--enable", "ALL", "--cache-dir"])
        .arg(&root)
        .arg(&file));
    fs::set_permissions(&version, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("cache"), "{stderr}");
}

#[cfg(unix)]
#[test]
fn unwritable_cache_support_files_exit_two() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let file = dir.path().join("a.yaml");
    fs::write(&file, "a: b\n").unwrap();
    for existing_gitignore in [false, true] {
        let root = dir.path().join(format!("cache-{existing_gitignore}"));
        fs::create_dir_all(root.join(env!("CARGO_PKG_VERSION"))).unwrap();
        if existing_gitignore {
            fs::write(root.join(".gitignore"), "*\n").unwrap();
        }
        fs::set_permissions(&root, fs::Permissions::from_mode(0o555)).unwrap();
        let (code, _, stderr) = run(ryl(dir.path())
            .args(["check", "--enable", "ALL", "--cache-dir"])
            .arg(&root)
            .arg(&file));
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(code, 2, "{stderr}");
        assert!(stderr.contains("cache"), "{stderr}");
    }
}

#[test]
fn cache_directory_precedence_and_diff_bypass() {
    let dir = tempdir().unwrap();
    let config = dir.path().join("ryl.toml");
    let file = dir.path().join("a.yaml");
    fs::write(
        &config,
        "cache-dir = 'configured'\n[lint.rules]\ntrailing-spaces = 'enable'\n",
    )
    .unwrap();
    fs::write(&file, "a: b\n").unwrap();
    let configured = dir.path().join("configured");
    let environment = dir.path().join("environment");
    let cli = dir.path().join("cli");
    assert_eq!(
        run(ryl(dir.path())
            .args(["check", "-c"])
            .arg(&config)
            .arg(&file))
        .0,
        0
    );
    assert_eq!(entries(&configured).len(), 1);
    assert_eq!(
        run(ryl(dir.path())
            .env("RYL_CACHE_DIR", &environment)
            .args(["check", "-c"])
            .arg(&config)
            .arg(&file))
        .0,
        0
    );
    assert_eq!(entries(&environment).len(), 1);
    assert_eq!(
        run(ryl(dir.path())
            .env("RYL_CACHE_DIR", &environment)
            .args(["check", "--cache-dir"])
            .arg(&cli)
            .arg("-c")
            .arg(&config)
            .arg(&file))
        .0,
        0
    );
    assert_eq!(entries(&cli).len(), 1);
    let diff = dir.path().join("diff");
    assert_eq!(
        run(ryl(dir.path())
            .args(["format", "--diff", "--cache-dir"])
            .arg(&diff)
            .arg(&file))
        .0,
        0
    );
    assert!(!diff.exists());
}

#[test]
fn format_marks_clean_files_and_invalidates_changed_content() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".ryl.toml"), "[format]\n").unwrap();
    let file = dir.path().join("a.yaml");
    fs::write(&file, "a: b\n").unwrap();
    let command = || run(ryl(dir.path()).arg("format").arg(&file));
    assert_eq!(command().0, 0);
    let root = dir.path().join(".ryl_cache");
    assert_eq!(entries(&root).len(), 1);
    assert_eq!(command().0, 0);
    assert_eq!(entries(&root).len(), 1);
    fs::write(&file, "a: c\n").unwrap();
    assert_eq!(command().0, 0);
    assert_eq!(entries(&root).len(), 2);
}

#[cfg(unix)]
#[test]
fn format_cache_preserves_unrepresentable_filename_notice() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".ryl.toml"), "[format]\n").unwrap();
    for name in [
        OsString::from("bad\nname.yaml"),
        OsString::from_vec(b"bad\xffname.yaml".to_vec()),
    ] {
        let file = dir.path().join(name);
        fs::write(&file, "a: b\n").unwrap();
        assert_eq!(run(ryl(dir.path()).arg("format").arg(&file)).0, 0);
        let (code, _, stderr) =
            run(ryl(dir.path()).args(["format", "--check"]).arg(&file));
        assert_eq!(code, 0, "{stderr}");
        assert!(stderr.contains("skipped by --check"), "{stderr}");
    }
    assert!(entries(&dir.path().join(".ryl_cache")).is_empty());
}
