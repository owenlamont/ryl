use std::io;

use granit_parser::{ErrorKind, Event, InputIoError, Parser, ScanError};

/// Iterator adapter that adds parser error reporting and a UTF-8 byte limit.
///
/// A byte reader must decode its input before constructing this adapter because the YAML parser
/// consumes Rust `char` values, not raw bytes. The decoder expresses each read as
/// `io::Result<char>`: `Ok(c)` is a decoded character, `Err(error)` is a read failure, and the
/// iterator's `None` is clean EOF.
struct CheckedChars<I> {
    /// Decoded character source wrapped by this adapter.
    input: I,
    /// Number of UTF-8 bytes represented by characters returned so far.
    bytes_read: usize,
    /// Maximum number of UTF-8 bytes accepted from the source.
    byte_limit: usize,
    /// Prevents the source from being polled again after EOF or an error.
    finished: bool,
}

impl<I> CheckedChars<I> {
    fn new(input: I, byte_limit: usize) -> Self {
        Self {
            input,
            bytes_read: 0,
            byte_limit,
            finished: false,
        }
    }
}

/// Implementing `Iterator` makes `CheckedChars` a lazy character source for the parser.
/// `next()` is called once whenever the parser needs another character.
impl<I> Iterator for CheckedChars<I>
where
    I: Iterator<Item = io::Result<char>>,
{
    type Item = Result<char, ErrorKind>;

    fn next(&mut self) -> Option<Self::Item> {
        // Do not poll the source after EOF or after reporting the first error.
        if self.finished {
            return None;
        }

        let Some(next) = self.input.next() else {
            // `None` from the source means clean EOF. Returning `None` forwards that EOF to the
            // parser without manufacturing an error.
            self.finished = true;
            return None;
        };

        match next {
            Ok(c) => {
                let char_bytes = c.len_utf8();

                if char_bytes > self.byte_limit.saturating_sub(self.bytes_read) {
                    self.finished = true;
                    Some(Err(ErrorKind::InputByteLimitExceeded {
                        limit: self.byte_limit,
                    }))
                } else {
                    self.bytes_read += char_bytes;
                    Some(Ok(c))
                }
            }
            Err(error) => {
                // Report the reader failure as an input error. Setting `finished` guarantees that
                // neither this adapter nor the parser will read beyond the failure.
                self.finished = true;
                Some(Err(ErrorKind::InputIo {
                    // With the crate's `std` feature, this retains the original `io::Error`
                    // alongside its portable message instead of discarding it after formatting.
                    error: InputIoError::from(error),
                }))
            }
        }
    }
}

fn main() -> Result<(), ScanError> {
    let yaml = "service:\n  enabled: true\n  retries: 3\n";

    // This in-memory source cannot fail, so wrap each character in `Ok`. A real reader-backed UTF-8
    // decoder would have the same `Iterator<Item = io::Result<char>>` interface and return its I/O
    // failures as `Err` items.
    let decoded = yaml.chars().map(Ok::<_, io::Error>);

    // Construct the lazy adapter. No input is read until the parser starts requesting characters.
    let input = CheckedChars::new(decoded, 1024);

    // `new_from_fallible_iter` distinguishes the adapter's clean EOF (`None`) from its source
    // failures (`Some(Err(...))`). The `?` below returns either source or YAML syntax failures.
    for next in Parser::new_from_fallible_iter(input) {
        let (event, _) = next?;
        if let Event::Scalar(value, ..) = event {
            println!("{value}");
        }
    }

    Ok(())
}
