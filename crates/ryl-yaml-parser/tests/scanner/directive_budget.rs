use ryl_yaml_parser::{ErrorKind, Event, Parser, ScanError, Span};

/// Directives whose handles, prefixes, escapes and parameters each draw on the byte budget.
const DIRECTIVES: &[&str] = &[
    "%TAG !e! tag:a,2000:%41b!c\n--- !e!x v\n",
    "%TAG ! !lo%41cal\n--- !x v\n",
    "%TAG !! tag:x\n---\nv\n",
    "%FOO a\tbb\n---\nv\n",
];

fn outcome(
    events: impl Iterator<Item = Result<(Event<'static>, Span), ScanError>>,
) -> Result<usize, ErrorKind> {
    let mut count = 0;
    for event in events {
        event.map_err(|error| error.kind().clone())?;
        count += 1;
    }
    Ok(count)
}

/// Parse `yaml` from both borrowed and streaming input under `limit`, requiring them to agree.
fn parse_within(yaml: &'static str, limit: usize) -> Result<usize, ErrorKind> {
    let options = ryl_yaml_parser::options! { max_directive_bytes: limit };
    let borrowed = outcome(Parser::new_from_str_with_options(yaml, options.clone()));
    let streamed = outcome(Parser::new_from_iter_with_options(yaml.chars(), options));
    assert_eq!(borrowed, streamed, "{yaml:?} at limit {limit}");
    borrowed
}

#[test]
fn every_directive_byte_draws_on_the_budget() {
    for yaml in DIRECTIVES {
        let line = yaml.split_once('\n').unwrap().0;
        let needed = (0..=line.len())
            .find(|&limit| parse_within(yaml, limit).is_ok())
            .unwrap_or_else(|| panic!("{yaml:?} never fits"));
        assert!(needed > 0, "{yaml:?}");
        for limit in 0..needed {
            assert!(
                matches!(
                    parse_within(yaml, limit),
                    Err(ErrorKind::DirectiveByteLimitExceeded { .. })
                ),
                "{yaml:?} at limit {limit}"
            );
        }
    }
}
