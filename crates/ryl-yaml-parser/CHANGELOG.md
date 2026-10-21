# Changelog

## 2.0.0 (breaking changes)

- Added the default-enabled `comments` feature. Disabling it compiles out scanner comment capture,
  parser and stack comment state, and the public comment types, variants, and
  `Options::max_buffered_comment_events`, improving parsing performance by about 13%.
  YAML comments are still skipped and validated. Applications using `default-features = false`
  must enable `comments` to access
  `Comment`, `Placement`, comment token/event variants, or `Options::max_buffered_comment_events`.
  `Options::emit_comments` remains available and defaults to `true` with the feature enabled and
  `false` without it. Setting it to `true` has no effect without the feature.
- Made `Span` non-exhaustive so future releases can add source metadata. Construct spans with
  `Span::new`, `Span::empty`, or `Span::default`, use the metadata builders or field assignment,
  and include `..` when destructuring. External struct literals and struct-update syntax are
  no longer supported.
- Split the input whitespace API into the `SkipTabs::{Yes, No}` policy and `WhitespaceResult`
  output. Custom `Input` implementations must return `(usize, Result<WhitespaceResult, ErrorKind>)`
  from `skip_ws_to_eol` and `(usize, WhitespaceResult)` from `skip_ws_to_eol_blanks`.
  Replace `SkipTabs::Result(found_tabs, has_valid_yaml_ws)` with
  `WhitespaceResult::new(found_tabs, has_valid_yaml_ws)`; its result accessors are unchanged.
  Consumed-character counts, comment separation errors, and tab behavior are preserved.
- Added `Options::strict_indentation` (default `false`) to require strict YAML-compliant indentation
  for flow entries, delimiters, and scalar continuations (compliant with case 9C9N). Default behavior
  is compatible with `PyYAML` and `ruamel.yaml`.
- Accept an explicit key indicator `?` as the last character of the input (it was rejected with
  "expected whitespace", while `?` followed by a line break was accepted).
- Accept quoted scalar continuation lines indented one column past the enclosing block
  (`a: "one\n two"`). They were rejected with "invalid indentation in multiline quoted scalar".
- A block scalar ending in an indentation-only line with no line break at the end of input
  (`|\n  a\n  `) no longer gains an extra line break (`"a\n\n"`, now `"a\n"`).
- The `SequenceEnd` span of an indentless sequence (`key:\n- a`) is empty at the next token,
  instead of covering that token (`? ` of the next key).
- Accept tabs as separation after an explicit key indicator `?` in flow collections
  (`{?\ta: b}`) and before block mapping scalar keys (`?\tkey\n: value\n` and
  `? \tkey\n: value\n`), as permitted by YAML 1.2.2. Tabs used as required indentation,
  including before compact block collections (`?\t-` and `?\tkey:`), remain invalid.
- With `strict_indentation` enabled, keep explicit-key tab indentation checks active across anchor
  and tag properties, including intervening comments. A key such as `?\t&a\n\tkey\n: value\n`
  is rejected in strict mode because its content line lacks the required space indentation after
  the anchor. The default relaxed mode preserves its acceptance of this input.
- Preserve the physical end-of-input line and column for stream-end and block-end spans when
  the last line has no line break, instead of moving them to column zero of a nonexistent next line.
- Report the physical source-error position when fallible input fails during a suppressed trailing
  comment without a final line break. Comment emission no longer changes these error coordinates.
