# Changelog

## Unreleased

First release as `ryl-yaml-parser`, forked from
[granit-parser](https://github.com/bourumir-wyngs/granit-parser) 2.0.0, whose changelog
records the history before the fork.

### Breaking changes

- Removed the `comments`, `error_messages`, `std` and `debug_prints` Cargo features and
  `no_std` support. Comment events, error messages and `std::io::Error` retention are
  always available; `Options::emit_comments` still turns comment emission off.
- Raised the minimum supported Rust version to 1.99.0 and moved to edition 2024.
- Removed the `dump_events`, `time_parser` and `run_parser` binaries and the examples.
