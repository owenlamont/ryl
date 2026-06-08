use std::fs;
use std::process::Command;

use tempfile::tempdir;

mod common;
use common::cli::{command_output, run};

/// `tags` is a ryl-only rule, so it is configured through TOML rather than the
/// yamllint-compatible YAML config that `-d` carries.
fn lint_with_toml_config(content: &str, config: &str) -> (i32, String) {
    let dir = tempdir().unwrap();
    let file = dir.path().join("doc.yaml");
    fs::write(&file, content).unwrap();
    let config_path = dir.path().join(".ryl.toml");
    fs::write(&config_path, config).unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config_path).arg(&file));
    (code, command_output(&stdout, &stderr).to_string())
}

#[test]
fn unsafe_tags_flagged_for_core_and_local_namespaces() {
    let (code, output) = lint_with_toml_config(
        "exec: !!python/object/apply:os.system [\"id\"]\nobj: !ruby/object:Foo {}\nplain: !!str value\n",
        "[rules.tags]\nforbid-unsafe-tags = true\n",
    );
    assert_eq!(code, 1, "unsafe tags should fail: {output}");
    assert!(
        output.contains("forbidden unsafe tag \"!!python/object/apply:os.system\""),
        "core-schema python tag message missing: {output}"
    );
    assert!(output.contains("1:7"), "python tag position: {output}");
    assert!(
        output.contains("forbidden unsafe tag \"!ruby/object:Foo\""),
        "local ruby tag message missing: {output}"
    );
    assert!(output.contains("2:6"), "ruby tag position: {output}");
    assert!(output.contains("tags"), "rule id missing: {output}");
    assert!(
        !output.contains("!!str"),
        "standard core tag must not be flagged as unsafe: {output}"
    );
}

#[test]
fn removed_yaml_1_1_types_flagged_for_core_schema_only() {
    let (code, output) = lint_with_toml_config(
        "a: !!omap []\nb: !!set {}\nc: !env X\nd: !!str s\n",
        "[rules.tags]\nforbid-removed-types = true\n",
    );
    assert_eq!(code, 1, "removed types should fail: {output}");
    assert!(
        output.contains("forbidden removed YAML 1.1 type \"!!omap\""),
        "omap message missing: {output}"
    );
    assert!(output.contains("1:4"), "omap position: {output}");
    assert!(
        output.contains("forbidden removed YAML 1.1 type \"!!set\""),
        "set message missing: {output}"
    );
    assert!(output.contains("2:4"), "set position: {output}");
    assert!(
        !output.contains("!env"),
        "local tag is not a removed core type: {output}"
    );
    assert!(
        !output.contains("!!str"),
        "standard core type must not be flagged: {output}"
    );
}

#[test]
fn allowed_tags_flags_only_unlisted_custom_tags() {
    let (code, output) = lint_with_toml_config(
        "a: !env X\nb: !keep Y\nc: !!omap []\nd: !!str s\n",
        "[rules.tags]\nallowed-tags = [\"!keep\"]\n",
    );
    assert_eq!(code, 1, "unlisted custom tag should fail: {output}");
    assert!(
        output.contains("tag \"!env\" is not in allowed-tags"),
        "unlisted tag message missing: {output}"
    );
    assert!(output.contains("1:4"), "!env position: {output}");
    assert!(
        !output.contains("!keep"),
        "allowlisted tag must not be flagged: {output}"
    );
    assert!(
        !output.contains("!!omap"),
        "core-schema tag is not policed by allowed-tags: {output}"
    );
}

#[test]
fn enabled_with_all_options_off_reports_nothing() {
    let (code, output) = lint_with_toml_config(
        "a: !env X\nb: !!omap []\nc: !!python/object:Foo {}\n",
        "[rules]\ntags = \"enable\"\n",
    );
    assert_eq!(code, 0, "no option enabled means no diagnostics: {output}");
    assert!(output.trim().is_empty(), "expected no output: {output}");
}

#[test]
fn multibyte_key_column_is_char_based() {
    let (code, output) = lint_with_toml_config(
        "café: !!omap []\n",
        "[rules.tags]\nforbid-removed-types = true\n",
    );
    assert_eq!(code, 1, "removed type should fail: {output}");
    assert!(
        output.contains("1:7"),
        "char-based column past the multibyte key: {output}"
    );
}

