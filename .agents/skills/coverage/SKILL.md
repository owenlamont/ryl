---
name: coverage
description: >-
  Use when chasing missed lines/regions to satisfy CI's zero-missed-coverage
  gate, or writing tests to close uncovered ranges. Covers the
  coverage-missing.py workflow, coverage-friendly Rust idioms, and the common
  uncovered hotspots.
---

# Coverage Workflow

The CI enforces zero missed lines and zero missed regions. Use this workflow instead of
hunting through scattered tips:

1. First run `prek run --all-files` and rerun it until all automatic fixes have
   stabilised and a full pass succeeds without modifying files.
2. Quick status before pushing: run `uv run .agents/skills/coverage/coverage-missing.py`
   (cross-platform; needs `uv` + a Rust toolchain with `cargo-llvm-cov`). It reruns the
   coverage suite and prints any uncovered ranges, or explicitly confirms when coverage
   is complete.
3. If the coverage script itself fails, it prints the failing nextest output (the FAIL
   lines + panic tails); fix those tests, then rerun. Fall back to a manual
   `cargo nextest run` only when you need the full log.
4. If the script reports files, extend CLI/system tests targeting those ranges until
   the script produces no output.
5. For richer artifacts (HTML/LCOV), see the cargo-llvm-cov docs (HTML isn't easily
   machine-readable).
6. When coverage points to tricky regions, prefer CLI/system tests in `tests/`
   that drive `env!("CARGO_BIN_EXE_ryl")` so you exercise the same paths as users.
7. When you need to observe the exact flow through an uncovered branch, run the
   failing test under `rust-lldb` (ships with the toolchain). Start with
   `cargo test --no-run` and then
   `rust-lldb target/debug/deps/<test-binary> -- <filter args>` to set breakpoints
   on the problematic lines.
8. If cached coverage lingers, clear `target/llvm-cov-target` and rerun. But **a single
   stubborn uncovered region is usually an unreachable branch, not stale cache** — before
   clearing, confirm the missed line is reachable by *some* input. An unhit `if let`
   else / error arm looks identical to a cache miss but is closed by a test (or an
   `expect`), not a rebuild; chasing the cache first has burned multiple rebuilds.

## Coverage-Friendly Rust Idioms

- Guard invariants with `expect` (or an early `return Err(...)`) when the
  “else” branch is truly unreachable. Leaving a `return` in the unreachable path
  often shows up as a permanent uncovered region even though the condition is
  ruled out. Reserve `assert!` for test-only code or cases where a runtime panic
  is acceptable.
- When walking indices backwards, call `checked_sub(1).expect("…")` instead of
  matching on `checked_sub`; the `expect` documents the invariant and removes
  the uncovered `None` branch that instrumentation reports.
- When collecting spans, store the raw `(start, end)` and filter once at the end rather
  than pushing `Range` conditionally, so LLVM records the conversion branch once.
- Normalize prefix checks with `strip_prefix(...).expect(...)` when the prefix is already
  guaranteed; this removes the otherwise-uncovered `return` path.

Windows/MSVC: ensure the `llvm-tools-preview` component is installed (already listed in
`rust-toolchain.toml`). Run from a Developer Command Prompt if linker tools go missing.

## Common hotspots

- A CLI/system test that sets a subprocess working dir via `Command::current_dir` can stop
  the instrumented child writing its `.profraw` (so its lines read as uncovered). Drive
  coverage-sensitive dedup/path tests with absolute paths instead of changing cwd.
- Configuration discovery: use the `Env` abstraction (`discover_config_with`) and fake
  envs to hit inline data, explicit files (success and YAML failure), and env-var paths.
- Project configuration search: cover empty inputs, single files without parents, and
  multiple files in the same directory to trigger dedup logic.
- YAML parsing: drive `from_yaml_str` through string vs sequence options and ensure rule
  merges hit both update and insert branches.
- CLI context resolution: pass an empty `PathBuf` into `resolve_ctx` to trigger the
  fallback to `.`.
- Flow scanners in rules: always reconcile parser byte spans with `char_indices()` via
  `crate::rules::span_utils` to avoid off-by-byte bugs when UTF-8 characters appear.
- Rules using the shared `crate::rules::support::mapping_key_walker::Walker` to track
  key/value position must advance it for *every* node-producing event, including
  `Event::Alias` (call `Walker::skip_node`). An alias in value position (`k: *a`, or a
  `<<: *base` merge) that does not advance the walker desyncs the key/value alternation,
  so the following key is read as a value and vice-versa. Exercise rules with aliases in
  both key and value position.
- Resolving a scalar to its typed value (int/bool/null/float/string) is centralised in
  `crate::yaml_dom::scalar` (`resolve_scalar` / `resolve_plain_scalar`); reuse it rather
  than reinventing parsing. ryl resolves scalars per the YAML 1.2 **core** schema
  everywhere (leading-zero decimal is an int, an empty plain scalar is null, `0x`/`0o`
  radixes, full bool/null spelling sets); keep that schema choice consistent across rules
  instead of switching to JSON/1.1 semantics in any single rule.
- Matching a core-schema tag (`!!int`, `!!str`, …): use
  `crate::yaml_dom::core_schema_suffix(tag)` / `is_core_schema(tag)`, **never** granit's
  `Tag::is_yaml_core_schema` (it inspects only the *handle*, so a verbatim core tag
  `!<tag:yaml.org,2002:int>` slips past it). The shared helpers handle the canonical
  handle (incl. a resolving `%TAG`) and the verbatim spelling, but not a `%TAG` that
  splits the URI mid-token; to match one type regardless of split point, compare the
  full resolved URI (`handle` ++ `suffix`), as `rules::support::merge_key` does.
