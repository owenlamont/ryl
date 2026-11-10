use std::fs;
use std::path::{Path, PathBuf};

use clap::ValueEnum;
use tempfile::tempdir;

use crate::common::cli::{run, ryl};
use ryl::config::{YamlLintConfig, init::Preset};

#[test]
fn presets_write_loadable_configs_that_lint() {
    for preset in Preset::value_variants() {
        let name = preset.name();
        let embedded = preset.toml();
        let dir = tempdir().unwrap();
        let (code, stdout, stderr) = run(ryl(dir.path())
            .current_dir(dir.path())
            .args(["init", "--preset", name]));
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(stdout, "Created .ryl.toml\n");
        assert!(stderr.is_empty(), "{stderr}");
        let text = fs::read_to_string(dir.path().join(".ryl.toml")).unwrap();
        assert_eq!(
            text,
            format!(
                "# {name} preset, written by ryl {}\n\n{embedded}",
                env!("CARGO_PKG_VERSION")
            )
        );
        assert!(
            YamlLintConfig::from_toml_str(&text)
                .unwrap()
                .enables_any_rule()
        );
        let (valid, invalid, rule) = match preset {
            Preset::Default | Preset::Relaxed => (
                "---\nkey: value\n",
                "---\nkey: value  \n",
                "trailing-spaces",
            ),
            Preset::Ansible => (
                "- name: Configure server\n  hosts: all\n",
                "---\nmode: 0644\n",
                "octal-values",
            ),
            Preset::Cloudformation => (
                "Value: !Ref Bucket\nUnused: &bucket data\n",
                "---\nValue: &bucket data\nAlias: *bucket\n",
                "anchors",
            ),
            Preset::HomeAssistant => (
                "password: !secret api_password\nplatform: state\nmessage: \"Hello {{ user }}\"\n",
                "---\nmessage: Hello {{ user }}\n",
                "quoted-strings",
            ),
            Preset::Kubernetes => (
                "kind: List\nitems:\n- apiVersion: v1\n",
                "---\nvalue: .inf\n",
                "float-values",
            ),
            Preset::GithubActions => (
                "on: push\n",
                "---\nbase: &base {}\njob:\n  <<: *base\n",
                "merge-keys",
            ),
            Preset::Compose => (
                "services: !reset {}\n",
                "---\nservices: !unknown {}\n",
                "tags",
            ),
            Preset::Helm => (
                "name: 'demo'\nversion: \"1.0\"\n",
                "---\nname: demo\n",
                "quoted-strings",
            ),
            Preset::GitlabCi => (
                "job: !reference [template, script]\n",
                "---\njob: !unknown []\n",
                "tags",
            ),
        };
        fs::write(dir.path().join("values.yaml"), valid).unwrap();
        let (code, _, stderr) = run(ryl(dir.path())
            .current_dir(dir.path())
            .args(["check", "values.yaml"]));
        assert_eq!(code, 0, "{name}: {stderr}");
        assert!(stderr.is_empty(), "{name}: {stderr}");
        if !matches!(preset, Preset::Default | Preset::Relaxed) {
            let default = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src/config/init/presets/default.toml");
            let (code, _, stderr) = run(ryl(dir.path())
                .current_dir(dir.path())
                .args(["check", "--strict", "-c"])
                .arg(default)
                .arg("values.yaml"));
            assert!(matches!(code, 1 | 2), "{name}: {stderr}");
            assert!(stderr.contains("document-start"), "{name}: {stderr}");
        }
        fs::write(dir.path().join("values.yaml"), invalid).unwrap();
        let (code, _, stderr) = run(ryl(dir.path())
            .current_dir(dir.path())
            .args(["check", "values.yaml"]));
        assert_eq!(code, 1, "{stderr}");
        assert!(stderr.contains(rule), "{name}: {stderr}");
        if !matches!(preset, Preset::Default | Preset::Relaxed) {
            let default = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src/config/init/presets/default.toml");
            let (code, _, stderr) = run(ryl(dir.path())
                .current_dir(dir.path())
                .args(["check", "-c"])
                .arg(default)
                .arg("values.yaml"));
            assert_eq!(code, 0, "{name}: {stderr}");
            assert!(stderr.is_empty(), "{name}: {stderr}");
        }
    }
}

#[test]
fn omitted_preset_is_default_and_unrelated_pyproject_is_allowed() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("pyproject.toml"),
        "[project]\nname = 'demo'\n",
    )
    .unwrap();
    let (code, _, stderr) = run(ryl(dir.path()).current_dir(dir.path()).arg("init"));
    assert_eq!(code, 0, "{stderr}");
    assert!(
        fs::read_to_string(dir.path().join(".ryl.toml"))
            .unwrap()
            .ends_with(Preset::Default.toml())
    );
}

