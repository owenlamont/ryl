use ryl_yaml_parser::{ErrorKind, Parser, ParserStack, StrInput};

type Stack = ParserStack<'static, core::iter::Empty<char>, StrInput<'static>>;

const RELAXED: &str = "key: [\nvalue\n ]\n";
const CONFORMING: &str = "key: [\n value\n ]\n";

fn stack_with_include(strict: bool, borrowed: bool, source: &'static str) -> Stack {
    let mut stack = Stack::with_options(ryl_yaml_parser::options! { strict_indentation: strict });
    if borrowed {
        stack.set_borrowed_resolver(move |_| Ok(source));
    } else {
        stack.set_resolver(move |_| Ok(source.to_owned()));
    }
    stack
}

#[test]
fn strict_options_reach_owned_and_borrowed_includes() {
    for borrowed in [false, true] {
        let mut strict = stack_with_include(true, borrowed, RELAXED);
        strict.push_str_parser(Parser::new_from_str("parent\n"), "parent.yaml".into());
        let error = strict.push_include("child.yaml").unwrap_err();
        assert_eq!(error.kind(), &ErrorKind::InvalidIndentation);
        assert_eq!((error.marker().line(), error.marker().col()), (2, 0));
        assert_eq!(error.source_stack(), ["parent.yaml", "child.yaml"]);
        assert_eq!(strict.stack(), ["parent.yaml"]);
        // Eager include validation must leave the parent available after failure.
        assert_eq!(
            strict.collect::<Result<Vec<_>, _>>().unwrap(),
            Parser::new_from_str("parent\n")
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        );

        for (strict, source) in [(false, RELAXED), (true, CONFORMING)] {
            let mut stack = stack_with_include(strict, borrowed, source);
            stack.push_include("child.yaml").unwrap();
            assert_eq!(
                stack.collect::<Result<Vec<_>, _>>().unwrap(),
                Parser::new_from_str(source)
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap()
            );
        }
    }
}

#[test]
fn stack_keeps_caller_supplied_parser_indentation_options() {
    for parser_strict in [false, true] {
        let mut stack =
            Stack::with_options(ryl_yaml_parser::options! { strict_indentation: !parser_strict });
        stack.push_str_parser(
            Parser::new_from_str_with_options(
                RELAXED,
                ryl_yaml_parser::options! { strict_indentation: parser_strict },
            ),
            "supplied.yaml".into(),
        );
        let result = stack.by_ref().collect::<Result<Vec<_>, _>>();
        if parser_strict {
            let error = result.unwrap_err();
            assert_eq!(error.kind(), &ErrorKind::InvalidIndentation);
            assert_eq!((error.marker().line(), error.marker().col()), (2, 0));
        } else {
            assert_eq!(
                result.unwrap(),
                Parser::new_from_str(RELAXED)
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap()
            );
        }
        assert!(stack.next().is_none());
        assert!(stack.next().is_none());
    }
}
