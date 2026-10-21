# granit-parser

[![unsafe forbidden](https://img.shields.io/badge/unsafe-forbidden-success.svg)](https://github.com/rust-secure-code/safety-dance/)
[![panic-free](https://img.shields.io/badge/panic--free-%E2%9C%94%EF%B8%8F-brightgreen)](https://effective-rust.com/panic.html)
[![GitHub Workflow Status](https://img.shields.io/github/actions/workflow/status/bourumir-wyngs/granit-parser/ci.yml)](https://github.com/bourumir-wyngs/granit-parser/actions/workflows/ci.yml)
[![Fuzz](https://github.com/bourumir-wyngs/granit-parser/actions/workflows/fuzz.yml/badge.svg)](https://github.com/bourumir-wyngs/granit-parser/actions/workflows/fuzz.yml)
[![docs.rs](https://docs.rs/granit-parser/badge.svg)](https://docs.rs/granit-parser)

[![codecov](https://codecov.io/gh/bourumir-wyngs/granit-parser/graph/badge.svg)](https://codecov.io/gh/bourumir-wyngs/granit-parser)
[![Socket Badge](https://badge.socket.dev/cargo/package/granit-parser)](https://socket.dev/cargo/package/granit-parser)
[![dependency status](https://deps.rs/crate/granit-parser/latest/status.svg)](https://deps.rs/crate/granit-parser/latest)

[![1.3 API compatibility](https://github.com/bourumir-wyngs/granit-parser/actions/workflows/api-compat.yml/badge.svg)](https://github.com/bourumir-wyngs/granit-parser/actions/workflows/api-compat.yml)
[![crates.io](https://img.shields.io/crates/l/granit-parser.svg)](https://crates.io/crates/granit-parser)
[![crates.io](https://img.shields.io/crates/v/granit-parser.svg)](https://crates.io/crates/granit-parser)
[![crates.io](https://img.shields.io/crates/d/granit-parser.svg)](https://crates.io/crates/granit-parser)
[![CodSpeed](https://img.shields.io/endpoint?url=https://codspeed.io/badge.json)](https://app.codspeed.io/bourumir-wyngs/granit-parser?utm_source=badge)

> “YAML is hard. Much more than I had anticipated. If you are exploring dark corners of YAML ... I'm curious to know what it is.”
>
> — [Ethiraric](https://crates.io/users/Ethiraric)

**granit-parser** is a YAML 1.2 parser in pure Rust with comment and style support, no-std support, and spans for parser events. It accepts many real-world YAML documents from the YAML 1.1 era where the syntax overlaps with YAML 1.2. “Granit” is a correct word in many European languages (English *granite*).

This crate started as a fork of [saphyr-parser](https://crates.io/crates/saphyr-parser) that descends from [yaml-rust](https://github.com/chyh1990/yaml-rust), with influences from [libyaml](https://crates.io/crates/libyaml) and [yaml-cpp](https://github.com/jbeder/yaml-cpp). The project has since diverged significantly and is now maintained as an independent project.

Its primary goals are:

* [`Comment`](https://docs.rs/granit-parser/latest/granit_parser/struct.Comment.html) support and [`StructureStyle`](https://docs.rs/granit-parser/latest/granit_parser/enum.StructureStyle.html) information. This is for linting, formatting, and analysis.
* compliance with the [yaml-test-suite](https://github.com/yaml/yaml-test-suite), including correctness in edge cases
* compatibility with real-world YAML usage
* quickly incorporate the changes we need for the upstream dependency [serde-saphyr](https://crates.io/crates/serde-saphyr).

`granit-parser` 1.0 stabilizes the public API after the experimental 0.0.x releases. The 1.0
migration, including scanner and parser-stack replacements, is documented in
[`CHANGELOG.md`](CHANGELOG.md).

See [releases](https://github.com/bourumir-wyngs/granit-parser/releases)

## Minimal example

[`Parser::new_from_str`](https://docs.rs/granit-parser/latest/granit_parser/struct.Parser.html#method.new_from_str) returns an iterator of ([`Event`](https://docs.rs/granit-parser/latest/granit_parser/enum.Event.html), [`Span`](https://docs.rs/granit-parser/latest/granit_parser/struct.Span.html)) pairs. The event helpers expose common node metadata, and spans provide byte ranges, source slices, and explicit tag-token starts for tagged nodes:

Comments are emitted as [`Event::Comment(text, placement)`](https://docs.rs/granit-parser/latest/granit_parser/enum.Event.html#variant.Comment). They are presentation metadata for tools such as linters and formatters, not YAML data nodes, so consumers that build YAML values should filter them out. The companion [`Span`](https://docs.rs/granit-parser/latest/granit_parser/struct.Span.html) for a comment covers the whole source comment, including `#` and excluding the line break; when parsing from [`Parser::new_from_str`](https://docs.rs/granit-parser/latest/granit_parser/struct.Parser.html#method.new_from_str), [`span.slice(yaml)`](https://docs.rs/granit-parser/latest/granit_parser/struct.Span.html#method.slice) returns that source comment text.

Applications that do not use comments can disable their capture and emission by setting
[`Options::emit_comments`](https://docs.rs/granit-parser/latest/granit_parser/struct.Options.html#structfield.emit_comments)
to `false` or disabling the default `comments` Cargo feature. The field is always available and
defaults to `true` with the feature enabled and `false` without it. Setting it to `true` has no
effect without the feature. Comments are still recognized and validated as YAML syntax.

Document starts are emitted as [`Event::DocumentStart(explicit, version)`](https://docs.rs/granit-parser/latest/granit_parser/enum.Event.html#variant.DocumentStart), where `version` is the optional [`YamlVersion`](https://docs.rs/granit-parser/latest/granit_parser/struct.YamlVersion.html) declared by a preceding `%YAML` directive for that document.

```rust
use granit_parser::Parser;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let yaml = r#"
%YAML 1.2
%TAG !example! tag:example.com,2000:
%TAG !core! tag:yaml.org,2002:i
---
items: !shopping
  - milk
  - !example!sliced bread
  - !!str bread
  - !core!nt 1
locations: # Example with composite keys
  [47.3769, 8.5417]: local
  [40.7128, -74.0060]: remote

# JSON-style \uXXXX surrogate pairs:
music: "\uD834\uDD1E\uD83C\uDFB5\uD83C\uDFB6"
"#;

    for next in Parser::new_from_str(yaml) {
        let (event, span) = next?;

        if let Some(tag) = event.tag() {
            let tag_start = span
                .tag_start()
                .map(|mark| (mark.line(), mark.col(), mark.byte_offset()));

            if let Some((value, _style)) = event.scalar() {
                println!(
                    "scalar tag: {tag} core-suffix={:?} core-str={} tag_start(line,col,byte)={tag_start:?} for {value:?}",
                    tag.core_suffix(),
                    tag.is_yaml_core_schema_tag("str")
                );
            } else if event.is_node() {
                println!(
                    "node tag: {tag} custom={} tag_start(line,col,byte)={tag_start:?}",
                    tag.is_custom()
                );
            }
        }

        println!(
            "{event:?} bytes={:?} source={:?}",
            span.byte_range(),
            span.slice(yaml)
        );
    }

    Ok(())
}
```

This prints an event stream like:

```text
StreamStart bytes=Some(0..0) source=Some("")
DocumentStart(true, Some(YamlVersion { major: 1, minor: 2 })) bytes=Some(80..83) source=Some("---")
MappingStart(Block, 0, None) bytes=Some(84..84) source=Some("")
Scalar("items", Plain, 0, None) bytes=Some(84..89) source=Some("items")
node tag: !shopping custom=true tag_start(line,col,byte)=Some((6, 7, Some(91)))
SequenceStart(Block, 0, Some(Tag { handle: "!", suffix: "shopping", original_handle: "!" })) bytes=Some(103..103) source=Some("")
Scalar("milk", Plain, 0, None) bytes=Some(105..109) source=Some("milk")
scalar tag: tag:example.com,2000:sliced core-suffix=None core-str=false tag_start(line,col,byte)=Some((8, 4, Some(114))) for "bread"
Scalar("bread", Plain, 0, Some(Tag { handle: "tag:example.com,2000:", suffix: "sliced", original_handle: "!example!" })) bytes=Some(130..135) source=Some("bread")
scalar tag: tag:yaml.org,2002:str core-suffix=Some("str") core-str=true tag_start(line,col,byte)=Some((9, 4, Some(140))) for "bread"
Scalar("bread", Plain, 0, Some(Tag { handle: "tag:yaml.org,2002:", suffix: "str", original_handle: "!!" })) bytes=Some(146..151) source=Some("bread")
scalar tag: tag:yaml.org,2002:int core-suffix=Some("int") core-str=false tag_start(line,col,byte)=Some((10, 4, Some(156))) for "1"
Scalar("1", Plain, 0, Some(Tag { handle: "tag:yaml.org,2002:i", suffix: "nt", original_handle: "!core!" })) bytes=Some(165..166) source=Some("1")
SequenceEnd bytes=Some(167..167) source=Some("")
Scalar("locations", Plain, 0, None) bytes=Some(167..176) source=Some("locations")
Comment(" Example with composite keys", Right) bytes=Some(178..207) source=Some("# Example with composite keys")
MappingStart(Block, 0, None) bytes=Some(210..210) source=Some("")
SequenceStart(Flow, 0, None) bytes=Some(210..211) source=Some("[")
Scalar("47.3769", Plain, 0, None) bytes=Some(211..218) source=Some("47.3769")
Scalar("8.5417", Plain, 0, None) bytes=Some(220..226) source=Some("8.5417")
SequenceEnd bytes=Some(226..227) source=Some("]")
Scalar("local", Plain, 0, None) bytes=Some(229..234) source=Some("local")
SequenceStart(Flow, 0, None) bytes=Some(237..238) source=Some("[")
Scalar("40.7128", Plain, 0, None) bytes=Some(238..245) source=Some("40.7128")
Scalar("-74.0060", Plain, 0, None) bytes=Some(247..255) source=Some("-74.0060")
SequenceEnd bytes=Some(255..256) source=Some("]")
Scalar("remote", Plain, 0, None) bytes=Some(258..264) source=Some("remote")
Comment(" JSON-style \\uXXXX surrogate pairs:", Above) bytes=Some(266..302) source=Some("# JSON-style \\uXXXX surrogate pairs:")
MappingEnd bytes=Some(303..303) source=Some("")
Scalar("music", Plain, 0, None) bytes=Some(303..308) source=Some("music")
Scalar("𝄞🎵🎶", DoubleQuoted, 0, None) bytes=Some(310..348) source=Some("\"\\uD834\\uDD1E\\uD83C\\uDFB5\\uD83C\\uDFB6\"")
MappingEnd bytes=Some(349..349) source=Some("")
DocumentEnd bytes=Some(349..349) source=Some("")
StreamEnd bytes=Some(349..349) source=Some("")
```
## Key differences from saphyr-parser

All changes are intentionally scoped around correctness, compliance, and interoperability. This list summarizes 
differences done from the time of forking (0.0.6). Saphyr-parser may also have changes in its later releases.

### YAML compliance fixes

* **Invalid extra closing brackets are rejected**

  ```yaml
  [ a, b, c ] ]
  ```

* **Comments no longer truncate multiline scalars**

  ```yaml
  word1  # comment
  word2
  ```

  This is now correctly treated as invalid YAML instead of silently discarding content.

* **Reserved directives are ignored**

  Previously reported as errors; now handled according to the YAML specification.


### Compatibility adjustment
While linters would insist on strict compliance, other applications may prefer full backward compatibility. 
To balance requirements for all use cases, the compliance is governed by settings in [`Options`](https://docs.rs/granit-parser/latest/granit_parser/struct.Options.html).

* **Relaxed indentation for flow collection entries and delimiters**

  ```yaml
  key: [
  1, 2, 3,
  4, 5, 6
  ]
  ```

  While not strictly YAML-compliant, under-indented entries and delimiters in flow sequences
  and mappings are accepted for compatibility with PyYAML, ruamel.yaml, and real-world inputs.
  Under-indented implicit mapping keys must keep the key and its `:` on the same line.

  Set [`strict_indentation: true`](https://docs.rs/granit-parser/latest/granit_parser/struct.Options.html#structfield.strict_indentation) to require YAML-compliant flow indentation, including entries,
  delimiters, and multiline scalar continuations. The default is `false`.

  ```rust
  use granit_parser::{options, Parser};

  let parser = Parser::new_from_str_with_options(
      "key: [\n  1, 2, 3\n  ]\n",
      options! { strict_indentation: true },
  );
  assert!(parser.collect::<Result<Vec<_>, _>>().is_ok());
  ```


### JSON-style Unicode surrogate pairs
This parser supports explicit handling for JSON-style Unicode surrogate pairs in quoted scalar escape sequences.
* `\uXXXX` escapes that encode a high surrogate are now required to be followed immediately by a valid low surrogate escape, and both escapes are combined into the corresponding Unicode scalar value.
* Unpaired high surrogates, unpaired low surrogates, and reversed surrogate pairs are now rejected during scanning instead of being treated as generic invalid Unicode escape codes.

### Parsing correctness improvements

* **Plain scalar continuation fixed**

 Supports cases like:

  ```yaml
  hello:
    world: this is a string
      --- still a string
  ```

* **More helpful error reporting**
 
  Mismatched brackets and quotes now report the position of the opening token instead of the end of file.


### Performance

* **Zero-copy parsing for `&str` input**

  Uses `Cow<'input, str>` to avoid unnecessary allocations when parsing from in-memory strings.

* Comment handling
  If your application does not use comment parsing, set `Options::emit_comments = false` and disable the
  `comments` Cargo feature. The option suppresses capture and emission even if another dependency enables
  the feature through Cargo feature unification. Disabling the feature compiles out comment handling
  state when no dependency enables it. Comments can still be present in YAML, they are not emitted.

### Internal extensions

* **Parser stack support**

  Enables features such as `!include` by exposing additional internal capabilities.

### API migration notes

`Span` is non-exhaustive. Its public fields remain readable and writable, but external code
must use constructors instead of struct literals or struct-update syntax. Use the metadata
builders or assign individual fields, and include `..` when destructuring:

```rust
use granit_parser::{Marker, Span};

let start = Marker::new(0, 1, 0);
let end = Marker::new(3, 1, 3);
let span = Span::new(start, end)
    .with_indent(Some(0))
    .with_tag_start(Some(start));
let Span { start: actual_start, .. } = span;
assert_eq!(actual_start, start);
```

For custom `Input` implementations, `SkipTabs` now contains only the input policies `Yes` and
`No`. Whitespace results use `granit_parser::input::WhitespaceResult` instead:

- `skip_ws_to_eol` returns `(usize, Result<WhitespaceResult, ErrorKind>)`.
- `skip_ws_to_eol_blanks` returns `(usize, WhitespaceResult)`.

Replace `SkipTabs::Result(found_tabs, has_valid_yaml_ws)` with
`WhitespaceResult::new(found_tabs, has_valid_yaml_ws)`. The `found_tabs()` and
`has_valid_yaml_ws()` accessors remain available on the result. The tuple's character count
still reports consumed characters, including when `skip_ws_to_eol` returns an error.
These are breaking API changes intended for the next major release.

### Error handling without grepping message text
  
  Granit-parser uses the [`ErrorKind`](https://docs.rs/granit-parser/latest/granit_parser/enum.ErrorKind.html) enum to encode the error kind, and this enum may carry additional metadata. This is useful when messages need to be translated (built-in messages are English), or when the using code needs to handle some error specifically without greping for message text.

### Fallible streaming input

[`Parser::new_from_iter`](https://docs.rs/granit-parser/latest/granit_parser/struct.Parser.html#method.new_from_iter) remains available for genuinely infallible sources. Do not use it for an adapter that turns I/O, decoding, or size-limit failures into EOF: an incomplete YAML prefix may itself be a valid document.

Use [`Parser::new_from_fallible_iter`](https://docs.rs/granit-parser/latest/granit_parser/struct.Parser.html#method.new_from_fallible_iter) for a streaming source that can fail. It accepts an [`Iterator`](https://doc.rust-lang.org/std/iter/trait.Iterator.html) whose items are [`Result<char, ErrorKind>`](https://doc.rust-lang.org/std/result/enum.Result.html). The iterator protocol has three distinct outcomes:

* `Some(Ok(character))` supplies another character.
* `None` reports clean physical end-of-input.
* `Some(Err(error_kind))` reports a terminal source failure.

The parser returns a [`ScanError`](https://docs.rs/granit-parser/latest/granit_parser/struct.ScanError.html) carrying that same [`ErrorKind`](https://docs.rs/granit-parser/latest/granit_parser/enum.ErrorKind.html) and never polls the source again.
See the [fallible reader example](examples/fallible_reader.rs).

The lower-level [`Scanner`](https://docs.rs/granit-parser/latest/granit_parser/struct.Scanner.html)
is also fallible. Its iterator item is `Result<Token, ScanError>`, so collecting all tokens keeps
scanner failures distinct from clean EOF:

```rust
use granit_parser::{Scanner, StrInput};

let tokens = Scanner::new(StrInput::new("key: value\n"))
    .collect::<Result<Vec<_>, _>>()
    .expect("valid YAML should scan");

assert!(!tokens.is_empty());
```

### Security

This crate includes fixes to improve resilience against:

* denial-of-service inputs
* parser hangs
* panic conditions

Like the upstream parser, it does **not** interpret application-level types, so parsing YAML does not trigger external side effects.

### Improved ergonomics
The following ergonomic helpers are available:
- `Event::tag`
- `Event::scalar`
- `Event::anchor_id`
- `Event::alias_id`
- `Event::is_node`
- `Tag::parts`
- `Tag::handle`
- `Tag::suffix`
- `Tag::original_handle`
- `Tag::original_parts`
- `Tag::original`
- `Tag::core_suffix`
- `Tag::suffix_in_namespace`
- `Tag::is_custom`
- `Tag::is_yaml_core_schema_tag`
- `Span::slice`
- `Span::tag_start`
- `ParserStack::push_include`

See CHANGELOG.md for details.

### Unsupported features
YAML 1.1 line breaks. YAML 1.1 defines NEL (U+0085), LS (U+2028) and PS (U+2029) as line breaks, this parser
uses `\n`/`\r` only line-breaking that as expected in YAML 1.2. 

## Testing

Run the tests from a repository checkout with the official YAML suite initialized:

```sh
git submodule update --init tests/yaml-test-suite
cargo test
cargo test --test yaml-test-suite -- ::strict
```

Strict mode runs every active upstream case without case-specific overrides. The
runner reports cases disabled by upstream's own `skip: true` metadata separately
(currently the four `ZYU8` directive variants). The relaxed-mode compatibility
override for `9C9N` does not apply to strict mode.

Missing or empty suite data fails the run. The suite data is not included in the
published crate, so running these conformance tests requires a repository checkout.
The filesystem-backed suite is not run under Miri isolation.

## Tools

The repository includes a few developer tools for inspecting parser output and measuring performance.

Root package binaries:

* `dump_events` prints the parser event stream for a YAML file.
  ```sh
  cargo run --bin dump_events -- input.yaml
  ```
* `time_parser` measures one full parse and discards the events.
  ```sh
  cargo run --release --bin time_parser -- input.yaml
  ```
* `run_parser` runs repeated parses and reports aggregate timings.
  ```sh
  cargo run --release --bin run_parser -- input.yaml 10
  ```

Standalone helper crates:

* `walk` opens a small REPL for navigating parsed YAML spans.
  ```sh
  cargo run --manifest-path tools/walk/Cargo.toml -- input.yaml
  ```
* `bench_compare` compares benchmark output from multiple parsers.
  ```sh
  cargo bench_compare -- run_bench
  ```
* `gen_large_yaml` generates large YAML inputs for benchmark work.
  ```sh
  cargo gen_large_yaml -- --help
  ```

See `tools/README.md` and `tools/bench_compare/README.md` for the longer tool notes.


## License

Licensed under either:

* Apache License, Version 2.0
* MIT license

At your option.

This project inherits licensing terms from its upstream origins. See the `LICENSE` file and `.licenses/` directory for details.
