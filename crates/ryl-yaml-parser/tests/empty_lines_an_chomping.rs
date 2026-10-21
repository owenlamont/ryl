use granit_parser::{Event, Parser};

#[allow(clippy::unreachable)]
#[test]
fn yaml_xv9v_empty_lines_and_chomping() {
    let yaml = r#"
    Folding: "Empty line
             as a line feed"
    Chomping: |
        Clipped empty lines

"#;

    let parser = Parser::new_from_str(yaml);

    for next in parser {
        match next {
            Ok((Event::DocumentEnd, _)) => {
                break; // fine
            }
            Err(err) => {
                unreachable!("{} reported for valid YAML", err);
            }
            Ok((Event::Scalar(cow, style, size, _tag), span)) => {
                println!("{cow:?}|{style:?}|{size:?}|{span:?}");
            }
            _ => {}
        }
    }
}
