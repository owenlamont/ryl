//! Property tests for `fix_markdown_str` — the `--fix` write-back into YAML embedded
//! in markdown. The YAML generator is reused from the safe-fix suite via `#[path]`
//! (`ast`/`strategy`/`config`) and wrapped into a markdown host (`wrap`); the
//! invariants (`verify_host_preserved`/`verify_regions`/`run_invariants`) are
//! self-consistent, with no external oracle. Deterministic siblings pin
//! known-dirty/CRLF/ragged/boundary-crossing cases so the random property cannot
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
use ryl::config::YamlLintConfig;
use ryl::fix::{apply_safe_fixes_filtered, fix_markdown_str, suppressed_rules};
use ryl::{EmbeddedRegion, MarkdownSources, extract_regions};

use config::{parse_for_compare, safe_fix_configs, synthetic_base_dir, synthetic_path};
use wrap::arb_markdown_doc;

fn regions_of(markdown: &str, cfg: &YamlLintConfig) -> Vec<EmbeddedRegion> {
    extract_regions(
        markdown,
        MarkdownSources {
            front_matter: cfg.markdown_front_matter(),
            fenced_blocks: cfg.markdown_fenced_blocks(),
        },
    )
}

fn verify_host_preserved(
    original: &str,
    fixed: &str,
    cfg: &YamlLintConfig,
) -> Result<(), TestCaseError> {
    let before = regions_of(original, cfg);
    let after = regions_of(fixed, cfg);
    prop_assert_eq!(
        before.len(),
        after.len(),
        "region count changed: {:?} -> {:?}",
        original,
        fixed
    );
    let mut original_cursor = 0;
    let mut fixed_cursor = 0;
    for (orig, fixd) in before.iter().zip(after.iter()) {
        prop_assert_eq!(orig.kind, fixd.kind, "region kind changed");
        prop_assert_eq!(
            &original[original_cursor..orig.raw_span.start],
            &fixed[fixed_cursor..fixd.raw_span.start],
            "host bytes around a region changed"
        );
        original_cursor = orig.raw_span.end;
        fixed_cursor = fixd.raw_span.end;
    }
    prop_assert_eq!(
        &original[original_cursor..],
        &fixed[fixed_cursor..],
        "trailing host bytes changed"
    );
    Ok(())
}

fn verify_regions(
    original: &str,
    fixed: &str,
    cfg: &YamlLintConfig,
) -> Result<(), TestCaseError> {
    for (orig, fixd) in regions_of(original, cfg).iter().zip(regions_of(fixed, cfg)) {
        if let Some(before) = parse_for_compare(&orig.content) {
            prop_assert!(
                parse_for_compare(&fixd.content).as_ref() == Some(&before),
                "region parsed value changed: {:?} -> {:?}",
                orig.content,
                fixd.content
            );
        }
        let safe_fixed = apply_safe_fixes_filtered(
            &orig.content,
            cfg,
            synthetic_path(),
            synthetic_base_dir(),
            suppressed_rules(),
        );
        prop_assert!(
            fixd.content == orig.content || fixd.content == safe_fixed,
            "region must be unchanged or exactly safe-fixed: orig={:?} fixed={:?} safe_fixed={:?}",
            orig.content,
            fixd.content,
            safe_fixed
        );
    }
    Ok(())
}

fn run_invariants(markdown: &str) -> Result<(), TestCaseError> {
    for prepared in safe_fix_configs() {
        let cfg = &prepared.cfg;
        let fixed =
            fix_markdown_str(markdown, synthetic_path(), cfg, synthetic_base_dir());
        let result = fixed.clone().unwrap_or_else(|| markdown.to_string());
        verify_host_preserved(markdown, &result, cfg)?;
        verify_regions(markdown, &result, cfg)?;
        if let Some(once) = fixed {
            let twice =
                fix_markdown_str(&once, synthetic_path(), cfg, synthetic_base_dir());
            prop_assert!(
                twice.is_none() || twice.as_deref() == Some(once.as_str()),
                "fix not idempotent under '{}': once={:?} twice={:?}",
                prepared.name,
                once,
                twice
            );
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/proptest-regressions/property_markdown_fix.txt",
        ))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn markdown_fix_invariants(document in arb_markdown_doc()) {
        run_invariants(&document.render())?;
    }
}

fn fixed_with_default(markdown: &str) -> Option<String> {
    let cfg = &safe_fix_configs()[0].cfg;
    fix_markdown_str(markdown, synthetic_path(), cfg, synthetic_base_dir())
}

#[test]
fn known_dirty_front_matter_and_indented_block_is_fixed() {
    let markdown = "---\nfoo: [1,2]\n---\n\ntext\n\n  ```yaml\n  bar: [3,4]\n  ```\n";
    run_invariants(markdown).expect("invariants hold for known-dirty markdown");
    assert_eq!(
        fixed_with_default(markdown).as_deref(),
        Some("---\nfoo: [1, 2]\n---\n\ntext\n\n  ```yaml\n  bar: [3, 4]\n  ```\n"),
        "front matter and indented fenced block must both be fixed"
    );
}

#[test]
fn known_dirty_crlf_block_round_trips() {
    let markdown = "# t\r\n\r\n```yaml\r\nbar: [3,4]\r\n```\r\n";
    run_invariants(markdown).expect("invariants hold for CRLF markdown");
    assert_eq!(
        fixed_with_default(markdown).as_deref(),
        Some("# t\r\n\r\n```yaml\r\nbar: [3, 4]\r\n```\r\n"),
        "CRLF must be preserved through the fix"
    );
}

#[test]
fn fence_nested_in_front_matter_does_not_corrupt() {
    let markdown =
        "---\nzzz: [9,9]\nfoo: |\n  ```yaml\n  bar: [1,2]\n  ```\n---\n\nbody\n";
    run_invariants(markdown)
        .expect("invariants hold when a fence is nested in a front-matter scalar");
}

#[test]
fn fence_crossing_front_matter_terminator_does_not_corrupt() {
    let markdown = "---\ntags: [x,y]\ndesc: |\n  ```yaml\n  inner: [1,2]\n---\nafter: [3,4]\n```\n\ntext\n";
    run_invariants(markdown)
        .expect("invariants hold when a fence crosses the front-matter terminator");
}

#[test]
fn fence_opening_on_last_front_matter_line_does_not_corrupt() {
    let markdown = "---\ndesc: |\n  ```yaml\n---\nafter: [1,2]\n```\n\ntext\n";
    run_invariants(markdown)
        .expect("invariants hold when a fence opens on the last front-matter line");
}

#[test]
fn ragged_indent_block_is_skipped() {
    let markdown = "text\n\n   ```yaml\n   a: [1,2 ]\n  b: 3\n   ```\n";
    run_invariants(markdown).expect("invariants hold for ragged markdown");
    assert!(
        fixed_with_default(markdown).is_none(),
        "ragged-indent block cannot be reproduced exactly, so it is left untouched"
    );
}
