use granit_parser::{Parser, ScanError};

const README: &str = include_str!("../README.md");

fn minimal_example_section() -> &'static str {
    README
        .split_once("## Minimal example")
        .and_then(|(_, tail)| tail.split_once("\n## ").map(|(section, _)| section))
        .expect("README must contain a '## Minimal example' section before the next heading")
}

fn extract_yaml_input(section: &str) -> &str {
    section
        .split_once("let yaml = r#\"")
        .and_then(|(_, tail)| tail.split_once("\"#;").map(|(yaml, _)| yaml))
        .expect("README minimal example must contain a raw string YAML input")
}

fn extract_expected_output(section: &str) -> &str {
    section
        .split_once("```text\n")
        .and_then(|(_, tail)| tail.split_once("\n```").map(|(text, _)| text))
        .expect("README minimal example must contain a fenced text block with expected output")
}

fn render_readme_example(yaml: &str) -> Result<String, ScanError> {
    let mut lines = Vec::new();

    for next in Parser::new_from_str(yaml) {
        let (event, span) = next?;

        if let Some(tag) = event.tag() {
            let tag_start = span
                .tag_start()
                .map(|mark| (mark.line(), mark.col(), mark.byte_offset()));

            if let Some((value, _style)) = event.scalar() {
                lines.push(format!(
                    "scalar tag: {tag} core-suffix={:?} core-str={} tag_start(line,col,byte)={tag_start:?} for {value:?}",
                    tag.core_suffix(),
                    tag.is_yaml_core_schema_tag("str")
                ));
            } else if event.is_node() {
                lines.push(format!(
                    "node tag: {tag} custom={} tag_start(line,col,byte)={tag_start:?}",
                    tag.is_custom()
                ));
            }
        }

        lines.push(format!(
            "{event:?} bytes={:?} source={:?}",
            span.byte_range(),
            span.slice(yaml)
        ));
    }

    Ok(lines.join("\n"))
}

#[test]
fn minimal_example_output_matches_readme() {
    let section = minimal_example_section();
    let yaml = extract_yaml_input(section);
    let expected = extract_expected_output(section)
        .lines()
        .filter(|line| cfg!(feature = "comments") || !line.starts_with("Comment("))
        .collect::<Vec<_>>()
        .join("\n");
    let actual =
        render_readme_example(yaml).expect("README example YAML should parse successfully");

    println!("Actual output:\n{actual}");
    println!("Expected output:\n{expected}");

    assert_eq!(actual, expected, "README example output is out of sync");
}
