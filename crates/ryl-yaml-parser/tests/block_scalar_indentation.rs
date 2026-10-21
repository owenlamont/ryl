use granit_parser::{Event, Parser};

#[allow(clippy::unreachable)]
#[test]
fn yaml_5llu_block_scalar_wrong_indent_should_fail() {
    // It is ONLY an error for any of the leading empty lines to contain MORE spaces than the first non-empty line.
    let yaml_ok = "block scalar: >\n\n  \n   \n    invalid\n";

    let parser = Parser::new_from_str(yaml_ok);

    for next in parser {
        match next {
            Ok((Event::DocumentEnd, _)) => {
                break; // fine
            }
            Err(err) => {
                unreachable!("{} reported for valid YAML", err);
            }
            _ => {}
        }
    }

    // It IS an error for any of the leading empty lines to contain MORE spaces than the first non-empty line.
    let yaml_invalid = "block scalar: >\n\n                             \n   \n    invalid\n";

    let parser = Parser::new_from_str(yaml_invalid);

    for next in parser {
        match next {
            Ok((Event::DocumentEnd, _)) => {
                unreachable!("Document end before any error, invalid YAML");
            }
            Err(_err) => {
                break; // fine
            }
            _ => {}
        }
    }
}
