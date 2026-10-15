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
use representation::{check_annotations, check_values, check_yaml_1_1_preserved};

#[test]
#[ignore = "driven by scripts/formatter_corpus_check.py over cloned real-world repos"]
fn corpus_markdown_regions() {
    let pairs = std::env::var("RYL_MARKDOWN_PAIRS").expect("RYL_MARKDOWN_PAIRS is set");
    let output =
        std::env::var("RYL_MARKDOWN_REGIONS").expect("RYL_MARKDOWN_REGIONS is set");
    let sources = MarkdownSources {
        front_matter: true,
        fenced_blocks: true,
    };
    let mut rows = Vec::new();
    for (file_index, line) in std::fs::read_to_string(pairs)
        .expect("read Markdown pairs")
        .lines()
        .enumerate()
    {
        let (before_path, after_path) = line.split_once('\t').expect("a pair of paths");
        let (Ok(before), Ok(after)) = (
            std::fs::read_to_string(before_path),
            std::fs::read_to_string(after_path),
        ) else {
            rows.push(
                serde_json::json!({"file": file_index, "error": "non-UTF-8 Markdown"}),
            );
            continue;
        };
        let before_regions = extract_regions(&before, sources);
        let after_regions = extract_regions(&after, sources);
        let mut host_equal = before_regions.len() == after_regions.len();
        let mut before_cursor = 0;
        let mut after_cursor = 0;
        for (region_index, (left, right)) in
            before_regions.iter().zip(&after_regions).enumerate()
        {
            host_equal &= left.kind == right.kind
                && before.as_bytes()[before_cursor..left.raw_span.start]
                    == after.as_bytes()[after_cursor..right.raw_span.start];
            let stem = format!("{output}.{file_index}.{region_index}");
            let before_region = format!("{stem}.before");
            let after_region = format!("{stem}.after");
            std::fs::write(&before_region, &left.content)
                .expect("write original region");
            std::fs::write(&after_region, &right.content)
                .expect("write formatted region");
            rows.push(serde_json::json!({
                "file": file_index,
                "region": region_index,
                "kind": format!("{:?}", left.kind),
                "before": before_region,
                "after": after_region,
            }));
            before_cursor = left.raw_span.end;
            after_cursor = right.raw_span.end;
        }
        host_equal &=
            before.as_bytes()[before_cursor..] == after.as_bytes()[after_cursor..];
        rows.push(serde_json::json!({
            "file": file_index,
            "host_equal": host_equal,
            "regions": before_regions.len(),
        }));
    }
    std::fs::write(
        output,
        serde_json::to_vec(&rows).expect("serialize regions"),
    )
    .expect("write regions");
}

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
        check_values(&before.content, &after.content).map_err(TestCaseError::fail)?;
        check_yaml_1_1_preserved(&before.content, &after.content)
            .map_err(TestCaseError::fail)?;
        check_annotations(&before.content, &after.content)
            .map_err(TestCaseError::fail)?;
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
        markdown in prop_oneof![arb_markdown(), arb_fence_like_scalar(), arb_blank_scalar_with_explicit_sequence_key()],
        table in settings::arb_format_config(),
    ) {
        run_invariants(&markdown)?;
        let cfg = ryl::config::YamlLintConfig::from_toml_str(&table).expect(&table);
        check_config(&markdown, &table, &cfg)?;
    }
}

fn arb_blank_scalar_with_explicit_sequence_key() -> impl Strategy<Value = String> {
    (2usize..=8, 0usize..=3, prop::sample::select(vec!['`', '~'])).prop_map(
        |(width, indent, marker)| {
            let prefix = " ".repeat(indent);
            let fence = marker.to_string().repeat(3);
            let yaml = format!(
                "a: |\n{}\n{}w\np:\n{}-\n{}? b\n{}: x\n",
                " ".repeat(width - 1),
                " ".repeat(width),
                " ".repeat(width),
                " ".repeat(width + 2),
                " ".repeat(width + 2),
            );
            let body: String = yaml
                .lines()
                .map(|line| format!("{prefix}{line}\n"))
                .collect();
            format!("{prefix}{fence}yaml\n{body}{prefix}{fence}\n")
        },
    )
}

fn arb_fence_like_scalar() -> impl Strategy<Value = String> {
    (
        prop::sample::select(vec!['`', '~']),
        3usize..129,
        0usize..4,
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(|(marker, length, indent, fold, crlf)| {
            let fence = marker.to_string().repeat(length);
            let prefix = " ".repeat(indent);
            let body = if fold {
                format!("alpha beta gamma {fence}")
            } else {
                format!("\"{fence}\"")
            };
            let input =
                format!("{prefix}{fence}yaml\n{prefix}{body}\n{prefix}{fence}\n");
            if crlf {
                input.replace('\n', "\r\n")
            } else {
                input
            }
        })
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

#[test]
fn blank_first_scalar_line_and_flush_explicit_key_stay_stable() {
    let cfg = ryl::config::YamlLintConfig::from_toml_str(
        "[format]\nindent-sequences = false\n",
    )
    .unwrap();
    let input = "x\n\n~~~yaml\na: |\n  \n   w\np:\n   -\n     ? b\n     : x\n~~~\n";
    check_config(input, "flush sequence", &cfg).unwrap();
    let formatted = format(input, &cfg);
    let region = extract_regions(
        &formatted,
        MarkdownSources {
            front_matter: true,
            fenced_blocks: true,
        },
    );
    let standalone = ryl::format::format_str(
        "a: |\n  \n   w\np:\n   -\n     ? b\n     : x\n",
        &cfg,
        synthetic_path(),
        &[],
    );
    assert_eq!(region[0].content, standalone);
}

#[test]
fn fence_like_scalar_lines_preserve_markdown_boundaries() {
    let cfg = ryl::config::YamlLintConfig::from_toml_str(
        "line-length = 16\nindent-width = 2\n[format]\nfold-long-lines = true\n",
    )
    .unwrap();
    for marker in ['`', '~'] {
        for length in [3, 4, 5, 8, 32, 128] {
            let fence = marker.to_string().repeat(length);
            for scalar_length in [length - 1, length, length + 1] {
                let value = marker.to_string().repeat(scalar_length);
                for prefix in ["", " ", "  ", "   ", "> "] {
                    check_fence_bodies(&fence, &value, prefix, &cfg);
                }
            }
        }
    }
}

fn check_fence_bodies(
    fence: &str,
    value: &str,
    prefix: &str,
    cfg: &ryl::config::YamlLintConfig,
) {
    for body in [
        format!("\"{value}\""),
        format!("alpha beta gamma {value}"),
        format!(">-\n  alpha beta gamma {value}"),
        format!("alpha beta gamma\n    {value}"),
    ] {
        let body: String = body
            .lines()
            .map(|line| format!("{prefix}{line}\n"))
            .collect();
        let input = format!(
            "```yaml\nb:   2\n```\n\n{prefix}{fence}yaml\n{body}{prefix}{fence}\n\n```yaml\na:   1\n```\n"
        );
        let output = format(&input, cfg);
        verify_preserved(&input, &output).unwrap();
        assert!(
            output.ends_with("```yaml\na: 1\n```\n"),
            "safe sibling must format"
        );
        assert!(
            output.starts_with("```yaml\nb: 2\n```\n"),
            "preceding sibling must format"
        );
        assert_eq!(output, format(&output, cfg));
    }
}
