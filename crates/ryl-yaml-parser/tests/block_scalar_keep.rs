// 6FWR: Block Scalar Keep (|+)
use granit_parser::{Event, Parser};

#[allow(clippy::unreachable)]
#[test]
fn case_6fwr_keep_space() {
    // Suite expectation: "ab\n\n \n" \u{2014} the final kept line contains a single space.
    let yaml = "--- |+\n ab\n\n \n...\n";
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
                // "ab\n\n\n"|Literal|0|Span { start: Marker { index: 8, line: 2, col: 1 }, end: Marker { index: 14, line: 5, col: 0 } }
                println!("{cow:?}|{style:?}|{size:?}|{span:?}");
            }
            Ok((_event, _)) => {}
        }
    }
}
