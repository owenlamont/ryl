//! Property tests for `--diff` (`ryl::fix::diff_outcome`).
//!
//! `--diff` is a *preview* of `--fix`: it renders the safe-fix result as a unified
//! diff instead of writing it. The invariants here pin that the preview is both
//! **faithful** and **applicable**, across the safe-fix config matrix:
//!
//!  * *faithful* — a diff is emitted exactly when `--fix` would change the file, and
//!    a plain YAML file is never both diffed and skipped;
//!  * *applicable* — the emitted diff, applied by an **independent** implementation
//!    (`diffy`, not the `similar` crate that produced it), reproduces the fix output
//!    byte-for-byte. This is the contract a runner like hk relies on when it applies
//!    the diff itself rather than re-invoking ryl, and it stresses the unified-diff
//!    edge cases the generator already produces (CRLF, no-final-newline, multibyte).
//!
//! The YAML generator is reused from the safe-fix suite via `#[path]`
//! (`ast`/`strategy`/`config`) and wrapped into a markdown host (`wrap`), so the
//! embedded YAML matches the flat `--fix` suite. Deterministic siblings pin
//! known-dirty / CRLF / unparsable / markdown cases so the random property cannot
//! pass vacuously.

// Shared with the safe-fix suite (which uses every item); this binary reuses only
// the generator and a few helpers, so allow the rest to be unused.
#[path = "property_safe_fix/ast.rs"]
#[allow(dead_code)]
mod ast;
#[path = "property_safe_fix/config.rs"]
#[allow(dead_code)]
mod config;
#[path = "property_safe_fix/strategy.rs"]
mod strategy;
#[path = "property_markdown_fix/wrap.rs"]
mod wrap;

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use ryl::config::{SourceKind, YamlLintConfig};
use ryl::fix::{apply_safe_fixes, diff_outcome, fix_markdown_str};

use config::{safe_fix_configs, synthetic_base_dir, synthetic_path};
use strategy::arb_document;
use wrap::arb_markdown_doc;

/// Apply a unified diff with an implementation independent of the one that produced
/// it, so a round-trip proves the emitted diff is standard-compliant. Panicking here
/// is the intended failure mode: a diff ryl emits that a conforming applier cannot
/// parse or apply is a bug, and proptest shrinks the offending input.
fn apply_unified(original: &str, diff: &str) -> String {
    let patch = diffy::Patch::from_str(diff).unwrap_or_else(|err| {
        panic!("emitted diff must parse as a patch: {err}\n{diff}")
    });
    diffy::apply(original, &patch)
        .unwrap_or_else(|err| panic!("emitted diff must apply cleanly: {err}\n{diff}"))
}

fn assert_yaml_preview(input: &str) -> Result<(), TestCaseError> {
    for prepared in safe_fix_configs() {
        let cfg = &prepared.cfg;
        let fixed =
            apply_safe_fixes(input, cfg, synthetic_path(), synthetic_base_dir());
        let outcome = diff_outcome(
            input,
            cfg,
            synthetic_path(),
            synthetic_base_dir(),
            SourceKind::Yaml,
        );
        // A bare `\r` is diffed as line *content* (the `\n`-only renderer), so it
        // round-trips through `git apply`. The one residual case `similar` can't
        // render is a side that *ends* in a bare `\r`, which `--diff` skips and
        // points at `--fix`: "no diff, one skip" instead of a diff.
        if fixed != input && (input.ends_with('\r') || fixed.ends_with('\r')) {
            prop_assert!(
                outcome.diff.is_none() && !outcome.skipped.is_empty(),
                "a trailing-bare-CR change must be skipped, not diffed, under '{}'; input {:?}",
                prepared.name,
                input
            );
            continue;
        }
        prop_assert_eq!(
            outcome.diff.is_some(),
            fixed != input,
            "a diff must be emitted exactly when the fix changes the file under '{}'; input {:?}",
            prepared.name,
            input
        );
        if let Some(diff) = &outcome.diff {
            prop_assert!(
                outcome.skipped.is_empty(),
                "a diffed YAML file must not also be skipped under '{}'; input {:?}",
                prepared.name,
                input
            );
            let applied = apply_unified(input, diff);
            prop_assert_eq!(
                applied.as_str(),
                fixed.as_str(),
                "applying the emitted diff must reproduce the fix output under '{}'; input {:?}; diff {:?}",
                prepared.name,
                input,
                diff
            );
        }
    }
    Ok(())
}

