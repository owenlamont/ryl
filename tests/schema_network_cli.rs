use std::path::Path;
use std::process::Output;

fn run(dir: &Path, extra: &[&str]) -> Output {
    let canonical = dir.canonicalize().unwrap();
    let dir = canonical.as_path();
    crate::common::cli::ryl(dir)
        .args(["check", "-c"])
        .arg(dir.join(".ryl.toml"))
        .args(extra)
        .arg(dir.join("a.yaml"))
        .env("XDG_CONFIG_HOME", dir.join("user-config"))
        .env("RYL_SCHEMA_CACHE_DIR", dir.join("schema-cache"))
        .env_remove("RYL_SCHEMA_ALLOWED_ORIGINS")
        .env_remove("RYL_NO_CACHE")
        .env("RYL_CACHE_DIR", dir.join("lint-cache"))
        .env_remove("RYL_OFFLINE")
        .output()
        .unwrap()
}

fn project(url: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".ryl.toml"),
        format!("[[lint.rules.schema.schemas]]\nurl = '{url}'\n"),
    )
    .unwrap();
    std::fs::write(dir.path().join("a.yaml"), "{}\n").unwrap();
    dir
}

#[test]
fn ungranted_url_is_schema_named_exit_two_without_query_leaks() {
    let dir = project("https://schemas.example.org/root?token=secret");
    let output = run(dir.path(), &[]);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("invalid config: schema"), "{stderr}");
    assert!(!stderr.contains("secret"), "{stderr}");
    #[cfg(feature = "schema-network")]
    assert!(stderr.contains("origin not granted"), "{stderr}");
    #[cfg(not(feature = "schema-network"))]
    assert!(stderr.contains("lacks network support"), "{stderr}");
    println!("ryl check: {stderr}");
}

#[cfg(feature = "schema-network")]
#[test]
fn progress_no_progress_offline_and_no_cache_flags_obey_gates() {
    let dir = project("https://127.0.0.1/root?token=secret");
    for (extra, progress, expected) in [
        (
            vec!["--schema-allow-origin", "https://127.0.0.1", "--no-cache"],
            true,
            "HTTPS request failed",
        ),
        (
            vec![
                "--schema-allow-origin",
                "https://127.0.0.1",
                "--no-progress",
                "--no-cache",
            ],
            false,
            "HTTPS request failed",
        ),
        (
            vec!["--schema-allow-origin", "https://127.0.0.1", "--offline"],
            false,
            "offline cache",
        ),
    ] {
        let output = run(dir.path(), &extra);
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert_eq!(output.status.code(), Some(2), "{stderr}");
        assert_eq!(stderr.contains("Downloading schema"), progress, "{stderr}");
        assert!(stderr.contains(expected), "{stderr}");
        assert!(!stderr.contains("secret"), "{stderr}");
    }
}

#[cfg(feature = "schema-network")]
#[test]
fn offline_cli_reads_cache_but_no_cache_disables_both_cache_families() {
    use sha2::{Digest, Sha256};
    let url = "https://127.0.0.1/root.json";
    let dir = project(url);
    let hex = |input: &[u8]| -> String {
        Sha256::digest(input)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    };
    let cache = dir.path().join("schema-cache");
    std::fs::create_dir(&cache).unwrap();
    let metadata = serde_json::json!({"url":url, "sha256":hex(b"true"), "fetched":0});
    std::fs::write(cache.join(hex(url.as_bytes())), format!("{metadata}\ntrue"))
        .unwrap();
    let output = run(
        dir.path(),
        &["--schema-allow-origin", "https://127.0.0.1", "--offline"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut fresh = metadata;
    fresh["fetched"] = serde_json::json!(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    );
    std::fs::write(cache.join(hex(url.as_bytes())), format!("{fresh}\ntrue")).unwrap();
    let online = run(dir.path(), &["--schema-allow-origin", "https://127.0.0.1"]);
    assert!(
        online.status.success(),
        "{}",
        String::from_utf8_lossy(&online.stderr)
    );
    assert!(!String::from_utf8_lossy(&online.stderr).contains("Downloading schema"));
    let output = run(
        dir.path(),
        &[
            "--schema-allow-origin",
            "https://127.0.0.1",
            "--offline",
            "--no-cache",
        ],
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("offline cache"));
}

#[cfg(feature = "lsp")]
#[test]
fn server_configuration_refuses_remote_associations_before_network() {
    let dir = project("https://schemas.example.org/root.json");
    let error = ryl::config::discover_config_with(
        &[],
        &ryl::config::Overrides {
            config_file: Some(dir.path().join(".ryl.toml")),
            config_data: None,
        },
        &ryl::config::ServerEnv,
    )
    .unwrap_err();
    #[cfg(feature = "schema-network")]
    assert!(error.contains("not supported by ryl server yet"), "{error}");
    #[cfg(not(feature = "schema-network"))]
    assert!(error.contains("lacks network support"), "{error}");
}

#[cfg(all(target_os = "linux", feature = "schema-network"))]
#[test]
fn terminal_preparation_status_cancels_cleanly_on_fast_offline_failure() {
    let dir = project("https://127.0.0.1/root.json");
    let quote =
        |path: &Path| format!("'{}'", path.to_str().unwrap().replace('\'', "'\\''"));
    let command = format!(
        "{} check -c {} --schema-allow-origin https://127.0.0.1 --offline {}",
        quote(Path::new(env!("CARGO_BIN_EXE_ryl"))),
        quote(&dir.path().join(".ryl.toml")),
        quote(&dir.path().join("a.yaml"))
    );
    let output = std::process::Command::new("script")
        .args(["-q", "-e", "-c", &command, "/dev/null"])
        .env("HOME", dir.path())
        .env("XDG_CONFIG_HOME", dir.path().join("config"))
        .env("RYL_SCHEMA_CACHE_DIR", dir.path().join("cache"))
        .env_remove("RYL_SCHEMA_ALLOWED_ORIGINS")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stdout).contains("offline cache"));
}

#[cfg(feature = "schema-network")]
#[test]
fn invalid_cli_origin_grant_is_a_schema_named_config_error() {
    let dir = project("https://schemas.example.org/root.json");
    let output = run(
        dir.path(),
        &["--schema-allow-origin", "http://schemas.example.org"],
    );
    let error = String::from_utf8(output.stderr).unwrap();
    assert_eq!(output.status.code(), Some(2), "{error}");
    assert!(error.contains("schema"), "{error}");
    assert!(error.contains("require HTTPS"), "{error}");
}

#[test]
fn malformed_pin_values_do_not_leak_url_queries_in_toml_key_paths() {
    for token in ["secret", "secret with spaces", "secret`part"] {
        let dir = project("https://schemas.example.org/root");
        std::fs::write(dir.path().join(".ryl.toml"), format!("[lint.rules.schema.pins]\n'https://user:password@schemas.example.org/root?token={token}' = 42\n")).unwrap();
        let output = run(dir.path(), &[]);
        let error = String::from_utf8(output.stderr).unwrap();
        assert_eq!(output.status.code(), Some(2), "{error}");
        assert!(error.contains("expected a string"), "{error}");
        assert!(!error.contains("secret"), "{error}");
        assert!(!error.contains("password"), "{error}");
    }
}
