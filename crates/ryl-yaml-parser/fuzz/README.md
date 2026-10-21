# Fuzzing

Run the deterministic checks of the fuzz oracles with the normal test suite:

```sh
cargo test --test fuzz_targets
```

These tests call the same functions as libFuzzer, including accepted/rejected resource-limit
boundaries, every scalar chomping variant, and the parser-stack backend and document-tail matrix.
They require no fuzzing dependencies or nightly compiler.

For a bounded fuzzing run from the repository root:

```sh
cargo +nightly fuzz build parser_stack
rustc +nightly --edition 2021 fuzz/seed_corpus.rs -o fuzz/target/seed_corpus
fuzz/target/seed_corpus parser_stack fuzz/corpus/parser_stack
cargo +nightly fuzz run parser_stack -- -dict=fuzz/yaml.dict -max_total_time=60 -timeout=10
```

The seed generator adds named seeds without replacing existing corpus files. It includes the
control bytes required to reach each generated construction; raw YAML alone does not select
those constructions reliably. CI seeds and runs every target. `parse_any` and `scan_any` receive
the raw changelog regression inputs directly.

| Target | Checks |
| --- | --- |
| `parse_any`, `scan_any` | Full traces across string, buffered and all-OK fallible inputs; errors, spans, fusion and valid event prefixes even for malformed YAML |
| `options` | All resource options; exact directive-byte, reserved-parameter and block-nesting limits; strict indentation and comment suppression |
| `directives_tags` | Tag resolution, primary-handle overrides, non-specific `!`, and reserved-directive comments excluded from limits |
| `parser_stack` | Seven input/include/replay backends; trailing comments, extra documents, nested error context, repeated peeks and fusion |
| `flow_collections` | Generated flow syntax, initial versus interior `\|`/`>` characters, and tabs after mapping colons |
| `large_scalars` | Exact scalar values, folding, root document boundaries, chomping and UTF-8 spans at sizes up to 256 KiB |
| `fallible_input` | Injected I/O, decoding and byte-limit errors; no source polling after failure |
| `aliases_merges`, `duplicate_keys` | Anchor/alias relationships and preservation of duplicate mapping entries |

Most existing selector values retain their constructions. New semantic cases use high selector
bytes: 240–255 for directives/tags, 252–254 for options and flow collections, and 254 for root
block scalars. Each target documents its remaining control bytes.
For raw `options` inputs, bit `0x80` of the second byte selects strict indentation for parser,
scanner, and comment-suppression checks. The seed corpus includes conforming and under-indented
inputs in both modes, with comment emission enabled and disabled.

When a run fails, retain the artifact under `fuzz/artifacts/<target>/`, reproduce and minimize it,
then add a deterministic regression. Establish the expected result from the YAML rules or public
API contract before changing an oracle. A parser defect should remain reproducible until the
implementation is fixed.

AddressSanitizer is enabled by default. Environments running under `ptrace` may prevent
LeakSanitizer from working at process exit. In that specific case, rerun locally with
`ASAN_OPTIONS=detect_leaks=0:detect_odr_violation=0`; this retains address checks. CI does not
disable leak detection.
