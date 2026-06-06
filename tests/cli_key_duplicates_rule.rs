use std::fs;
use std::process::Command;

use tempfile::tempdir;

fn run(cmd: &mut Command) -> (i32, String, String) {
    let out = cmd.output().expect("process");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

fn command_output<'a>(stdout: &'a str, stderr: &'a str) -> &'a str {
    if stderr.is_empty() { stdout } else { stderr }
}

/// Lint `yaml` with a TOML config whose `[rules.key-duplicates]` body is
/// `options`, returning the exit code and whichever stream carried output.
fn run_toml(options: &str, yaml: &str) -> (i32, String) {
    let dir = tempdir().unwrap();
    let file = dir.path().join("in.yaml");
    fs::write(&file, yaml).unwrap();
    let config = dir.path().join("ryl.toml");
    fs::write(&config, format!("[rules.key-duplicates]\n{options}")).unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    (code, command_output(&stdout, &stderr).to_string())
}

#[test]
fn canonical_treats_numeric_spellings_of_one_integer_as_duplicate() {
    let (code, output) = run_toml("check-canonical = true\n", "0xB: a\n11: b\n");
    assert_eq!(code, 1, "expected canonical integer duplicate: {output}");
    assert!(
        output.contains("duplication of key \"11\" in mapping"),
        "missing canonical duplicate message: {output}"
    );
}

#[test]
fn canonical_reports_each_duplicate_integer_spelling() {
    let (code, output) =
        run_toml("check-canonical = true\n", "0xB: a\n11: b\n0o13: c\n");
    assert_eq!(code, 1, "expected two canonical duplicates: {output}");
    assert_eq!(
        output.matches("duplication of key").count(),
        2,
        "both `11` and `0o13` should be flagged as the integer 11: {output}"
    );
}

#[test]
fn canonical_ignores_plain_alias_values() {
    let (code, output) = run_toml(
        "check-canonical = true\n",
        "anchor: &a 1\nref: *a\nother: *a\n",
    );
    assert_eq!(
        code, 0,
        "a plain alias value outside a merge is not a duplicate: {output}"
    );
}

#[test]
fn canonical_handles_complex_mapping_keys() {
    // A sequence used as a mapping key: the value of a complex (non-scalar) key
    // exercises the no-pending-scalar-key path and must not panic.
    let (code, output) = run_toml("check-canonical = true\n", "?\n  - a\n: 1\n");
    assert_eq!(
        code, 0,
        "a complex mapping key is not a duplicate: {output}"
    );
}

#[test]
fn canonical_keeps_quoted_string_distinct_from_integer() {
    let (code, output) = run_toml("check-canonical = true\n", "\"11\": a\n11: b\n");
    assert_eq!(
        code, 0,
        "quoted string key must not collide with integer key: {output}"
    );
}

#[test]
fn canonical_treats_null_spellings_as_duplicate() {
    let (code, output) = run_toml("check-canonical = true\n", "~: a\nnull: b\n");
    assert_eq!(code, 1, "expected null-spelling duplicate: {output}");
    assert!(
        output.contains("duplication of key \"null\" in mapping"),
        "missing null duplicate message: {output}"
    );
}

#[test]
fn canonical_falls_back_to_text_for_locally_tagged_keys() {
    let (code, output) = run_toml("check-canonical = true\n", "!foo 11: a\n11: b\n");
    assert_eq!(
        code, 0,
        "a locally tagged key must not collide with an integer: {output}"
    );
}

#[test]
fn canonical_falls_back_to_text_for_unparsable_core_tagged_keys() {
    let (code, output) = run_toml(
        "check-canonical = true\n",
        "!!bool maybe: a\n!!bool maybe: b\n",
    );
    assert_eq!(code, 1, "expected text-fallback duplicate: {output}");
    assert!(
        output.contains("duplication of key \"maybe\" in mapping"),
        "missing text-fallback duplicate message: {output}"
    );
}

#[test]
fn canonical_flags_merge_vs_merge_collision() {
    let (code, output) = run_toml(
        "check-canonical = true\n",
        "a: &a {x: 1}\nb: &b {x: 2}\nc:\n  <<: [*a, *b]\n",
    );
    assert_eq!(code, 1, "expected merge-vs-merge collision: {output}");
    assert!(
        output.contains("duplication of key \"x\" in merged mappings"),
        "missing merge collision message: {output}"
    );
}

#[test]
fn canonical_treats_reordered_mapping_values_as_equal() {
    // Mapping key order is insignificant, so two sources whose shared key has the
    // same mapping value written in different key order do not collide.
    let (code, output) = run_toml(
        "check-canonical = true\n",
        "a: &a {x: {p: 1, q: 2}}\nb: &b {x: {q: 2, p: 1}}\nc:\n  <<: [*a, *b]\n",
    );
    assert_eq!(
        code, 0,
        "reordered but equivalent mapping values must not collide: {output}"
    );
}

#[test]
fn canonical_flags_collision_through_an_anchored_sequence_merge_source() {
    let (code, output) = run_toml(
        "check-canonical = true\n",
        "a: &a {x: 1}\nb: &b {x: 2}\nsources: &sources [*a, *b]\nhost:\n  <<: *sources\n",
    );
    assert_eq!(
        code, 1,
        "expected collision via anchored sequence: {output}"
    );
    assert!(
        output.contains("duplication of key \"x\" in merged mappings"),
        "an anchored sequence used as a merge value must resolve its mappings: {output}"
    );
}

#[test]
fn canonical_distinguishes_differing_tagged_values() {
    let (code, output) = run_toml(
        "check-canonical = true\n",
        "a: &a {x: !foo 1}\nb: &b {x: !bar 1}\nc:\n  <<: [*a, *b]\n",
    );
    assert_eq!(
        code, 1,
        "differing local tags are different values: {output}"
    );
    assert!(
        output.contains("duplication of key \"x\" in merged mappings"),
        "values with different local tags must collide: {output}"
    );
}

#[test]
fn canonical_distinguishes_a_non_default_core_tag() {
    let (code, output) = run_toml(
        "check-canonical = true\n",
        "a: &a {x: {item: 1}}\nb: &b {x: !!set {item: 1}}\nc:\n  <<: [*a, *b]\n",
    );
    assert_eq!(
        code, 1,
        "a !!set value differs from a plain mapping: {output}"
    );
    assert!(
        output.contains("duplication of key \"x\" in merged mappings"),
        "a non-default core tag must distinguish the value: {output}"
    );
}

#[test]
fn canonical_resolves_explicitly_tagged_integer_radixes() {
    // An explicit !!int must canonicalize the same spellings as an untagged
    // integer, so `!!int 0xB` (and `!!int 0o13`) collide with `11`.
    for tagged in ["!!int 0xB", "!!int 0o13"] {
        let (code, output) =
            run_toml("check-canonical = true\n", &format!("{tagged}: a\n11: b\n"));
        assert_eq!(code, 1, "expected `{tagged}` to collide with 11: {output}");
        assert!(
            output.contains("duplication of key \"11\" in mapping"),
            "`{tagged}` should canonicalize to integer 11: {output}"
        );
    }
}

#[test]
fn canonical_treats_a_default_core_tag_as_untagged() {
    let (code, output) = run_toml(
        "check-canonical = true\n",
        "a: &a {x: !!int 1}\nb: &b {x: 1}\nc:\n  <<: [*a, *b]\n",
    );
    assert_eq!(
        code, 0,
        "an explicit !!int resolves to the same value as a plain integer: {output}"
    );
}

#[test]
fn canonical_recognizes_an_explicitly_tagged_merge_key() {
    // A `<<` carrying an explicit `!!merge` tag is a merge directive even though
    // it is quoted, so its sources are expanded and the collision is found.
    let (code, output) = run_toml(
        "check-canonical = true\n",
        "a: &a {x: 1}\nb: &b {x: 2}\nc:\n  !!merge '<<': [*a, *b]\n",
    );
    assert_eq!(
        code, 1,
        "expected collision via explicit merge tag: {output}"
    );
    assert!(
        output.contains("duplication of key \"x\" in merged mappings"),
        "an !!merge-tagged key must drive merge expansion: {output}"
    );
}

#[test]
fn canonical_treats_a_non_merge_tagged_double_angle_as_a_string_key() {
    // `!!str '<<'` is an ordinary string key, not a merge directive, so its
    // value is not expanded as a merge.
    let (code, output) = run_toml(
        "check-canonical = true\n",
        "base: &b {k: 1}\nm:\n  !!str '<<': *b\n  k: 2\n",
    );
    assert_eq!(
        code, 0,
        "a non-merge-tagged `<<` must not drive merge expansion: {output}"
    );
}

#[test]
fn canonical_merges_a_repeated_alias_at_most_once() {
    // Two `<<: *b` keys merge the same anchor; it contributes once (idempotent),
    // so there is no merge-vs-merge collision and the duplicate `<<` is exempt.
    let (code, output) = run_toml(
        "check-canonical = true\n",
        "base: &b {x: 1}\nh:\n  <<: *b\n  <<: *b\n",
    );
    assert_eq!(
        code, 0,
        "merging one anchor twice contributes its keys once: {output}"
    );
}

#[test]
fn canonical_does_not_flag_same_anchor_merged_twice() {
    let (code, output) = run_toml(
        "check-canonical = true\n",
        "base: &b {x: 1}\ntop:\n  <<: [*b, *b]\n",
    );
    assert_eq!(
        code, 0,
        "merging one anchor twice loses nothing and must not be flagged: {output}"
    );
}

#[test]
fn canonical_does_not_flag_merge_sources_that_agree() {
    let (code, output) = run_toml(
        "check-canonical = true\n",
        "a: &a {x: 1}\nb: &b {x: 1}\nc:\n  <<: [*a, *b]\n",
    );
    assert_eq!(
        code, 0,
        "two merge sources with the same value for a key lose nothing: {output}"
    );
}

#[test]
fn shadowing_ignores_a_same_value_override() {
    let (code, output) = run_toml(
        "forbid-merge-key-shadowing = true\n",
        "d: &d {timeout: 30}\np:\n  <<: *d\n  timeout: 30\n",
    );
    assert_eq!(
        code, 0,
        "re-specifying a merged key to its own value changes nothing: {output}"
    );
}

#[test]
fn canonical_resolves_a_reused_base_that_merges_and_overrides() {
    // `&base` is itself built by merging two sources that share a key (with the
    // same value) and then overrides a merged key; reusing it via `*base` must
    // resolve its effective key set without a false positive.
    let (code, output) = run_toml(
        "check-canonical = true\n",
        "d1: &d1 {shared: 5, a: 1}\nd2: &d2 {shared: 5, b: 2}\nbase: &base\n  <<: [*d1, *d2]\n  own: 1\n  shared: 9\nuser:\n  <<: *base\n",
    );
    assert_eq!(
        code, 0,
        "a transitively-merged, overriding base must resolve cleanly: {output}"
    );
}

#[test]
fn canonical_does_not_flag_intentional_merge_override() {
    let (code, output) = run_toml(
        "check-canonical = true\n",
        "base: &b {x: 1}\nthing:\n  <<: *b\n  x: 2\n",
    );
    assert_eq!(
        code, 0,
        "an explicit override of a merged key is not a duplicate: {output}"
    );
}

#[test]
fn canonical_does_not_flag_explicit_then_merge_override() {
    let (code, output) = run_toml(
        "check-canonical = true\n",
        "base: &b {x: 1}\nthing:\n  x: 2\n  <<: *b\n",
    );
    assert_eq!(
        code, 0,
        "an explicit key shadowed by a later merge is allowed under check-canonical: {output}"
    );
}

#[test]
fn shadowing_flags_merge_then_explicit_override() {
    let (code, output) = run_toml(
        "forbid-merge-key-shadowing = true\n",
        "base: &b {x: 1}\nthing:\n  <<: *b\n  x: 2\n",
    );
    assert_eq!(code, 1, "expected shadowing report: {output}");
    assert!(
        output.contains("duplication of key \"x\" in mapping"),
        "missing shadowing message: {output}"
    );
}

#[test]
fn shadowing_flags_explicit_then_merge_override() {
    let (code, output) = run_toml(
        "forbid-merge-key-shadowing = true\n",
        "base: &b {x: 1}\nthing:\n  x: 2\n  <<: *b\n",
    );
    assert_eq!(code, 1, "expected shadowing report at merge line: {output}");
    assert!(
        output.contains("duplication of key \"x\" in merged mappings"),
        "missing shadowing-at-merge message: {output}"
    );
}

#[test]
fn shadowing_flags_inline_mapping_merge_value() {
    let (code, output) = run_toml(
        "forbid-merge-key-shadowing = true\n",
        "thing:\n  <<: {x: 1}\n  x: 2\n",
    );
    assert_eq!(code, 1, "expected inline-merge shadowing: {output}");
    assert!(
        output.contains("duplication of key \"x\" in mapping"),
        "missing inline-merge shadowing message: {output}"
    );
}

#[test]
fn canonical_flags_inline_mappings_in_merge_sequence() {
    let (code, output) =
        run_toml("check-canonical = true\n", "c:\n  <<: [{x: 1}, {x: 2}]\n");
    assert_eq!(
        code, 1,
        "expected inline merge-sequence collision: {output}"
    );
    assert!(
        output.contains("duplication of key \"x\" in merged mappings"),
        "missing inline merge-sequence message: {output}"
    );
}

#[test]
fn merge_of_scalar_anchor_is_ignored_safely() {
    let (code, output) = run_toml(
        "check-canonical = true\n",
        "a: &a 5\nc:\n  <<: *a\n  y: 1\n",
    );
    assert_eq!(
        code, 0,
        "merging a scalar anchor contributes no keys and must not error: {output}"
    );
}

#[test]
fn canonical_resolves_transitive_merge_keys() {
    let (code, output) = run_toml(
        "check-canonical = true\n",
        "a: &a {x: 1}\nb: &b\n  <<: *a\n  z: 2\nc: &c {x: 9}\nd:\n  <<: [*b, *c]\n",
    );
    assert_eq!(
        code, 1,
        "x reaches the host through *b's own merge and collides with *c: {output}"
    );
    assert!(
        output.contains("duplication of key \"x\" in merged mappings"),
        "missing transitive merge collision message: {output}"
    );
}

#[test]
fn scalar_value_for_merge_key_does_not_taint_later_values() {
    let (code, output) = run_toml(
        "forbid-merge-key-shadowing = true\n",
        "thing:\n  <<: plain\n  list: [m, n]\n  nested: {x: 1}\n  x: 2\n",
    );
    assert_eq!(
        code, 0,
        "a plain `<<` value is not a merge and must not mark later values as merged: {output}"
    );
}

#[test]
fn each_document_resolves_its_own_anchors() {
    let (code, output) = run_toml(
        "check-canonical = true\n",
        "---\na: &a {x: 1}\nb: &b {x: 2}\nm:\n  <<: [*a, *b]\n---\nc: &c {y: 1}\nn:\n  <<: *c\n",
    );
    assert_eq!(code, 1, "first document collides: {output}");
    assert_eq!(
        output
            .matches("duplication of key \"x\" in merged mappings")
            .count(),
        1,
        "only the first document collides; the second is independent: {output}"
    );
}

#[test]
fn merge_source_that_uses_merge_key_does_not_leak_phantom_directive() {
    // The `&b` anchor composes via its own `<<`; merging it into `c` must not
    // surface a spurious `duplication of key "<<"`.
    let (code, output) = run_toml(
        "forbid-merge-key-shadowing = true\n",
        "a: &a {x: 1}\nb: &b\n  <<: *a\n  y: 2\nc:\n  <<: *b\n",
    );
    assert_eq!(
        code, 0,
        "a transitive `<<` base must not leak a phantom merge directive: {output}"
    );
}

#[test]
fn inline_merge_in_anchored_base_does_not_leak_directive() {
    let (code, output) = run_toml(
        "forbid-merge-key-shadowing = true\n",
        "base: &b\n  <<: {a: 1}\n  timeout: 30\nprod:\n  <<: *b\n  timeout: 60\n",
    );
    assert_eq!(
        code, 1,
        "the explicit timeout override should report: {output}"
    );
    assert!(
        !output.contains("duplication of key \"<<\""),
        "the base's own `<<` must not leak into the merged key set: {output}"
    );
    assert!(
        output.contains("duplication of key \"timeout\" in mapping"),
        "the legitimate timeout shadow should still report: {output}"
    );
}

#[test]
fn quoted_merge_key_is_not_treated_as_a_merge_directive() {
    let (code, output) = run_toml(
        "forbid-merge-key-shadowing = true\n",
        "base: &b {k: 1}\nm:\n  \"<<\": *b\n  k: 2\n",
    );
    assert_eq!(
        code, 0,
        "a quoted \"<<\" is a plain string key, not a merge directive: {output}"
    );
}

#[test]
fn canonical_still_reports_explicit_duplicate_after_a_merge() {
    // check-canonical must stay strictly additive: a real explicit/explicit
    // duplicate that the default rule catches is not swallowed by a merge.
    let (code, output) = run_toml(
        "check-canonical = true\n",
        "base: &b {k: 1}\nhost:\n  <<: *b\n  k: 2\n  k: 3\n",
    );
    assert_eq!(
        code, 1,
        "expected the explicit k duplicate to report: {output}"
    );
    assert!(
        output.contains("duplication of key \"k\" in mapping"),
        "explicit/explicit duplicate over a merged key must still fire: {output}"
    );
}

#[test]
fn nway_merge_collision_reports_a_single_diagnostic() {
    let (code, output) = run_toml(
        "check-canonical = true\n",
        "a: &a {x: 1}\nb: &b {x: 2}\nc: &c {x: 3}\nhost:\n  <<: [*a, *b, *c]\n",
    );
    assert_eq!(code, 1, "expected a merge collision: {output}");
    assert_eq!(
        output
            .matches("duplication of key \"x\" in merged mappings")
            .count(),
        1,
        "a 3-way collision on one key must report once, not twice: {output}"
    );
}

#[test]
fn forbid_duplicated_merge_keys_composes_with_canonical() {
    let (code, output) = run_toml(
        "check-canonical = true\nforbid-duplicated-merge-keys = true\n",
        "a: &a {p: 1}\nb: &b {q: 2}\nc:\n  <<: *a\n  <<: *b\n",
    );
    assert_eq!(code, 1, "expected duplicated merge key report: {output}");
    assert!(
        output.contains("duplication of key \"<<\" in mapping"),
        "missing duplicated merge-key message: {output}"
    );
}

#[test]
fn default_config_allows_quoted_merge_key_duplicates() {
    // yamllint keys the merge-key exemption on the resolved scalar value, so a
    // quoted "<<" is treated like a plain << — duplicates stay silent under the
    // default (option-free) config.
    let dir = tempdir().unwrap();
    let file = dir.path().join("in.yaml");
    fs::write(&file, "m:\n  \"<<\": 1\n  \"<<\": 2\n").unwrap();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  key-duplicates: enable\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(
        code, 0,
        "quoted merge-key duplicates must stay silent like yamllint: stdout={stdout} stderr={stderr}"
    );
}

#[test]
fn canonical_suppresses_merge_collision_when_host_overrides_the_key() {
    let (code, output) = run_toml(
        "check-canonical = true\n",
        "a: &a {x: 1}\nb: &b {x: 2}\nhost:\n  <<: [*a, *b]\n  x: 99\n",
    );
    assert_eq!(
        code, 0,
        "an explicit key resolves the merge conflict, so it is not flagged: {output}"
    );
}

#[test]
fn duplicate_keys_reported() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("dup.yaml");
    fs::write(&file, "foo: 1\nbar: 2\nfoo: 3\n").unwrap();

    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  key-duplicates: enable\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(code, 1, "expected failure: stdout={stdout} stderr={stderr}");
    let output = command_output(&stdout, &stderr);
    assert!(
        output.contains("duplication of key \"foo\" in mapping"),
        "missing key duplication message: {output}"
    );
    assert!(
        output.contains("key-duplicates"),
        "rule id missing: {output}"
    );
}

#[test]
fn merge_keys_allowed_by_default() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("merge.yaml");
    fs::write(
        &file,
        "anchor: &a\n  value: 1\nmerged:\n  <<: *a\n  <<: *a\n",
    )
    .unwrap();

    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  key-duplicates: enable\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(
        code, 0,
        "merge keys allowed by default: stdout={stdout} stderr={stderr}"
    );
    assert!(stdout.trim().is_empty(), "expected no stdout: {stdout}");
    assert!(stderr.trim().is_empty(), "expected no stderr: {stderr}");
}

#[test]
fn merge_keys_forbidden_when_configured() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("merge.yaml");
    fs::write(
        &file,
        "anchor: &a\n  value: 1\nmerged:\n  <<: *a\n  <<: *a\n",
    )
    .unwrap();

    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  key-duplicates:\n    forbid-duplicated-merge-keys: true\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(code, 1, "expected failure: stdout={stdout} stderr={stderr}");
    let output = command_output(&stdout, &stderr);
    assert!(
        output.contains("duplication of key \"<<\" in mapping"),
        "missing merge duplication message: {output}"
    );
    assert!(
        output.contains("key-duplicates"),
        "rule id missing: {output}"
    );
}
