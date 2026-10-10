#[path = "property_safe_fix/ast.rs"]
#[allow(dead_code, reason = "shared generator exposes unused render helpers")]
mod ast;
#[path = "property_safe_fix/config.rs"]
#[allow(dead_code, reason = "shared with the safe-fix suite")]
mod config;
#[path = "property_format/passes.rs"]
#[allow(dead_code, reason = "shared with the standalone formatter suite")]
mod passes;
#[path = "property_format/properties.rs"]
mod properties;
#[path = "property_format/representation.rs"]
mod representation;
#[path = "property_format/settings.rs"]
mod settings;
#[path = "property_fix_convergence/stack.rs"]
#[allow(dead_code, reason = "only the decorated document is embedded")]
mod stack;
#[path = "property_safe_fix/strategy.rs"]
mod strategy;
#[path = "property_markdown_fix/wrap.rs"]
mod wrap;

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use ryl::fix::{Rewrite, fix_markdown_str};
use ryl::{MarkdownSources, extract_regions};

use config::{synthetic_base_dir, synthetic_path};
use representation::{annotations, representation, yaml_1_1_representation};

fn arb_markdown() -> impl Strategy<Value = String> {
    wrap::arb_markdown_doc().prop_flat_map(|host| {
        prop::collection::vec(
            properties::arb_document_with_properties(),
            host.sections.len() + usize::from(host.front.is_some()),
        )
        .prop_map(move |documents| {
            let mut documents = documents.into_iter();
            let front = host.front.as_ref().map(|_| {
                documents.next().expect("one document per region").document
            });
            let sections = host.sections.iter().map(|section| wrap::FencedSection {
                prose: section.prose.clone(),
                doc: documents.next().expect("one document per region").document,
                indent: section.indent,
                fence: section.fence,
                info: section.info.clone(),
            }).collect();
            let mut markdown = wrap::MarkdownDoc { front, sections, newline: host.newline }.render();
            markdown.push_str("\n# café\n\n```text\nuntouched: [1,2]\n```\n\n<!-- ordinary Markdown -->\n");
            markdown
        })
    })
}

fn verify_preserved(original: &str, formatted: &str) -> Result<(), TestCaseError> {
    let sources = MarkdownSources {
        front_matter: true,
        fenced_blocks: true,
    };
    let before = extract_regions(original, sources);
    let after = extract_regions(formatted, sources);
    prop_assert_eq!(before.len(), after.len(), "region count changed");
    let mut original_cursor = 0;
    let mut formatted_cursor = 0;
    for (before, after) in before.iter().zip(&after) {
        prop_assert_eq!(before.kind, after.kind, "region kind changed");
        prop_assert_eq!(
            &original[original_cursor..before.raw_span.start],
            &formatted[formatted_cursor..after.raw_span.start],
            "non-YAML Markdown changed"
        );
        prop_assert_eq!(
            representation(&before.content),
            representation(&after.content),
            "core values/parse changed: {:?} -> {:?}",
            before.content,
            after.content
        );
        prop_assert_eq!(
            yaml_1_1_representation(&before.content),
            yaml_1_1_representation(&after.content),
            "PyYAML-equivalent values changed: {:?} -> {:?}",
            before.content,
            after.content
        );
        prop_assert_eq!(
            annotations(&before.content),
            annotations(&after.content),
            "comments/anchors changed: {:?} -> {:?}",
            before.content,
            after.content
        );
        original_cursor = before.raw_span.end;
        formatted_cursor = after.raw_span.end;
    }
    prop_assert_eq!(
        &original[original_cursor..],
        &formatted[formatted_cursor..],
        "trailing Markdown changed"
    );
    Ok(())
}

fn format(markdown: &str, cfg: &ryl::config::YamlLintConfig) -> String {
    fix_markdown_str(
        markdown,
        synthetic_path(),
        cfg,
        synthetic_base_dir(),
        Rewrite::Format,
    )
    .unwrap_or_else(|| markdown.to_string())
}

fn check_config(
    markdown: &str,
    name: &str,
    cfg: &ryl::config::YamlLintConfig,
) -> Result<(), TestCaseError> {
    let once = format(markdown, cfg);
    verify_preserved(markdown, &once).map_err(|error| {
        TestCaseError::fail(format!("{name} on {markdown:?}: {error}"))
    })?;
    prop_assert_eq!(
        &once,
        &format(&once, cfg),
        "not idempotent under {} on {:?}",
        name,
        markdown
    );
    Ok(())
}

fn run_invariants(markdown: &str) -> Result<(), TestCaseError> {
    for pass in passes::format_passes()
        .iter()
        .filter(|pass| pass.name.starts_with("format/"))
    {
        check_config(markdown, &pass.name, &pass.cfg)?;
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/proptest-regressions/property_markdown_format.txt",
        ))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn markdown_format_preserves_regions_and_host(
        markdown in arb_markdown(),
        table in settings::arb_format_config(),
    ) {
        run_invariants(&markdown)?;
        let cfg = ryl::config::YamlLintConfig::from_toml_str(&table).expect(&table);
        check_config(&markdown, &table, &cfg)?;
    }
}

#[test]
fn dirty_regions_change_under_every_profile() {
    let input = "---\n#front\nfoo: &a [1,2]\nbar: *a\n---\n\ntext\n\n  ```yaml\n  #fence\n  foo: &b [3,4]\n  bar: *b\n  ```\n\n```text\na: [1,2]\n```\n";
    run_invariants(input).unwrap();
    for pass in passes::format_passes()
        .iter()
        .filter(|pass| pass.name.starts_with("format/"))
    {
        let output = format(input, &pass.cfg);
        let sources = MarkdownSources {
            front_matter: true,
            fenced_blocks: true,
        };
        for (before, after) in extract_regions(input, sources)
            .iter()
            .zip(extract_regions(&output, sources))
        {
            assert_ne!(
                before.content, after.content,
                "{} must format both regions",
                pass.name
            );
        }
    }
}

#[test]
fn host_oracle_rejects_value_annotation_and_prose_changes() {
    let input = "---\na: &x 1 # keep\nb: *x\n---\n\nprose\n";
    for broken in [
        input.replace(" 1", " 2"),
        input.replace("keep", "lost"),
        input.replace('x', "y"),
        input.replace("prose", "different"),
        input.replace("---\na", "---\n- a"),
    ] {
        assert!(
            verify_preserved(input, &broken).is_err(),
            "oracle accepted {broken:?}"
        );
    }
}

#[test]
fn crlf_blockquotes_tabs_and_ragged_regions_keep_the_guarantee() {
    for input in [
        "---\na: [1,2]\n---\n\n> ```yml\n> b: [3,4]\n> ```\n".replace('\n', "\r\n"),
        "- item\n\n\t```yaml\n\ta: [1,2]\n\t```\n".to_string(),
        "text\n\n   ```yaml\n   a: [1,2]\n  b: 3\n   ```\n".to_string(),
    ] {
        run_invariants(&input).unwrap();
    }
}
