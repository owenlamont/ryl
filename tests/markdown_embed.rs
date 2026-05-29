use ryl::{MarkdownSources, RegionKind, extract_regions};

const BOTH: MarkdownSources = MarkdownSources {
    front_matter: true,
    fenced_blocks: true,
};

#[test]
fn extracts_front_matter_and_fenced_block_with_offsets() {
    let doc = "---\na: 1\n---\n\n```yaml\nb: 2\n```\n";
    let regions = extract_regions(doc, BOTH);

    assert_eq!(regions.len(), 2);
    assert_eq!(regions[0].kind, RegionKind::FrontMatter);
    assert_eq!(regions[0].line_offset, 1);
    assert_eq!(regions[0].col_offset, 0);
    assert_eq!(regions[0].content, "a: 1\n");
    assert_eq!(regions[1].kind, RegionKind::FencedBlock);
    assert_eq!(regions[1].line_offset, 5);
    assert_eq!(regions[1].col_offset, 0);
    assert_eq!(regions[1].content, "b: 2\n");
}

#[test]
fn indented_block_reports_stripped_indent_as_col_offset() {
    let regions = extract_regions("-  x\n\n   ```yaml\n   k: v\n   ```\n", BOTH);

    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].kind, RegionKind::FencedBlock);
    assert_eq!(regions[0].line_offset, 3);
    assert_eq!(regions[0].col_offset, 3);
    assert_eq!(regions[0].content, "k: v\n");
}

#[test]
fn source_toggles_select_region_kinds() {
    let doc = "---\na: 1\n---\n\n```yaml\nb: 2\n```\n";

    let front_only = extract_regions(
        doc,
        MarkdownSources {
            front_matter: true,
            fenced_blocks: false,
        },
    );
    assert_eq!(front_only.len(), 1);
    assert_eq!(front_only[0].kind, RegionKind::FrontMatter);

    let fenced_only = extract_regions(
        doc,
        MarkdownSources {
            front_matter: false,
            fenced_blocks: true,
        },
    );
    assert_eq!(fenced_only.len(), 1);
    assert_eq!(fenced_only[0].kind, RegionKind::FencedBlock);
}

#[test]
fn front_matter_requires_a_closing_delimiter() {
    assert!(extract_regions("---\na: 1\n", BOTH).is_empty());
    assert!(extract_regions("not front matter", BOTH).is_empty());

    let dots = extract_regions("---\na: 1\n...\n", BOTH);
    assert_eq!(dots.len(), 1);
    assert_eq!(dots[0].content, "a: 1\n");
}

#[test]
fn empty_or_unlabelled_blocks_produce_no_regions() {
    assert!(extract_regions("```yaml\n```\n", BOTH).is_empty());
    assert!(extract_regions("```\nplain\n```\n", BOTH).is_empty());
    assert!(extract_regions("```python\nx = 1\n```\n", BOTH).is_empty());
}

#[test]
fn yaml_language_tag_variants_are_recognised() {
    for fence in ["```yaml", "```yml", "```YAML", "```{.yaml}"] {
        let doc = format!("{fence}\na: 1\n```\n");
        let regions = extract_regions(&doc, BOTH);
        assert_eq!(regions.len(), 1, "fence {fence} should be extracted");
        assert_eq!(regions[0].content, "a: 1\n");
    }
}