- `strict_indentation` also checks the line after an escaped line break (`\` at the end of a
  line) in a double-quoted scalar in block context (`key: "a\` then `b"` at column 0).

## 1.3.0

**API Additions**:

- Added public `Input::fetch_block_scalar_line(&mut self, out: &mut String) -> usize`.
  It appends a complete block-scalar content line, leaves CR/LF/NUL unconsumed, and returns
  the number of Unicode characters consumed. The default reads characters directly;
  `StrInput` overrides it to copy a source slice in bulk.
- Added public `Input::take_quoted_scalar_ascii_chunk(&mut self, single: bool) -> &str`.
  It optionally consumes and returns an ordinary ASCII prefix inside a quoted scalar,
  stopping before whitespace, Unicode, controls, the matching quote, or a double-quoted
  escape. The default returns an empty slice without consuming input; `StrInput` provides
  the optimized implementation.
- Both methods have defaults, so existing custom `Input` implementations need not add
  implementations. Their contracts and examples are documented on the trait.

**Performance**:

- Batch ordinary ASCII runs in quoted scalars for string-backed input, preserving borrowed
  scalar contents and the existing handling of escapes, folding, Unicode, and invalid controls.
- Read block-scalar content lines in bulk for string-backed input and append directly for
  streaming inputs, removing the temporary line buffer while preserving validation and spans.
- Track the oldest possible simple key to avoid repeated scans of nested flow levels,
  preserving lookahead limits, required-key errors, and comment ordering.

**Fixes**:

- Refine #33 (under-indented flow sequence)

## 1.2.1

- Accept under-indented entries in multiline flow collections for compatibility with PyYAML
  and ruamel.yaml, extending the existing relaxation for flow delimiters (#33). Reject
  under-indented implicit keys spanning lines even when preceded by anchors or tags.
- Accept trailing dashes before flow delimiters in plain scalar content, such as `[a -, after]`.
- Recognize separated comments after reserved directives, emit them according to comment options,
  and exclude them from directive parameter and byte limits.
- Forward trailing comments after included documents through `ParserStack` with default options,
  while continuing to reject additional documents in an included source.
- Preserve the non-specific `!` tag when `%TAG !` overrides the primary tag handle; continue
  expanding shorthand tags such as `!foo` with the overridden prefix.
- Allow tabs as separation whitespace after `:` in block and flow mappings, including JSON,
  while retaining tab indentation checks.
- Stop zero-indented root block scalars at `---` document markers, preserving the next document
  and the preceding scalar's content.
- Reject | and > when they appear as the first character of an unquoted scalar inside a flow collection. 
- Do not reject every | or > encountered by scan_plain_scalar. These characters are legal after the 
   first character of a plain scalar ([a|b, a>b] ok, [|, >]) not. This is consistent with serde-yaml, also seems
   correct by YAML specs.
 
## 1.2.0

**Security**:

- Bounded scanner memory retained for directives with configurable directive-byte and
  reserved-parameter limits.
- Added a configurable block-collection nesting limit.

## 1.1.0

**API Additions**:

- Added the non-exhaustive `Options` type and `options!` macro for configuring buffered-comment,
  simple-key-lookahead, and flow-nesting limits through `Parser::with_options` and
  `Scanner::with_options`. Existing constructors use `Options::default()`, whose buffered-comment
  limit is 96 events.
- Added `Options::emit_comments`. Disabling it makes scanners recognize and validate comments
  without capturing their text or emitting comment tokens/events, bypassing comment-event
  buffering. `ParserStack::with_options` propagates this behavior to included sources and filters
  replayed comment events.
- Added `Parser::new_from_str_with_options`, `Parser::new_from_iter_with_options`, and
  `Parser::new_from_fallible_iter_with_options` for configuring common input sources without
  constructing input adapters directly.
- Fuzz testing hardening.

**Fixes**:

- Reduced the cost of validating large plain and block scalar contents by using an ASCII fast path
  while retaining the scanner-level validation backstop required by custom `Input`
  implementations.

## v1.0.0

**Breaking Changes**:

- Made `Scanner` a fallible, fused iterator with
  `Item = Result<Token<'input>, ScanError>`. Scanner failures are now emitted once and cannot be
  mistaken for clean EOF. Use `scanner.next()` for incremental scanning or
  `scanner.collect::<Result<Vec<_>, _>>()` to collect tokens. Removed the public `get_error`,
  `next_token`, `fetch_next_token`, and `fetch_more_tokens` methods; queue management is now an
  implementation detail.
- Removed the internal-only `Event::Nothing` variant.
- Marked `Event`, `TokenType`, and `Placement` as non-exhaustive. Downstream matches must include a
  wildcard arm so new variants can be added in compatible releases.
- Removed the redundant UTF-8-only `TEncoding` payload. `TokenType::StreamStart` is now a unit
  variant; the scanner consumes decoded Rust `char` values regardless of their original encoding.
- Removed the duplicated `Span` from scanner `Comment`. The companion `Token` remains the sole
  source span, and `Comment::new` now accepts only the comment text.
- Made `Comment` and `Token` fields private. Use `Comment::text`, `Comment::placement`, and
  `Comment::into_text`, or `Token::span`, `Token::token_type`, and `Token::into_parts` to inspect
  them; use their `new` constructors to create values.
- Made `Tag` fields private. Use `Tag::handle`, `Tag::suffix`, `Tag::original_handle`, `Tag::parts`,
  and `Tag::original_parts` to inspect them, and use the existing constructors to create tags.
- Changed `ScanError::kind` to return `&ErrorKind` instead of cloning the error category.
- Consolidated the old `ScanError::new` and convenience `ScanError::new_str` constructors into
  `ScanError::new(marker, impl Into<String>)` and removed `new_str`.
- `ScanError` now stores a structured `ErrorKind`, and `ScanError::info` renders and returns an owned
  `String` instead of borrowing a stored message.
- With `std` enabled, retaining arbitrary `std::io::Error` values means `InputIoError` and public
  types containing it no longer guarantee the `UnwindSafe` or `RefUnwindSafe` auto traits.
- Renamed `Parser::get_anchor_offset` and `ReplayParser::get_anchor_offset` to `anchor_offset`.
- Removed the duplicate `ParserStack::resolve` method; use `ParserStack::push_include`. Also removed
  the internal `AnyParser` enum from the public API.
- Made the `parser_stack` module private. Import `ParserStack` and `ReplayParser` directly from the
  crate root.

**API Additions**:

- Re-exported `ParseResult` at the crate root.
- Re-exported `ParserStack` and `ReplayParser` at the crate root.
- Implemented `FusedIterator` for `Parser`, `Scanner`, `ReplayParser`, and `ParserStack`, and made
  `ReplayParser` directly iterable.
- Added `#[must_use]` to parser, scanner, and buffered-input constructors, scanner state accessors,
  and the value-returning `Input` predicate helpers.
- Added `#[track_caller]` to caller-precondition checks so panic locations point to the invalid
  call.

**Features and Fixes**:

- Added `FallibleBufferedInput` and `Parser::new_from_fallible_iter` for
  `Iterator<Item = Result<char, ErrorKind>>` sources. Clean iterator exhaustion remains EOF, while
  an error item now terminates parsing as a `ScanError` without polling the source again. Existing
  `BufferedInput` and `Parser::new_from_iter` APIs remain unchanged.
- Added `ErrorKind::InputIo`, `ErrorKind::InputDecoding`, and
  `ErrorKind::InputByteLimitExceeded` for machine-readable streaming source failures. The provided
  `examples/fallible_reader.rs` defines a documented `CheckedChars` iterator adapter demonstrating
  per-character I/O handling, UTF-8 byte limits, clean EOF, and terminal source errors.
- Added the optional `std` feature and the stable `InputIoError` wrapper. Message-only construction
  remains available under `no_std`; with `std`, construction from `std::io::Error` retains the
  original error through `InputIoError::io_error()` and the standard error source chain.
- Made `debug_prints` enable the new `std` feature explicitly.
- Added the default `Input::take_source_error` hook so existing custom `Input` implementations stay
  source-compatible while fallible inputs can distinguish source failure from EOF.
- Fixed literal NUL being treated as end-of-input and rejected raw DEL and C0/C1 controls other
  than TAB, LF, CR, and NEL in scalars and comments. The valid double-quoted `"\0"` escape remains
  supported.
- Added the public, non-exhaustive `ErrorKind` enum and `ScanError::kind()` for matching errors
  without relying on human-readable text. Variants carry relevant structured details where needed,
  such as the opening and closing characters for mismatched flow collections. `ScanError` now
  stores its marker and `ErrorKind`, while descriptions are rendered by `ErrorKind`'s `Display`
  implementation.
- `ScanError::new(marker, message)` constructs externally supplied errors for parser adapters and
  `ParserStack` include resolvers, copying the message into `ErrorKind::Custom(String)`.
- `ParserStack` errors now retain their nested source names in `ScanError::source_stack()` and
  include that context in human-readable error messages.
- Added the default `error_messages` feature. Disabling default features removes human-readable
  error text: `ErrorKind`'s `Display` implementation and `ScanError::info()` return an empty string,
  while structured error kinds and source markers remain available.


## v0.0.7
- Added `YamlVersion` and changed `Event::DocumentStart` to
  `DocumentStart(bool, Option<YamlVersion>)`, emitting the `%YAML` directive
  version for each document start when present.
- Added `Tag::suffix_in_namespace`, which returns the type name a tag resolves to within
  an arbitrary namespace prefix (matching the resolved `handle ++ suffix` URI across every
  spelling, including `%TAG` mid-name splits). This generalizes `Tag::core_suffix`, which
  now delegates to it.
- Add Tag::suffix_in_namespace: resolved type name within an arbitrary namespace
- Summarizing results of automated fuzzing and static analysis, robustness against malformed (bad intent) 
  YAML input was further hardened.

## v0.0.6
- Added `Tag::core_suffix` and made core-tag helpers match the resolved YAML 1.2.2
  Core Schema tags (`null`, `bool`, `int`, `float`, `map`, `seq`, and `str`)
  instead of checking only the resolved handle.
- Added `Span::indent` hints for non-empty block scalar content while leaving
  whitespace-only block scalar spans unannotated.

## v0.0.5
- Performance improvements on comment parsing
- Added `Span::tag_start` metadata for parser-emitted tagged node events, so diagnostics can point
  at the explicit tag token even when the node span starts on a later line.
- Added `Tag::original_handle`, `Tag::original_parts`, and `Tag::original` so consumers can inspect
  the tag handle as written before `%TAG` directive resolution and reconstruct normalized
  author-facing tag spelling, including verbatim tags (breaking change).

## v0.0.4
  Retracted

## v0.0.3

- Added `Span::slice(&str)` for convenient source extraction from parser-emitted
  spans when byte offsets are available.
- Added small `Tag` inspection helpers (`is_yaml_core_schema_tag`, `is_custom`,
  `parts`) for YAML-core-namespace and non-core-namespace tag consumers.
- Added `Event` inspection helpers (`anchor_id`, `alias_id`, `tag`, `scalar`,
  `is_node`) for ergonomic access to per-event node metadata. `anchor_id`
  returns `Option<usize>` for events that define an anchor; `alias_id` returns
  `Option<usize>` for aliases that reference an anchor; `is_node` returns `true`
  for any event that produces a value in the document tree.
- Added comment events. Comments are emitted as parser
  `Event::Comment(text, placement)` events with normal companion spans, and as
  scanner comment tokens carrying `Placement` metadata. This is an event-stream
  change: consumers building YAML data trees should ignore comment events, while
  formatting/linting tools can use the placement hint and comment span.
- Added `StructureStyle` metadata for sequences and mappings, exposed on
  `Event::SequenceStart(style, anchor_id, tag)` and
  `Event::MappingStart(style, anchor_id, tag)` events.
- Added `ParserStack::push_include` as an include-oriented alias for
  `ParserStack::resolve`.
- Added a custom-tag example showing how to inspect application tags such as
  `!degrees` from the event stream.
- Added an include-stack example demonstrating `!include`-style resolution
  through `ParserStack`.
- Updated the README minimal example to show `Span::slice` alongside byte
  ranges.
- Fixed the span of quoted (single- and double-quoted) scalars to end at the
  closing quote rather than after trailing whitespace and an optional
  end-of-line comment. This makes `Span::slice` return only the scalar source.
- Fixed pedantic Clippy warnings and switched into pedantic mode.

## v0.0.2

**Features**:

- Added `TryEventReceiver`, `TrySpannedEventReceiver`, `TryLoadError`,
  `Parser::try_load`, and `ParserTrait::try_load` for receiver-style parsing
  that can fail fast with an application error. The existing `load` API remains
  unchanged for infallible receivers.

## v0.0.1

Initial `granit-parser` release after splitting the parser into its own
standalone package.

**Breaking Changes**:

- Renamed the package to `granit-parser` and the Rust crate path to
  `granit_parser`.
- Raised MSRV to `1.81.0`.
- Made the library `no_std` by default, with `alloc` enabled internally.
- Changed parser events, scanner tokens, receivers, and parse-result aliases to
  carry an input lifetime.
- Changed scalar, tag, anchor, alias, and tag-directive payloads to use
  `Cow<'input, ...>` so `&str` input can avoid allocations.
- Renamed `TScalarStyle` to `ScalarStyle`.
- Changed `Marker::index()` and error messages to report character positions.
  Byte positions are now optional and available through `Marker::byte_offset()`
  and `Span::byte_range()`.
- Added `Span::indent` for parser-emitted indentation hints.
- `debug_prints` no longer reads environment variables. Debug output is now
  controlled by the feature and a local compile-time toggle in `src/debug.rs`.

**Features**:

- Added `BorrowedInput` and byte-slicing support for inputs that can safely
  borrow from stable source storage.
- Added zero-copy scanning for `StrInput` across plain scalars, quoted scalars,
  anchors, aliases, tags, and tag directives when no decoding is needed.
- Added `Parser::new_from_iter` as the iterator-backed constructor.
- Added `ParserTrait`, `ParserStack`, and `ReplayParser` for stacked parser
  workflows and replayed event streams.
- Added anchor offset management for stacked/replayed parser use cases.
- Added `Tag::is_yaml_core_schema()` and `Display` for `Tag`.
- Exported the input module and scanner/token types needed by downstream tools.

**Fixes**:

- Reject unclosed and misplaced flow brackets with diagnostics pointing at the
  relevant opening or closing marker.
- Reject unclosed quoted scalars with an `unclosed quote` diagnostic.
- Reject comment-interrupted multiline plain scalars instead of silently
  truncating the scalar.
- Handle reserved directives according to YAML rules while still rejecting
  malformed `%YAML` directives.
- Handle JSON-style surrogate pairs in quoted scalars and reject unpaired,
  reversed, or invalid surrogate escapes.
- Fix several flow mapping and flow sequence edge cases, including empty
  implicit keys and nested implicit mappings.
- Fix plain-scalar handling around `---`, `...`, `:`, `-`, tabs, and flow
  indicators.
- Fix block scalar indentation, chomping, kept trailing lines, tabs in scalar
  bodies, and non-ASCII span accounting.
- Fix document marker spans so `---` and `...` are emitted at the marker that
  triggered them.
- Fix spans for empty scalars, null values, mapping keys, non-ASCII input,
  comments, and block scalars.
- Avoid parser hangs and panic paths found by regression and fuzz tests.

**Performance**:

- Added fast ASCII paths for `StrInput`.
- Added chunked plain-scalar scanning and faster comment skipping.
- Switched several small parser/scanner stacks from `Vec` to `smallvec`.

**Build and Package**:

- Made the root crate standalone instead of relying on workspace-inherited
  package metadata.
- Removed unused root dependencies and dev-dependencies, including the old
  high-level YAML loader, `hashlink`, `miette`, `quickcheck`, `rustyline`, and
  `thiserror`.
- Added `smallvec` and disabled default features for `arraydeque`.
- Forbid unsafe code through crate attributes and Cargo lints.
- Renamed root tool binaries to `time_parser` and `run_parser`.

**Tests and CI**:

- Updated tests to use `granit_parser` and the lifetime/Cow-based event API.
- Run parser regression tests against both `StrInput` and `BufferedInput`.
- Replaced the YAML test-suite loader with a local fixture reader so the root
  crate no longer needs a high-level YAML parser as a dev-dependency.
- Added regression coverage for flow brackets, comments, document markers,
  block scalars, indentation hints, tabs, plain scalar indicators, surrogate
  escapes, non-ASCII positions, parser stacks, and fuzz-discovered inputs.
- Added a README example regression test.
- Added CI coverage for clippy, formatting, host library tests without default
  features, and `wasm32v1-none` `no_std` library checks.

**Documentation and Tools**:

- Rewrote crate docs and README examples for `granit-parser`.
- Added README coverage for repository tools.
- Added the standalone `tools/walk` helper crate for navigating parsed YAML
  spans.

## v0.0.6

**Fixes**:
- Fix emitting of tags with empty handles. `!tag` no longer emits as `!!tag`.

## v0.0.5

**Breaking Changes**:

- Emit `Cow<'input, Tag>` instead of `Tag` to avoid copies.

**Fixes**:

- 8ef76dcc: Fix `Marker`s for `null` and empty values.
- Fix `Span`s for collections to correctly mark the end of the collection.

**Changes**

- Exclude `yaml-test-suite` from the Cargo package.
- Bump `libtest-mimic` to `0.8.1`.

## v0.0.4

**Breaking Changes**:

- Allow events to borrow from the input.
- Rename `TScalarStyle` to `ScalarStyle`.

## v0.0.3

**Breaking Changes**:

- 926fdfb: Events now use spans rather than markers, allowing for tracking both
  the beginning and the end of scalars.
- 6c57b5b: Add a boolean to `DocumentStart` to know whether the start was
  explicit (`---`) or implicit.

**Features**:

- Add an `Input` interface to prepare the ground to future input-specific.
  optimizations (such as returning `Cow`'d strings when possible). This also
  potentially allows for user-defined optimizations.
- Add `Parser::new_from_iter` to load from an iterator. This automatically
  wraps using `BufferedInput`, which implements the new `Input` trait the
  `Parser` needs.

**Fixes**:

- 750c992: Add support for nested implicit flow mappings.
- 11cffc6: Fix error with deeply indented block scalars.
- d3b9641: Fix assertion that could erroneously trigger with multibyte
  characters.
- 95fe3fe: Fix parse errors when `---` appeared in the middle of plain scalars.
- 3358629: Fix infinite loop with `...` in plain scalars in flow contexts.
- Fix panics on other various erroneous inputs found while fuzzing.

**Internal changes**:

- Run all tests with both `Input` backends
- #15: Add fuzzing

## v0.0.2

This release does not provide much but is needed for the `saphyr` library to
depend on the new features.

**Breaking Changes**:

**Features**:
- Add `Marker::default()`
- Rework string handling in `ScanError`

**Fixes**:
- [yaml-rust2 #21](https://github.com/Ethiraric/yaml-rust2/issues/21#issuecomment-2053513507)
  Fix parser failing when a comment immediately follows a tag.

**Internal changes**:
- Various readability improvements and code cleanups
