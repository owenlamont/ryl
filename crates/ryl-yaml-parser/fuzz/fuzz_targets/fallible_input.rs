#![cfg_attr(not(test), no_main)]

use std::str;

use granit_parser::{
    ErrorKind, Event, FallibleBufferedInput, InputIoError, Parser, Scanner, TokenType,
};
#[cfg(not(test))]
use libfuzzer_sys::fuzz_target;

struct ErrorAt<'a> {
    chars: str::Chars<'a>,
    remaining: usize,
    error: Option<ErrorKind>,
}

impl Iterator for ErrorAt<'_> {
    type Item = Result<char, ErrorKind>;

    fn next(&mut self) -> Option<Self::Item> {
        assert!(
            self.error.is_some(),
            "fallible source was polled after its terminal error"
        );
        if self.remaining == 0 {
            return Some(Err(self.error.take().expect("error checked above")));
        }

        self.remaining -= 1;
        Some(Ok(self
            .chars
            .next()
            .expect("injection position must not exceed the input")))
    }
}

pub fn check_input(data: &[u8]) {
    let [kind, position_hi, position_lo, payload @ ..] = data else {
        return;
    };
    if payload.len() > 64 * 1024 {
        return;
    }
    let Ok(payload) = str::from_utf8(payload) else {
        return;
    };

    // Quoting keeps the document syntactically valid, ensuring the parser and scanner reach the
    // injected source failure instead of terminating at an unrelated syntax error.
    let yaml = quoted_document(payload);
    let requested = u16::from_be_bytes([*position_hi, *position_lo]) as usize;
    let position = requested % (yaml.chars().count() + 1);
    let error = injected_error(*kind);

    check_parser(&yaml, position, &error);
    check_scanner(&yaml, position, &error);
}

#[cfg(not(test))]
fuzz_target!(|data: &[u8]| check_input(data));

fn check_parser(input: &str, position: usize, expected: &ErrorKind) {
    let source = ErrorAt {
        chars: input.chars(),
        remaining: position,
        error: Some(expected.clone()),
    };
    let mut parser = Parser::new_from_fallible_iter(source);
    let mut emitted_stream_end = false;

    let actual = loop {
        match parser.next() {
            Some(Ok((event, _))) => emitted_stream_end |= matches!(event, Event::StreamEnd),
            Some(Err(error)) => break error,
            None => panic!("source error was mistaken for clean EOF"),
        }
    };

    assert_eq!(actual.kind(), expected);
    assert!(!emitted_stream_end, "StreamEnd preceded a source error");
    assert!(parser.next().is_none() && parser.next().is_none());
}

fn check_scanner(input: &str, position: usize, expected: &ErrorKind) {
    let source = ErrorAt {
        chars: input.chars(),
        remaining: position,
        error: Some(expected.clone()),
    };
    let mut scanner = Scanner::new(FallibleBufferedInput::new(source));
    let mut emitted_stream_end = false;

    let actual = loop {
        match scanner.next() {
            Some(Ok(token)) => {
                emitted_stream_end |= matches!(token.token_type(), TokenType::StreamEnd);
            }
            Some(Err(error)) => break error,
            None => panic!("source error was mistaken for clean EOF"),
        }
    };

    assert_eq!(actual.kind(), expected);
    assert!(!emitted_stream_end, "StreamEnd preceded a source error");
    assert!(scanner.next().is_none() && scanner.next().is_none());
}

fn injected_error(selector: u8) -> ErrorKind {
    match selector % 3 {
        0 => ErrorKind::InputByteLimitExceeded {
            limit: selector as usize,
        },
        1 => ErrorKind::InputDecoding {
            message: "fuzz-injected decoding error".to_owned(),
        },
        _ => ErrorKind::InputIo {
            error: InputIoError::from_message("fuzz-injected I/O error"),
        },
    }
}

fn quoted_document(payload: &str) -> String {
    let mut yaml = String::with_capacity(payload.len() + 12);
    yaml.push_str("value: \"");
    for character in payload.chars() {
        match character {
            '"' => yaml.push_str("\\\""),
            '\\' => yaml.push_str("\\\\"),
            '\n' => yaml.push_str("\\n"),
            '\r' => yaml.push_str("\\r"),
            '\t' => yaml.push_str("\\t"),
            '\u{85}' => yaml.push_str("\\N"),
            '\u{2028}' => yaml.push_str("\\L"),
            '\u{2029}' => yaml.push_str("\\P"),
            '\u{20}'..='\u{7e}'
            | '\u{a0}'..='\u{2027}'
            | '\u{202a}'..='\u{d7ff}'
            | '\u{e000}'..='\u{fffd}'
            | '\u{10000}'..='\u{10ffff}' => yaml.push(character),
            _ => yaml.push('\u{fffd}'),
        }
    }
    yaml.push_str("\"\n");
    yaml
}
