# Coding Agent Instructions

Guidance on how to navigate and modify this codebase.

## What This Tool Does

ryl is a CLI tool for linting yaml files

## Project Structure

- **/src/** – All application code lives here.
- **/tests/** – Unit and integration tests.
- **pyproject.toml** - Package configuration
- **.pre-commit-config.yaml** - Prek managed linters and some configuration

## Coding Standards

- Code maintainability is the top priority - ideally a new agent can be onboarded onto
  using this repo and able to get all the necessary context from the documentation and
  code with no surprising behaviour or pitfalls (this is the pit of success principle -
  the most likely way to do something is also the correct way).
- In relation to maintainability / readability keep the code as succinct as practical.
  Every line of code has a maintenance and read time cost (so try to keep code readable
  with good naming of files, functions, structures, and variable instead of using
  comments). Remember every new conditional added has a significant testing burden as it
  will likely require a new test to be added and maintained. We want to keep code bloat
  to a minimum and the best refactors generally are those that remove lines of code
  while maintaining functionality.
- Comments should only be used to explain unavoidable code smells (arising from third
  party crate use), or the reason for temporary dependency version pinning (e.g.
  linking an unresolved GitHub issues) or lastly explaining opaque code or non-obvious
  trade-offs or workarounds.
- Leverage the provided linters and formatters to fix code, configuration, and
  documentation often - it's much cheaper to have the linters and formatters auto fix
  issues than correcting them yourself. Only correct what the linters and formatters
  can't automatically fix.
- No unsafe Rust code. Do not introduce `unsafe` in application code or tests. If a
  change appears to require `unsafe`, propose an alternative design that keeps code
  safe. The crate is built with `#![forbid(unsafe_code)]` and tests should follow the
  same principle.
- Remember the linter/formatter prek won't scan any new modules until they are added to
  git so don't forget to git add any new modules you create before running prek.
- Use the most modern Rust idioms and syntax allowed by the Rust version (currently this
  is Rust 1.89).
- Don't rely on your memory of libraries and APIs. All external dependencies evolve fast
  so ensure current documentation and/or repo is consulted when working with third party
  dependencies.

## Code Change Requirements

- Whenever any files are edited ensure all prek linters pass (run:
  `prek run --all-files`).
- Whenever source files are edited ensure the full test suite passes (run:
- `cargo llvm-cov nextest --summary-only`)
- For any behaviour or feature changes ensure all documentation is updated
  appropriately.

## Development Environment / Terminal

- This repo runs on Mac, Linux, and Windows. Don't make assumptions about the shell
  you're running on without checking first (it could be a Posix shell like Bash or
  Windows Powershell).
- `prek`, `rg`, `rumdl`, `typos`, `yamllint`, and `zizmor` should be installed as global
  tools (if they don't appear to be installed raise that with the user).
- `gh` will be available in most, but not all environments to inspect GitHub.
- Linters and tests may write outside the workspace (e.g., `~/.cache/prek`). If
  sandboxed, request permission escalation when running `prek`, `cargo test`,
  or coverage commands.
- Allow at least a 1-minute timeout per linter/test invocation; increase as
  needed for larger runs or CI.

## Automated Tests

- Don't use comments in tests, use meaningful function names, and variable names to
  convey the test purpose.
- Every line of code has a maintenance cost, so don't add tests that don't meaningfully
  increase code coverage. Aim for full branch coverage but also minimise the tests code
  lines to src code lines ratio.

## Coverage Playbook

The CI enforces zero missed lines and zero missed regions via cargo-llvm-cov.

- Quick status: `cargo llvm-cov nextest --summary-only`
- Text report with missing lines highlighted:
  `cargo llvm-cov --text --show-missing-lines
  --output-path target/llvm-cov/report.txt` (This is probably the best format for
  identifying missing coverage). Add focused tests if any ^0 markers remain.
- HTML: `cargo llvm-cov nextest --html` (open `target/llvm-cov/html/index.html`)
- LCOV for drilling into files: `cargo llvm-cov nextest --lcov --output-path lcov.info`

- Windows (MSVC) note: The MSVC toolchain is supported.
  Ensure the `llvm-tools-preview` component is installed (it is in
  `rust-toolchain.toml`). If you see linker tool issues, run from a Developer
  Command Prompt or ensure the MSVC build tools are in PATH.

### Hitting tricky regions in this repo

- Use the Env abstraction in `src/config.rs` to craft precise tests without
  touching the real FS/env.
  - Implement a tiny FakeEnv with predictable `current_dir`, `config_dir`,
    `read_to_string`, `path_exists`, and `env_var`.
  - Drive discovery via `discover_config_with(&inputs, &overrides, &fake_env)`.

- `discover_config_with` branches:
  - Inline data path: `Overrides { config_data: Some(..) }`.
  - Explicit file path: `Overrides { config_file: Some(..) }` (hit both valid
    parse and invalid YAML error).
  - Env var/core path: use `discover_config_with_env` with a closure or FakeEnv.

- `find_project_config_core` search/dedup:
  - Inputs empty → ensure a `.yamllint` in `current_dir()`.
  - Single file with no parent → `.parent()` is `None`, exercises
    `map_or_else` path that falls back to `current_dir()`.
  - Multiple files in same directory → exercises the dedup branch
    (`!starts.iter().any(..)`).

- `from_yaml_str` parsing arms (common micro‑regions):
  - `ignore` as string and as sequence (include non‑string items to hit skip paths).
  - `yaml-files` as string and as sequence (include non‑string items and all
    non‑string sequence).
  - `rules` with both updating an existing rule (from `extends: default`) and
    inserting a new rule (exercises `merge_from` branches and rule name
    accumulation).

- `resolve_ctx` in `src/main.rs`:
  - Use an empty `PathBuf` to exercise the
    `parent().map_or_else(|| PathBuf::from("."), ..)` branch.

### Triage tips

- If lines are 100% but regions are not, look for:
  - Short‑circuiting conditions and inline closures (`map_or_else`, `any`, etc.).
  - Multi‑arm `if let` / `match` on the same line.
  - Sequence vs scalar variants in config parsing.

- To pinpoint: search for zero‑count regions in the text export.
  - `awk` or `rg` against `target/llvm-cov/report.txt` for `^0` markers helps
    isolate the exact source line.

### CI policy

- CI step enforces: `--fail-uncovered-lines 0 --fail-uncovered-regions 0`.
- Aim to keep local runs green with: `cargo llvm-cov nextest --summary-only`
  before pushing.

## Release Checklist

- Bump versions in lockstep:
  - Cargo: update `Cargo.toml` `version`.
  - Python: update `pyproject.toml` `[project].version`.
- Refresh lockfile and validate:
  - Run `cargo generate-lockfile` (or `cargo check`) to refresh `Cargo.lock`.
  - Stage: `git add Cargo.toml Cargo.lock pyproject.toml`.
  - Run `prek run --all-files` (re-run if files were auto-fixed).
- Docs and notes:
  - Update README/AGENTS for behavior changes.
  - Summarize notable changes in the PR description or changelog (if present).
- Tag and push (when releasing):
  - `git tag -a vX.Y.Z -m "vX.Y.Z"`
  - `git push && git push --tags`
  - Releases are handled by `.github/workflows/release.yml` (publishes to
    crates.io, then PyPI).

## CLI Behavior

- Accepts one or more inputs: files and/or directories.
- Directories: recursively scan `.yml`/`.yaml` files, honoring git ignore and
  git exclude; does not follow symlinks.
- Files: parsed as YAML even if the extension is not `.yml`/`.yaml`.
- Exit codes: `0` (ok/none), `1` (invalid YAML), `2` (usage error).