fn assert_markdown_preview(input: &str) -> Result<(), TestCaseError> {
    for prepared in safe_fix_configs() {
        let cfg = &prepared.cfg;
        let fixed =
            fix_markdown_str(input, synthetic_path(), cfg, synthetic_base_dir());
        let outcome = diff_outcome(
            input,
            cfg,
            synthetic_path(),
            synthetic_base_dir(),
            SourceKind::Markdown,
        );
        prop_assert_eq!(
            outcome.diff.is_some(),
            fixed.is_some(),
            "a host-level diff must be emitted exactly when the markdown fix changes the file under '{}'; input {:?}",
            prepared.name,
            input
        );
        if let (Some(diff), Some(fixed)) = (&outcome.diff, &fixed) {
            let applied = apply_unified(input, diff);
            prop_assert_eq!(
                applied.as_str(),
                fixed.as_str(),
                "applying the emitted markdown diff must reproduce the fix output under '{}'; input {:?}; diff {:?}",
                prepared.name,
                input,
                diff
            );
        }
    }
    Ok(())
}

fn yaml_produces_a_diff(input: &str) -> bool {
    safe_fix_configs().iter().any(|prepared| {
        diff_outcome(
            input,
            &prepared.cfg,
            synthetic_path(),
            synthetic_base_dir(),
            SourceKind::Yaml,
        )
        .diff
        .is_some()
    })
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/proptest-regressions/property_diff.txt",
        ))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn yaml_diff_is_a_faithful_applicable_preview(document in arb_document()) {
        assert_yaml_preview(&document.render())?;
    }

    /// A leading `# ryl disable` mutes every rule, so the fix is a byte-for-byte
    /// no-op and `--diff` must emit nothing (mirrors property_safe_fix's strongest
    /// disable invariant).
    #[test]
    fn leading_disable_yields_no_diff(document in arb_document()) {
        let input = format!("# ryl disable\n{}", document.render());
        for prepared in safe_fix_configs() {
            let outcome = diff_outcome(
                &input,
                &prepared.cfg,
                synthetic_path(),
                synthetic_base_dir(),
                SourceKind::Yaml,
            );
            prop_assert!(
                outcome.diff.is_none(),
                "leading `# ryl disable` must produce no diff under '{}'; input {:?}",
                prepared.name,
                input
            );
        }
    }

    #[test]
    fn markdown_diff_is_a_faithful_applicable_preview(document in arb_markdown_doc()) {
        assert_markdown_preview(&document.render())?;
    }
}

#[test]
fn known_dirty_yaml_diff_round_trips() {
    // commas (space before `,`) and trailing-spaces both fire, so the "diff present"
    // branch is exercised on a concrete input — the random property cannot pass
    // vacuously if the generator drifts.
    let input = "items: [a ,b]  \n";
    assert!(
        yaml_produces_a_diff(input),
        "known-dirty input must produce a diff under at least one config"
    );
    assert_yaml_preview(input).expect("known-dirty YAML invariants hold");
}

#[test]
fn crlf_without_final_newline_diff_round_trips() {
    // CRLF + space-before-comma + no final newline: new-lines (enabled in the matrix)
    // rewrites CRLF->LF across the whole file, so the diff is a multi-line transform
    // — the case most likely to expose a malformed/inapplicable unified diff.
    let input = "a: [1 ,2]\r\nb: [3 ,4]";
    assert!(
        yaml_produces_a_diff(input),
        "CRLF dirty input must produce a diff"
    );
    assert_yaml_preview(input).expect("CRLF/no-final-newline invariants hold");
}

#[test]
fn unparsable_yaml_is_skipped_not_diffed() {
    let input = "[1, 2\n[3, 4\n";
    for prepared in safe_fix_configs() {
        let outcome = diff_outcome(
            input,
            &prepared.cfg,
            synthetic_path(),
            synthetic_base_dir(),
            SourceKind::Yaml,
        );
        assert!(
            outcome.diff.is_none(),
            "an unparsable file must not be diffed under '{}'",
            prepared.name
        );
        assert!(
            !outcome.skipped.is_empty(),
            "an unparsable file must be reported as skipped under '{}'",
            prepared.name
        );
    }
}

#[test]
fn crlf_preserving_config_diff_round_trips_via_diffy() {
    // Every config in the safe-fix matrix enables `new-lines` (CRLF->LF), so the
    // property matrix never feeds diffy a CRLF-*preserved* diff (CR on both sides). A
    // trailing-spaces-only config keeps CRLF, exercising the unified-diff shape with
    // embedded CR bytes that the hk / `git apply` consumer relies on for Windows files.
    let cfg = YamlLintConfig::from_yaml_str("rules:\n  trailing-spaces: enable\n")
        .expect("config parses");
    let input = "a:   1  \r\nb: 2  \r\nc: 3\r\n";
    let fixed = apply_safe_fixes(input, &cfg, synthetic_path(), synthetic_base_dir());
    assert_ne!(fixed, input, "trailing spaces must be removed");
    assert!(
        fixed.contains("\r\n"),
        "CRLF must be preserved (no new-lines rule)"
    );
    let outcome = diff_outcome(
        input,
        &cfg,
        synthetic_path(),
        synthetic_base_dir(),
        SourceKind::Yaml,
    );
    let diff = outcome
        .diff
        .expect("a CRLF file with trailing spaces must diff");
    assert_eq!(
        apply_unified(input, &diff),
        fixed,
        "diffy must round-trip a CRLF-preserved diff: {diff:?}"
    );
}