#[test]
fn tags_rule_does_not_fire_when_not_enabled() {
    let (code, output) = lint_with_toml_config(
        "exec: !!python/object/apply:os.system [\"id\"]\n",
        "[rules]\ntruthy = \"enable\"\n",
    );
    assert_eq!(code, 0, "tags off by default: {output}");
    assert!(
        !output.contains("tags"),
        "tags must not run unless enabled: {output}"
    );
}

#[test]
fn verbatim_and_javax_tag_spellings_are_normalised_and_detected() {
    let (code, output) = lint_with_toml_config(
        "a: !<tag:yaml.org,2002:omap> []\nb: !<!python/object> {}\nc: !!javax.script.ScriptEngineManager {}\n",
        "[rules.tags]\nforbid-unsafe-tags = true\nforbid-removed-types = true\n",
    );
    assert_eq!(code, 1, "verbatim/javax tags should fail: {output}");
    assert!(
        output.contains("forbidden removed YAML 1.1 type \"!!omap\""),
        "verbatim core tag should normalise to !!omap: {output}"
    );
    assert!(
        output.contains("forbidden unsafe tag \"!<!python/object>\""),
        "verbatim local construction tag should preserve its spelling: {output}"
    );
    assert!(
        output.contains("forbidden unsafe tag \"!!javax.script.ScriptEngineManager\""),
        "javax gadget namespace should be flagged: {output}"
    );
}

#[test]
fn custom_tag_directive_handle_is_not_namespace_matched() {
    let (code, output) = lint_with_toml_config(
        "%TAG !e! tag:example.com,2000:\n---\nx: !e!python/object value\n",
        "[rules.tags]\nforbid-unsafe-tags = true\n",
    );
    assert_eq!(
        code, 0,
        "a custom %TAG handle resolves its suffix into an unrelated namespace and must not be flagged: {output}"
    );
    assert!(
        output.trim().is_empty(),
        "expected no diagnostics: {output}"
    );
}

#[test]
fn custom_tag_directive_handle_is_allowlisted_as_written() {
    let (code, output) = lint_with_toml_config(
        "%TAG !e! tag:example.com,2000:\n---\na: !e!keep value\nb: !e!other value\n",
        "[rules.tags]\nallowed-tags = [\"!e!keep\"]\n",
    );
    assert_eq!(code, 1, "unlisted custom handle tag should fail: {output}");
    assert!(
        !output.contains("!e!keep"),
        "author-spelled allowlisted tag must not be flagged: {output}"
    );
    assert!(
        output.contains("tag \"!e!other\" is not in allowed-tags"),
        "diagnostic should preserve the author's custom handle: {output}"
    );
    assert!(
        !output.contains("tag:example.com,2000:"),
        "resolved URI should not replace the author's spelling: {output}"
    );

    let (code, output) = lint_with_toml_config(
        "%TAG !e! tag:example.com,2000:\n---\na: !e!keep value\n",
        "[rules.tags]\nallowed-tags = [\"tag:example.com,2000:keep\"]\n",
    );
    assert_eq!(
        code, 1,
        "resolved URI must not allow a differently-spelled tag: {output}"
    );
    assert!(
        output.contains("tag \"!e!keep\" is not in allowed-tags"),
        "allowlist matching should use the author's spelling: {output}"
    );
}

#[test]
fn non_specific_bare_tag_is_not_flagged() {
    let (code, output) = lint_with_toml_config(
        "a: ! plain\n",
        "[rules.tags]\nallowed-tags = [\"!keep\"]\n",
    );
    assert_eq!(
        code, 0,
        "the non-specific '!' tag carries no signal: {output}"
    );
    assert!(
        output.trim().is_empty(),
        "expected no diagnostics: {output}"
    );
}

#[test]
fn non_specific_bare_tag_stays_exempt_under_tag_directive() {
    // A `%TAG` directive must not turn the non-specific `!` into a flagged
    // custom tag; the exemption keys on the suffix, not the handle.
    let (code, output) = lint_with_toml_config(
        "%TAG ! tag:example.com,2000:\n---\na: ! plain\n",
        "[rules.tags]\nallowed-tags = [\"!keep\"]\n",
    );
    assert_eq!(
        code, 0,
        "bare ! must stay exempt under a %TAG directive: {output}"
    );
    assert!(
        output.trim().is_empty(),
        "expected no diagnostics: {output}"
    );
}

