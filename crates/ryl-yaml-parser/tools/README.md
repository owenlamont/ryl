# `granit-parser` tools
This directory contains tools that are used to develop the crate.
Due to dependency management, only some of them are available as binaries from the root `granit-parser` crate.

| Tool | Invocation |
|------|------------|
| `dump_events` | `cargo run --bin dump_events -- [...]` |
| `run_parser` | `cargo run --bin run_parser -- [...]` |
| `time_parser` | `cargo run --bin time_parser -- [...]` |
| `codspeed` | `cargo +stable bench --manifest-path tools/codspeed/Cargo.toml --bench parser --locked` |
| `walk` | `cargo run --manifest-path tools/walk/Cargo.toml -- [...]` |

## `codspeed`

The parser benchmarks live in an unpublished standalone package with their own
dependencies and lockfile. Run this package with stable Rust; CodSpeed's toolchain
requirements are independent of the parser's Rust 1.81 minimum version.

Run the benchmarks locally from the repository root:

```sh
cargo +stable bench --manifest-path tools/codspeed/Cargo.toml --bench parser --locked
```

To benchmark without parser comments, add `--no-default-features --features std,error_messages`.
The package forwards the parser's `comments`, `std`, `error_messages`, and
`debug_prints` features.

For CodSpeed instrumentation, install `cargo-codspeed` and run from the package directory:

```sh
cd tools/codspeed
cargo +stable codspeed build -m simulation --locked
cargo +stable codspeed run
```

The CodSpeed workflow runs these commands on stable Rust. The root CI and release
checks continue to check all root package targets with Rust 1.81.

## `dump_events`
This is a debugging helper for the parser. It outputs events emitted by the parser for a given file.

### Example
Consider the following `input.yaml` YAML file:
```yaml
- foo: bar
- baz:
  c: [3, 4, 5]
```

Running `cargo run --bin dump_events -- input.yaml` outputs:
```
      ↳ StreamStart
      ↳ DocumentStart
      ↳ SequenceStart(Block, 0, None)
      ↳ MappingStart(Block, 0, None)
      ↳ Scalar("foo", Plain, 0, None)
      ↳ Scalar("bar", Plain, 0, None)
      ↳ MappingEnd
      ↳ MappingStart(Block, 0, None)
      ↳ Scalar("baz", Plain, 0, None)
      ↳ Scalar("~", Plain, 0, None)
      ↳ Scalar("c", Plain, 0, None)
      ↳ SequenceStart(Flow, 0, None)
      ↳ Scalar("3", Plain, 0, None)
      ↳ Scalar("4", Plain, 0, None)
      ↳ Scalar("5", Plain, 0, None)
      ↳ SequenceEnd
      ↳ MappingEnd
      ↳ SequenceEnd
      ↳ DocumentEnd
      ↳ StreamEnd
```

Verbose scanner/parser debug output is compiled only with the `debug_prints` feature. It is also guarded by the local `ENABLED` constant in `src/debug.rs`; flip that constant in a local working tree when investigating parser behavior.

```sh
cargo run --features debug_prints --bin dump_events -- input.yaml
```

## `run_parser`
This is a benchmarking helper that runs the parser on the given file a given number of times and is able to extract simple metrics out of the results. The `--output-yaml` flag can be specified to make the output a YAML file that can be fed into other tools.

This binary is made to be used by `bench_compare`.

Synopsis: `run_parser input.yaml <iterations> [--output-yaml]`

### Examples
```sh
$> cargo run --release --bin run_parser -- bench_yaml/big.yaml 10
Average: 1.631936191s
Min: 1.629654651s
Max: 1.633045284s
95%: 1.633045284s

$> cargo run --release --bin run_parser -- bench_yaml/big.yaml 10 --output-yaml
parser: granit-parser
input: bench_yaml/big.yaml
average: 1649847674
min: 1648277149
max: 1651936305
percentile95: 1651936305
iterations: 10
times:
  - 1650216129
  - 1649349978
  - 1649507018
  - 1648277149
  - 1649036548
  - 1650323982
  - 1650917692
  - 1648702081
  - 1650209860
  - 1651936305
```

## `time_parser`
This is a benchmarking helper that times how long it takes for the parser to emit all events. It calls the parser on the given input file, receives parsing events and then immediately discards them. It is advised to run this tool with `--release`.

### Examples
Loading a small file could output the following:
```sh
$> cargo run --release --bin time_parser -- input.yaml
Loaded 0MiB in 14.189µs
```

While loading a larger file could output the following:
```sh
$> cargo run --release --bin time_parser -- bench_yaml/big.yaml
Loaded 220MiB in 1.612677853s
```

## `walk`
A simple REPL to visualize spans of a YAML file.

Synopsis: `cargo run --manifest-path tools/walk/Cargo.toml -- input.yaml`

For commands, refer to `read_action` in `walk.rs`.