#[test]
fn existing_project_candidates_are_preserved() {
    for name in [
        ".ryl.toml",
        "ryl.toml",
        ".config/.ryl.toml",
        ".config/ryl.toml",
        "pyproject.toml",
        ".yamllint",
        ".yamllint.yaml",
        ".yamllint.yml",
    ] {
        let dir = tempdir().unwrap();
        let native_name: PathBuf = name.split('/').collect();
        let path = dir.path().join(&native_name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let content = if name == "pyproject.toml" {
            "[tool.ryl]\n"
        } else {
            "original\n"
        };
        fs::write(&path, content).unwrap();
        let (code, _, stderr) =
            run(ryl(dir.path()).current_dir(dir.path()).arg("init"));
        assert_eq!(code, 2, "{name}: {stderr}");
        assert!(
            stderr.contains(&native_name.display().to_string()),
            "{stderr}"
        );
        assert!(stderr.contains("configuration already exists"), "{stderr}");
        if name.starts_with(".yamllint") {
            assert!(
                stderr.contains("--migrate-configs")
                    && stderr.contains("--migrate-write"),
                "{stderr}"
            );
        }
        assert_eq!(fs::read_to_string(&path).unwrap(), content);
        if name != ".ryl.toml" {
            assert!(!dir.path().join(".ryl.toml").exists());
        }
    }
}

#[test]
fn target_directory_is_refused() {
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join(".ryl.toml")).unwrap();
    let (code, _, stderr) = run(ryl(dir.path()).current_dir(dir.path()).arg("init"));
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains(".ryl.toml"), "{stderr}");
    assert!(dir.path().join(".ryl.toml").is_dir());
}

#[test]
fn ancestor_config_is_superseded_without_inheritance() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[lint.rules.document-end]\npresent = true\n",
    )
    .unwrap();
    let child = dir.path().join("child");
    fs::create_dir(&child).unwrap();
    let (code, _, stderr) = run(ryl(dir.path()).current_dir(&child).arg("init"));
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stderr.contains("takes over for this subtree")
            && stderr.contains("no inheritance"),
        "{stderr}"
    );
    assert!(
        stderr.contains(&dir.path().join(".ryl.toml").display().to_string()),
        "{stderr}"
    );
    fs::write(child.join("sample.yaml"), "---\nkey: value\n").unwrap();
    let (code, _, stderr) = run(ryl(dir.path())
        .current_dir(&child)
        .args(["check", "sample.yaml"]));
    assert_eq!(code, 0, "{stderr}");
}

#[test]
fn unsupported_presets_and_options_are_rejected() {
    let dir = tempdir().unwrap();
    for args in [
        vec!["--preset", "unknown"],
        vec!["--preset", "empty"],
        vec!["--force"],
        vec!["--output", "elsewhere.toml"],
    ] {
        let (code, _, stderr) = run(ryl(dir.path())
            .current_dir(dir.path())
            .arg("init")
            .args(&args));
        assert_eq!(code, 2, "{stderr}");
        assert!(stderr.contains(args[0]), "{stderr}");
        assert!(!dir.path().join(".ryl.toml").exists());
    }
}

#[test]
fn presets_match_effective_builtin_yaml_configs() {
    for preset in [Preset::Default, Preset::Relaxed] {
        let name = preset.name();
        let embedded = preset.toml();
        let toml = YamlLintConfig::from_toml_str(embedded).unwrap();
        let yaml = YamlLintConfig::from_yaml_str(&format!("extends: {name}")).unwrap();
        assert_eq!(toml.to_toml_string(), yaml.to_toml_string(), "{name}");
    }
}

#[test]
fn unreadable_or_malformed_pyproject_is_reported() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("pyproject.toml");
    fs::create_dir(&path).unwrap();
    assert_init_io_failure(dir.path(), "pyproject.toml");
    fs::remove_dir(&path).unwrap();
    fs::write(&path, "[invalid").unwrap();
    assert_init_io_failure(dir.path(), "pyproject.toml");
}

fn assert_init_io_failure(dir: &Path, name: &str) {
    let (code, _, stderr) = run(ryl(dir).current_dir(dir).arg("init"));
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains(name), "{stderr}");
    assert!(!dir.join(".ryl.toml").exists());
}

#[cfg(unix)]
#[test]
fn dangling_target_symlink_is_preserved() {
    let dir = tempdir().unwrap();
    let path = dir.path().join(".ryl.toml");
    std::os::unix::fs::symlink("missing.toml", &path).unwrap();
    let (code, _, stderr) = run(ryl(dir.path()).current_dir(dir.path()).arg("init"));
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains(".ryl.toml"), "{stderr}");
    assert_eq!(fs::read_link(&path).unwrap(), Path::new("missing.toml"));
    assert!(!dir.path().join("missing.toml").exists());
}