#[test]
fn tag_on_trailing_empty_scalar_points_at_its_content_line() {
    let (code, output) = lint_with_toml_config(
        "x: 1\nb: !!omap\n",
        "[rules.tags]\nforbid-removed-types = true\n",
    );
    assert_eq!(
        code, 1,
        "trailing empty tagged scalar should fail: {output}"
    );
    assert!(
        output.contains("2:4"),
        "must point at the tag token: {output}"
    );
    assert!(
        !output.contains("3:1"),
        "must not overshoot onto the trailing empty segment: {output}"
    );
}

#[test]
fn block_collection_tag_points_at_tag_and_disable_line_suppresses_it() {
    let config = "[rules.tags]\nallowed-tags = [\"!keep\"]\n";
    let (code, output) = lint_with_toml_config("a: !env\n  - x\n", config);
    assert_eq!(
        code, 1,
        "unlisted block collection tag should fail: {output}"
    );
    assert!(
        output.contains("1:4"),
        "diagnostic should point at the tag rather than the collection: {output}"
    );
    assert!(
        !output.contains("2:3"),
        "diagnostic must not point at the collection: {output}"
    );

    let (code, output) =
        lint_with_toml_config("a: !env # ryl disable-line rule:tags\n  - x\n", config);
    assert_eq!(
        code, 0,
        "disable-line on the explicit tag line should suppress it: {output}"
    );
    assert!(output.trim().is_empty(), "expected no output: {output}");
}

#[test]
fn tag_on_implicit_scalar_without_trailing_newline_stays_in_bounds() {
    let (code, output) = lint_with_toml_config(
        "!!python/object",
        "[rules.tags]\nforbid-unsafe-tags = true\n",
    );
    assert_eq!(code, 1, "unsafe tag should fail: {output}");
    assert!(
        output.contains("1:1"),
        "position must stay on the only line, not overshoot to line 2: {output}"
    );
    assert!(
        !output.contains("2:1"),
        "must not report an out-of-bounds line: {output}"
    );
}

#[test]
fn tags_rule_is_rejected_in_yaml_config() {
    // `tags` is ryl-only; yamllint-compatible YAML config (here via `-d`) must
    // reject it rather than silently linting or clashing with a future yamllint
    // `tags` rule.
    let dir = tempdir().unwrap();
    let file = dir.path().join("doc.yaml");
    fs::write(&file, "a: !!omap []\n").unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("-d")
        .arg("rules: {tags: {forbid-removed-types: true}}")
        .arg(&file));
    assert_eq!(
        code, 2,
        "a ryl-only rule in YAML config is a usage error: stdout={stdout} stderr={stderr}"
    );
    let output = command_output(&stdout, &stderr);
    assert!(
        output.contains("tags"),
        "error should name the rule: {output}"
    );
    assert!(
        output.to_lowercase().contains("toml"),
        "error should point to TOML config: {output}"
    );
}

#[test]
fn per_file_ignores_accept_the_tags_rule_name() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("ignored.yaml");
    fs::write(&file, "a: !!omap []\n").unwrap();
    let config = dir.path().join(".ryl.toml");
    fs::write(
        &config,
        format!(
            "[rules.tags]\nforbid-removed-types = true\n[per-file-ignores]\n'{}' = ['tags']\n",
            file.display()
        ),
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(
        code, 0,
        "per-file-ignores should suppress tags: stdout={stdout} stderr={stderr}"
    );
    assert!(stdout.trim().is_empty(), "expected no stdout: {stdout}");
    assert!(stderr.trim().is_empty(), "expected no stderr: {stderr}");
}

#[test]
fn rule_ignore_skips_file() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("ignored.yaml");
    fs::write(&file, "value: !!omap []\n").unwrap();
    let config = dir.path().join(".ryl.toml");
    fs::write(
        &config,
        "[rules.tags]\nforbid-removed-types = true\nignore = [\"ignored.yaml\"]\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(
        code, 0,
        "ignored file should pass: stdout={stdout} stderr={stderr}"
    );
    assert!(stdout.trim().is_empty(), "expected no stdout: {stdout}");
    assert!(stderr.trim().is_empty(), "expected no stderr: {stderr}");
}
