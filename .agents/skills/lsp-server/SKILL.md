---
name: lsp-server
description: >-
  Use when changing `ryl server` (`src/lsp/`) — diagnostics (push, pull, and the
  long-polled workspace scan), code actions, hover, rename, or position
  encoding. Covers the module split, the cancellable background scan, and the
  UTF-16 column trap that BMP fixtures pass vacuously.
---

# Language Server

`ryl server`, `src/lsp/`, behind the default-on `lsp` cargo feature: a synchronous
`lsp-server`+`lsp-types` adapter over the engine. User docs:
`docs/editor-integration.md`.

## Layout

`serve(&Connection)` runs the handshake + message loop; `run()` wires stdio and drops the
connection before `io_threads.join()` so the writer thread finishes. `mod.rs` is the
protocol loop/dispatch; the connection-free logic lives in submodules so it is
unit/property testable: `encoding` (position math + `uri_to_path`/`path_to_uri`),
`analysis` (lint/fix → LSP), `actions` (code-action builders), `hover`, `rename`.

## Diagnostics

Push diagnostics (`publishDiagnostics`) are gated on `Server::push_diagnostics` =
`!client_supports_pull_diagnostics`, so a pull-capable client gets diagnostics once via
pull, not twice — clients like VS Code merge the two channels.

Pull diagnostics are `textDocument/diagnostic` + `workspace/diagnostic`, the latter over
a per-entry-cancellable `discover::gather_yaml_from_dir_cancellable` walk of every root,
deduped. It runs on a background worker thread — so the message loop stays responsive —
lints files in parallel via `rayon`, and is cancellable via `$/cancelRequest`/shutdown
through an `AtomicBool` the worker checks (the walk per entry, the lint per `SCAN_BATCH`
batch, so only an in-flight batch is uninterruptible). A new pull supersedes/cancels any
in-flight one (bounding workers); `serve` joins outstanding workers before returning.

Each report carries a `result_id` (`analysis::result_id`, SHA-256 of the serialized
diagnostics; `None` for a clean file, which is then omitted), so a matching
`previousResultIds` entry answers `Unchanged`, and a previously-reported path the walk no
longer covers is cleared with an empty, id-less report.

## The long poll

**`workspace/diagnostic` is long-polled** (#408, after ty): the VS Code client re-pulls
a fixed 2 s after every response with no knob, so an all-`Unchanged` report (an empty one
included) is *not* answered — `finish_scan` parks it in `Server::pull` and `wake`
re-scans on the next didOpen/didChange/didClose/watched-file/config notification.

A worker cannot see session state, so scans return over `scan_tx`/`scan_rx` into a
`crossbeam_channel::select!` in `run_loop`, and `Server::revision` (bumped by those
notifications) stops a scan that raced a change from suspending on a stale report. A
parked pull is answered on `$/cancelRequest`, when a new pull supersedes it, and at
shutdown. The watcher registration covers `**/*.{yaml,yml}` as well as config names so an
out-of-editor change can wake it; `is_config_uri` keeps a source change from being taken
for a config one.

A `partialResultToken` switches `ReportSink` from bulk to streaming: the scan lints in
`SCAN_BATCH` batches (also the cancellation granularity) and `Full` reports go out as
`$/progress` batches — the first at once, then per `STREAM_INTERVAL` — while `Unchanged`
ones are held for the response, so nothing is sent twice. `ScanOutcome::streamed` then
forces an answer: having streamed, the request can no longer be held open.

## Other capabilities

`source.fixAll.ryl` + per-rule `source.fixAll.ryl.<rule>` (via `fix::SAFE_FIX_RULE_IDS`,
YAML only) + `quickfix` disable-rule inserts (`# ryl disable-line` / first-line
`# ryl disable-file`; the disable-line is suppressed for a diagnostic inside a block
scalar, where a `#` would be content not a directive, via `protected_scalar_lines`);
`textDocument/formatting`; hover (rule + message + docs link for a covered diagnostic);
anchor/alias `rename` + `prepareRename` (granit scanner tokens, document-scoped, YAML
only); and INCREMENTAL sync (ranged edits applied via `encoding::offset_at`). The engine
has no per-occurrence fix; code actions honour `context.only`.

## Configuration

Resolved per document via `discover_config` (full CLI precedence incl.
`YAMLLINT_CONFIG_FILE`), layering the client's `Settings` (`initializationOptions` /
`workspace/didChangeConfiguration`: `configPath`/`configData`/`enable`, CLI-equivalent
precedence).

Config-file changes go through `handle_config_change` via a dynamic
`didChangeWatchedFiles` registration: a push client gets a re-lint+re-push, a pull
client (whose pushes are gated off) is asked to re-pull via
`workspace/diagnostic/refresh` when it advertised `refreshSupport`
(`client_supports_diagnostic_refresh`), else it re-pulls on its own cadence.
`workspace/configuration` pull is deferred. A rule-less/absent config or `enable:false`
lints nothing silently; a malformed one lints nothing but is surfaced once via
`window/showMessage` (no hard exit-2).

## Position encoding is the one load-bearing piece

LSP columns are UTF-16 code units by default (NOT ryl's 1-based code-point columns);
`encoding::problem_range`/`offset_at` walk the line CR-aware via `line_syntax` and the
negotiated encoding (UTF-8/16/32), so multibyte/astral-plane columns need real
surrogate-pair fixtures (BMP `café`/`å` pass vacuously).

## Build

`lsp-types` 0.97 forces a benign `bitflags` 1-vs-2 duplicate, allowlisted in
`clippy.toml`. The `lsp` feature must stay compilable out: CI runs `cargo clippy
--no-default-features` (the LSP tests are `#![cfg(feature = "lsp")]`).