#[test]
fn bare_cr_as_content_diff_round_trips_via_diffy() {
    // A bare `\r` (not CRLF) mid-content is diffed as line *content* (the `\n`-only
    // renderer), so the emitted hunk lines stay `\n`-terminated and apply cleanly —
    // the contract `git apply -p0` relies on. trailing-spaces is CR-aware, so it
    // strips the run before the `\r` and before the `\n`; the file ends in `\n`, so
    // it is not the residual trailing-`\r` skip case.
    let cfg = YamlLintConfig::from_yaml_str("rules:\n  trailing-spaces: enable\n")
        .expect("config parses");
    let input = "a: 1  \rb: 2  \nc: 3\n";
    let fixed = apply_safe_fixes(input, &cfg, synthetic_path(), synthetic_base_dir());
    assert_eq!(
        fixed, "a: 1\rb: 2\nc: 3\n",
        "trailing spaces stripped, CR kept"
    );
    let outcome = diff_outcome(
        input,
        &cfg,
        synthetic_path(),
        synthetic_base_dir(),
        SourceKind::Yaml,
    );
    let diff = outcome
        .diff
        .expect("a bare-CR-content change must still diff");
    assert_eq!(
        apply_unified(input, &diff),
        fixed,
        "diffy must round-trip a bare-CR-content diff: {diff:?}"
    );
}

#[test]
fn trailing_bare_cr_change_is_skipped_not_diffed() {
    // `similar` counts a trailing `\r` as a line terminator, so a side that *ends* in
    // a bare `\r` can't be rendered as an applicable hunk; `--diff` skips it (use
    // `--fix`) rather than emit a corrupt patch.
    let cfg = YamlLintConfig::from_yaml_str("rules:\n  trailing-spaces: enable\n")
        .expect("config parses");
    let input = "a: 1  \rb: 2  \r";
    let fixed = apply_safe_fixes(input, &cfg, synthetic_path(), synthetic_base_dir());
    assert_ne!(fixed, input, "trailing spaces must be removed");
    assert!(fixed.ends_with('\r'), "fixed still ends in a bare CR");
    let outcome = diff_outcome(
        input,
        &cfg,
        synthetic_path(),
        synthetic_base_dir(),
        SourceKind::Yaml,
    );
    assert!(
        outcome.diff.is_none() && !outcome.skipped.is_empty(),
        "a trailing-bare-CR change must be skipped, not diffed"
    );
}

#[test]
fn known_dirty_markdown_diff_round_trips() {
    // A fenced block with a space before `,` is fixed at the host level, exercising
    // the markdown branch's "diff present" path deterministically.
    let input = "# Title\n\n```yaml\nitems: [a ,b]\n```\n";
    let cfg = &safe_fix_configs()[0].cfg;
    let outcome = diff_outcome(
        input,
        cfg,
        synthetic_path(),
        synthetic_base_dir(),
        SourceKind::Markdown,
    );
    assert!(
        outcome.diff.is_some(),
        "known-dirty markdown must produce a host-level diff"
    );
    assert_markdown_preview(input).expect("known-dirty markdown invariants hold");
}

#[test]
fn markdown_diff_skips_a_bare_cr_host() {
    // `pulldown-cmark` doesn't honour CommonMark's bare-`\r` line ending, so it can't
    // locate fences in a markdown host that uses bare `\r`; `--diff` skips the whole
    // file with a notice instead of silently extracting nothing.
    let input = "```yaml\ritems: [a ,b]\r```\r";
    let cfg = &safe_fix_configs()[0].cfg;
    assert!(
        fix_markdown_str(input, synthetic_path(), cfg, synthetic_base_dir()).is_none(),
        "a bare-CR markdown host must not be fixed"
    );
    let outcome = diff_outcome(
        input,
        cfg,
        synthetic_path(),
        synthetic_base_dir(),
        SourceKind::Markdown,
    );
    assert!(
        outcome.diff.is_none() && !outcome.skipped.is_empty(),
        "a bare-CR markdown host must be skipped, not diffed"
    );
}
