// Copyright 2015, Yuheng Chen.
// Copyright 2023, Ethiraric.
// See the LICENSE file at the top-level directory of this distribution.

//! YAML 1.2 parser implementation in pure Rust.
//!
//! `granit-parser` is a low-level event parser. It reads YAML input and yields a stream of
//! [`Event`] values paired with their source [`Span`].
//! With the default `comments` feature, comments are emitted as `Event::Comment`. They
//! are presentation metadata, not YAML data nodes, so consumers building YAML value trees should
//! ignore them.
//!
//! Add it to your project:
//!
//! ```sh
//! cargo add granit-parser
//! ```
//!
//! # Usage
//!
//! ```rust
//! # fn main() -> Result<(), granit_parser::ScanError> {
//! # #[cfg(feature = "comments")]
//! # {
//! use granit_parser::{Event, Parser, Placement};
//!
//! let yaml = r#"# header
//! items: # inline
//!   - milk
//!   - bread
//! "#;
//! let mut comments = Vec::new();
//!
//! for next in Parser::new_from_str(yaml) {
//!     let (event, span) = next?;
//!     if let Event::Comment(text, placement) = event {
//!         comments.push((
//!             text.into_owned(),
//!             placement,
//!             span.slice(yaml).unwrap().to_owned(),
//!         ));
//!     }
//! }
//!
//! assert_eq!(
//!     comments,
//!     [
//!         (" header".to_owned(), Placement::Above, "# header".to_owned()),
//!         (" inline".to_owned(), Placement::Right, "# inline".to_owned()),
//!     ]
//! );
//! # }
//! # Ok(())
//! # }
//! ```
//!
//! For comment events, the companion [`Span`] covers the whole source comment, including `#` and
//! excluding the line break. With [`Parser::new_from_str`], [`Span::slice`] returns that source
//! comment text.
//!
//! # Limits
//!
//! [`Options::strict_indentation`] enables strict YAML indentation for flow collections. It
//! defaults to `false` for compatibility with `PyYAML` and ruamel.yaml, accepting under-indented flow entries
//! and delimiters. [`Options`] also controls comment emission and limits on buffered comments,
//! simple-key lookahead, directive retention, and flow- and block-collection nesting. With the
//! `comments` feature, comment tokens and events are emitted by default; setting
//! `Options::emit_comments` to `false`
//! recognizes and validates comments without capturing their text or emitting them. The defaults
//! allow 96 buffered comment events, 1024 characters of simple-key lookahead, 1024 bytes of retained
//! directive data, 16 reserved-directive parameters, 255 nested flow collections, and 255 nested
//! block collections.
//! Existing constructors use these defaults.
//! Without the `comments` feature, comment capture and emission are compiled out of scanners,
//! parsers, and parser stacks. YAML comments are still skipped and validated.
//! [`Options::emit_comments`] remains available and defaults to `true` with the feature enabled
//! and `false` without it. Setting it to `true` has no effect without the feature.
//! [`Parser::new_from_str_with_options`], [`Parser::new_from_iter_with_options`],
//! [`Parser::new_from_fallible_iter_with_options`], [`Parser::with_options`],
//! [`Scanner::with_options`], and [`ParserStack::with_options`] accept customized options created
//! with [`options!`].
//!
//! # Features
//! **Note:** This crate's MSRV is `1.81.0`.
//!
//! #### `comments` (enabled by default)
//! Enables scanner comment tokens, parser comment events, and their capture, placement, buffering,
//! and continuation state. Without it, scanners skip comments without retaining their text, and
//! comment-specific state is compiled out. YAML comment syntax and source positions are still
//! validated and tracked.
//!
//! `Comment`, `Placement`, `TokenType::Comment`, `Event::Comment`, `ErrorKind::TooManyComments`,
//! `Options::max_buffered_comment_events`, and `Input::may_contain_comments` are available only
//! with this feature.
//!
//! #### `error_messages` (enabled by default)
//! Provides human-readable text through [`ErrorKind`]'s `Display` implementation and
//! [`ScanError::info`]. Disabling this feature makes both render an empty string while retaining
//! machine-readable error kinds and source markers.
//!
//! #### `std`
//! Retains the original `std::io::Error` inside [`InputIoError`] when constructed through
//! `InputIoError::from_io` or its `From<std::io::Error>` implementation. Without this feature,
//! [`InputIoError::from_message`] remains available for portable `no_std` error reporting.
//!
//! #### `debug_prints`
//! Enables the `debug` module and usage of debug prints in the scanner and the parser. Do not
//! enable if you are consuming the crate rather than working on it as this can significantly
//! decrease performance. Output remains opt-in behind a local compile-time toggle in
//! `src/debug.rs`.
//!
//! This feature does not raise the MSRV further.
//!
//! This feature enables `std` and is _not_ `no_std` compatible.

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::pedantic)]
#![no_std]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

mod char_traits;
#[macro_use]
mod debug;
mod error;
pub mod input;
mod macros;
mod options;
mod parser;
mod parser_stack;
mod scanner;

pub use crate::error::{ErrorKind, InputIoError, ScanError};
pub use crate::input::{str::StrInput, BorrowedInput, BufferedInput, FallibleBufferedInput, Input};
pub use crate::options::Options;
pub use crate::parser::{
    Event, EventReceiver, ParseResult, Parser, ParserTrait, SpannedEventReceiver, StructureStyle,
    Tag, TryEventReceiver, TryLoadError, TrySpannedEventReceiver, YamlVersion,
};
pub use crate::parser_stack::{ParserStack, ReplayParser};
#[cfg(feature = "comments")]
pub use crate::scanner::{Comment, Placement};
pub use crate::scanner::{Marker, ScalarStyle, Scanner, Span, Token, TokenType};

// Keep every Rust example in the package README covered by `cargo test --doc` without duplicating
// the README in the rendered crate-level documentation.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
mod readme_doctests {}

// Verify the feature-disabled public API without adding these checks to rendered documentation.
#[cfg(all(doctest, not(feature = "comments")))]
/// ```compile_fail
/// use granit_parser::Comment;
/// ```
/// ```compile_fail
/// use granit_parser::Placement;
/// ```
/// ```compile_fail
/// let _ = granit_parser::Event::Comment;
/// ```
/// ```compile_fail
/// let _ = granit_parser::TokenType::Comment;
/// ```
/// ```compile_fail
/// let _ = granit_parser::Options::default().max_buffered_comment_events;
/// ```
/// ```compile_fail
/// let _ = granit_parser::ErrorKind::TooManyComments;
/// ```
/// ```compile_fail
/// use granit_parser::{Input, StrInput};
/// let _ = <StrInput<'_> as Input>::may_contain_comments(&StrInput::new(""));
/// ```
mod comments_disabled_doctests {}
