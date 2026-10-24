// Copyright 2015, Yuheng Chen.
// Copyright 2023, Ethiraric.
// See the LICENSE file at the top-level directory of this distribution.

//! YAML 1.2 parser implementation in pure Rust.
//!
//! `ryl-yaml-parser` is a low-level event parser. It reads YAML input and yields a stream of
//! [`Event`] values paired with their source [`Span`].
//! Comments are emitted as `Event::Comment`. They
//! are presentation metadata, not YAML data nodes, so consumers building YAML value trees should
//! ignore them.
//!
//! Add it to your project:
//!
//! ```sh
//! cargo add ryl-yaml-parser
//! ```
//!
//! # Usage
//!
//! ```rust
//! # fn main() -> Result<(), ryl_yaml_parser::ScanError> {
//! use ryl_yaml_parser::{Event, Parser, Placement};
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
//! defaults to `false` for compatibility with `PyYAML` and ruamel.yaml, accepting under-indented
//! flow entries and delimiters; with it off, [`Parser::strict_indentation_error`] still reports
//! the error strict indentation would have stopped at. [`Options`] also controls comment
//! emission and limits on buffered comments, simple-key lookahead, directive retention, and
//! flow- and block-collection nesting.
//! Comment tokens and events are emitted by default; setting `Options::emit_comments` to `false`
//! recognizes and validates comments without capturing their text or emitting them. The defaults
//! allow 96 buffered comment events, 1024 characters of simple-key lookahead, 1024 bytes of
//! retained directive data, 16 reserved-directive parameters, 255 nested flow collections, and 255
//! nested block collections. Existing constructors use these defaults.
//! [`Parser::new_from_str_with_options`], [`Parser::new_from_iter_with_options`],
//! [`Parser::new_from_fallible_iter_with_options`], [`Parser::with_options`],
//! [`Scanner::with_options`], and [`ParserStack::with_options`] accept customized options created
//! with [`options!`].

#![forbid(unsafe_code)]
#![deny(
    clippy::all,
    clippy::pedantic,
    clippy::cargo,
    clippy::cognitive_complexity
)]
#![warn(missing_docs)]

mod char_traits;
mod error;
pub mod input;
mod macros;
mod observer;
mod options;
pub use observer::{TokenKind, TokenObserver};
mod parser;
mod parser_stack;
mod scanner;

pub use crate::error::{ErrorKind, InputIoError, ScanError};
pub use crate::input::{
    BorrowedInput, BufferedInput, FallibleBufferedInput, Input, str::StrInput,
};
pub use crate::options::Options;
pub use crate::parser::{
    Event, EventReceiver, ParseResult, Parser, ParserTrait, SpannedEventReceiver,
    StructureStyle, Tag, TryEventReceiver, TryLoadError, TrySpannedEventReceiver,
    YamlVersion,
};
pub use crate::parser_stack::{ParserStack, ReplayParser};
pub use crate::scanner::{Comment, Placement};
pub use crate::scanner::{Marker, ScalarStyle, Scanner, Span, Token, TokenType};

// Keep every Rust example in the package README covered by `cargo test --doc` without duplicating
// the README in the rendered crate-level documentation.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
mod readme_doctests {}