#[cfg(unix)]
#[test]
fn inaccessible_candidate_and_unwritable_directory_are_reported() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join(".config");
    fs::create_dir(&config_dir).unwrap();
    fs::set_permissions(&config_dir, fs::Permissions::from_mode(0o000)).unwrap();
    assert_init_io_failure(dir.path(), ".config");
    fs::set_permissions(&config_dir, fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500)).unwrap();
    assert_init_io_failure(dir.path(), ".ryl.toml");
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
}

#[cfg(unix)]
#[test]
fn deleted_working_directory_is_reported() {
    let dir = tempdir().unwrap();
    let child = dir.path().join("vanished");
    fs::create_dir(&child).unwrap();
    let (code, _, stderr) =
        run(std::process::Command::new("sh").current_dir(&child).args([
            "-c",
            "rmdir \"$PWD\"; exec \"$1\" init",
            "sh",
            env!("CARGO_BIN_EXE_ryl"),
        ]));
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains(".ryl.toml"), "{stderr}");
}

#[test]
fn ancestor_config_read_failure_is_reported() {
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("pyproject.toml")).unwrap();
    let child = dir.path().join("child");
    fs::create_dir(&child).unwrap();
    let (code, _, stderr) = run(ryl(dir.path()).current_dir(&child).arg("init"));
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("pyproject.toml"), "{stderr}");
    assert!(!child.join(".ryl.toml").exists());
}

#[test]
fn kubernetes_flow_conversion_quotes_urls_and_preserves_wrapped_expressions() {
    let dir = tempdir().unwrap();
    let cfg = format!(
        "target-version = '1.1'\n{}\n[format]\nmapping-style = 'block'\nsequence-style = 'flow'\nprose-wrap = 'preserve'\n",
        Preset::Kubernetes.toml()
    );
    fs::write(dir.path().join(".ryl.toml"), cfg).unwrap();
    let input = "apiVersion: v1\nkind: Pod\nmetadata:\n  name: formatter-compatibility\nspec:\n  containers:\n    - name: worker\n      image: busybox\n      args:\n        - /health?ready=true\nexpression: has(object.spec.foo) &&\n  object.spec.foo?first:second\n";
    let path = dir.path().join("pod.yaml");
    fs::write(&path, input).unwrap();
    let (code, _, stderr) = run(ryl(dir.path()).current_dir(dir.path()).args([
        "format",
        "--no-cache",
        "pod.yaml",
    ]));
    assert_eq!(code, 0, "{stderr}");
    let formatted = fs::read_to_string(&path).unwrap();
    assert!(
        formatted.contains("args: ['/health?ready=true']"),
        "{formatted}"
    );
    assert!(
        formatted.ends_with(
            "expression: has(object.spec.foo) &&\n  object.spec.foo?first:second\n"
        ),
        "{formatted}"
    );
    let (code, _, stderr) = run(ryl(dir.path()).current_dir(dir.path()).args([
        "check",
        "--fix",
        "--no-cache",
        "pod.yaml",
    ]));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(fs::read_to_string(&path).unwrap(), formatted);
    fs::write(&path, input).unwrap();
    let (code, _, stderr) = run(ryl(dir.path()).current_dir(dir.path()).args([
        "check",
        "--fix",
        "--no-cache",
        "pod.yaml",
    ]));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(fs::read_to_string(&path).unwrap(), input);
}

#[test]
fn reader_presets_only_add_flow_quote_requirements() {
    for preset in [
        Preset::Ansible,
        Preset::Cloudformation,
        Preset::Kubernetes,
        Preset::GithubActions,
        Preset::Compose,
        Preset::GitlabCi,
        Preset::HomeAssistant,
        Preset::Helm,
    ] {
        let cfg = YamlLintConfig::from_toml_str(preset.toml()).unwrap();
        let rule = ryl::rules::quoted_strings::Config::resolve(&cfg);
        let input = "{?foo: \"word\", :foo: \"word\"}\n";
        assert_eq!(
            ryl::rules::quoted_strings::check(input, &rule).len(),
            2,
            "{}",
            preset.name()
        );
        let fixed = ryl::rules::quoted_strings::fix(input, &rule).unwrap();
        assert!(
            ryl::rules::quoted_strings::check(&fixed, &rule).is_empty(),
            "{fixed}"
        );
        if !matches!(preset, Preset::Helm) {
            assert!(
                ryl::rules::quoted_strings::check(
                    "plain: word\nquestion: a?b\ncolon: a:b\nquoted: \"word\"\n",
                    &rule
                )
                .is_empty(),
                "{}",
                preset.name()
            );
        }
    }
}
