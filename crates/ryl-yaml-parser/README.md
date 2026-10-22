# ryl-yaml-parser

The YAML 1.2 parser behind [ryl](https://github.com/owenlamont/ryl), the YAML linter and
formatter. It emits parser events with byte spans, comments and structure-style
information, and is tuned for ryl's needs.

This crate is a fork of [granit-parser](https://github.com/bourumir-wyngs/granit-parser)
2.0.0 (commit `d2bde4f`) by Bourumir Wyngs, which itself descends from
[saphyr-parser](https://crates.io/crates/saphyr-parser) and
[yaml-rust](https://github.com/chyh1990/yaml-rust). The license notices of every
contributor in that lineage are kept in [`LICENSE`](LICENSE) and
[`.licenses/`](.licenses). granit-parser's README and changelog record how it differs
from saphyr-parser.

## Versioning

`ryl-yaml-parser` follows [semantic versioning](https://semver.org) on its own public API,
independently of ryl's version. While it is at 0.x, a minor-version bump may break the API.
Its minimum supported Rust version matches ryl's.

## Minimal example

[`Parser::new_from_str`](https://docs.rs/ryl-yaml-parser/latest/ryl_yaml_parser/struct.Parser.html#method.new_from_str) returns an iterator
of ([`Event`](https://docs.rs/ryl-yaml-parser/latest/ryl_yaml_parser/enum.Event.html), [`Span`](https://docs.rs/ryl-yaml-parser/latest/ryl_yaml_parser/struct.Span.html)) pairs. The event
helpers expose common node metadata, and spans provide byte ranges, source slices, and
explicit tag-token starts for tagged nodes.

Comments are emitted as `Event::Comment(text, placement)`. They are presentation
metadata for tools such as linters and formatters, not YAML data nodes, so consumers that
build YAML values should filter them out. A comment's span covers the whole source
comment, including `#` and excluding the line break. Setting
[`Options::emit_comments`](https://docs.rs/ryl-yaml-parser/latest/ryl_yaml_parser/struct.Options.html#structfield.emit_comments) to `false`
stops their capture and emission; comments are still recognized and validated.

Document starts are emitted as `Event::DocumentStart(explicit, version)`, where `version`
is the optional [`YamlVersion`](https://docs.rs/ryl-yaml-parser/latest/ryl_yaml_parser/struct.YamlVersion.html) declared by a preceding
`%YAML` directive for that document.

```rust
use ryl_yaml_parser::Parser;

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

## Strict indentation

Under-indented entries and delimiters in flow collections are accepted by default, for
compatibility with PyYAML, ruamel.yaml and real-world inputs:

```yaml
key: [
1, 2, 3,
4, 5, 6
]
```

Under-indented implicit mapping keys must keep the key and its `:` on the same line. Set
[`strict_indentation: true`](https://docs.rs/ryl-yaml-parser/latest/ryl_yaml_parser/struct.Options.html#structfield.strict_indentation) to
require YAML-compliant indentation of flow entries, delimiters and multiline scalar
continuations:

```rust
use ryl_yaml_parser::{options, Parser};

let parser = Parser::new_from_str_with_options(
    "key: [\n  1, 2, 3\n  ]\n",
    options! { strict_indentation: true },
);
assert!(parser.collect::<Result<Vec<_>, _>>().is_ok());
```

## Errors

Every failure carries an [`ErrorKind`](https://docs.rs/ryl-yaml-parser/latest/ryl_yaml_parser/enum.ErrorKind.html), so callers can translate
messages or handle a specific error without matching message text.

[`Parser::new_from_iter`](https://docs.rs/ryl-yaml-parser/latest/ryl_yaml_parser/struct.Parser.html#method.new_from_iter) is for genuinely
infallible sources. A streaming source that can fail belongs in
[`Parser::new_from_fallible_iter`](https://docs.rs/ryl-yaml-parser/latest/ryl_yaml_parser/struct.Parser.html#method.new_from_fallible_iter),
whose iterator yields `Result<char, ErrorKind>`; turning a failure into EOF would let an
incomplete prefix parse as a valid document:

- `Some(Ok(character))` supplies another character.
- `None` reports clean physical end-of-input.
- `Some(Err(error_kind))` reports a terminal source failure. The parser returns a
  [`ScanError`](https://docs.rs/ryl-yaml-parser/latest/ryl_yaml_parser/struct.ScanError.html) carrying that `ErrorKind` and never polls the
  source again.

The lower-level [`Scanner`](https://docs.rs/ryl-yaml-parser/latest/ryl_yaml_parser/struct.Scanner.html) is fallible too:

```rust
use ryl_yaml_parser::{Scanner, StrInput};

let tokens = Scanner::new(StrInput::new("key: value\n"))
    .collect::<Result<Vec<_>, _>>()
    .expect("valid YAML should scan");

assert!(!tokens.is_empty());
```

## Observe lexical tokens

An observer receives scanner emission order, including synthetic keys, comments and stream
end. Callback payloads borrow scanner storage; parser errors can stop observation before
stream end. Existing constructors retain a no-op observer.

```rust
use ryl_yaml_parser::{options, Parser, Span, StrInput, TokenKind};

let mut tokens = 0;
let parser = Parser::with_options_and_observer(
    StrInput::new("key: value\n"),
    options! { strict_indentation: true },
    |_: &Span, _: TokenKind<'_>| tokens += 1,
);
assert!(parser.collect::<Result<Vec<_>, _>>().is_ok());
assert!(tokens > 0);
```

## Unsupported

YAML 1.1 line breaks: NEL (U+0085), LS (U+2028) and PS (U+2029). Only `\n` and `\r`
break lines, as YAML 1.2 specifies.

## Testing

Run the tests from the ryl repository root with the official YAML suite initialized:

```sh
git submodule update --init crates/ryl-yaml-parser/tests/yaml-test-suite
cargo nextest run -p ryl-yaml-parser
```

Strict mode runs every active upstream case without case-specific overrides. The runner
reports cases disabled by upstream's own `skip: true` metadata separately (currently the
four `ZYU8` directive variants). The relaxed-mode compatibility override for `9C9N` does
not apply to strict mode. Missing or empty suite data fails the run; the suite data is not
in the published crate.

## License

Licensed under either the Apache License, Version 2.0 or the MIT license, at your option.
The crate inherits licensing terms from its upstream origins; see [`LICENSE`](LICENSE) and
[`.licenses/`](.licenses).
