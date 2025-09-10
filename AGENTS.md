# Coding Agent Instructions

Guidance on how to navigate and modify this codebase.

## What This Tool Does

ryl is a CLI tool for linting yaml files

## Code Change Requirements

- Whenever code is changed ensure all pre-commit linters pass (run:
  `prek run --all-files`)
- For any behaviour or feature changes ensure all documentation is updated
  appropriately.

## Project Structure

- **/src/** – All application code lives here.
- **/tests/** – Unit and integration tests.
- **pyproject.toml** - Package configuration
- **.pre-commit-config.yaml** - Pre-commit linters and some configuration

## Code Style

- Remember pre-commit won't scan any new modules until they are added to git so don't
  forget to git add any new modules you create before running pre-commit.
- pre-commit will auto correct many lint and format issues, if it reports any file
  changes run a second time to see if it passes (some errors it reports on a first run
  may have been auto-corrected). Only manually resolve lint and format issues if
  pre-commit doesn't report correcting or changing any files.
- Use the most modern Rust idioms and syntax allowed by the Rust version (currently this
  is Rust 1.89).
- Comments should be kept to an absolute minimum, try to achieve code readability
  through meaningful class, function, and variable names.
- Comments should only be used to explain unavoidable code smells (arising from third
  party crate use), or the reason for temporary dependency version pinning (e.g.
  linking an unresolved GitHub issues) or lastly explaining opaque code or non-obvious
  trade-offs or workarounds.

## Development Environment / Terminal

- This repo runs on Mac, Linux, and Windows. Don't make assumptions about the shell
  you're running on without checking first (it could be a Posix shell like Bash or
  Windows Powershell).
- `prek`, `rumdl`, `typos`, and `zizmor` should be installed as global uv tools.

## Automated Tests

- Don't use comments in tests, use meaningful function names, and variable names to
  convey the test purpose.
- Every line of code has a maintenance cost, so don't add tests that don't meaningfully
  increase code coverage. Aim for full branch coverage but also minimise the tests code
  lines to src code lines ratio.
- Coverage with nextest is supported via `cargo-llvm-cov`.
  - Run tests with coverage:
    - Quick summary: `cargo llvm-cov nextest --summary-only`
    - HTML report: `cargo llvm-cov nextest --html`
      (open `target/llvm-cov/html/index.html`)
    - LCOV (for CI): `cargo llvm-cov nextest --lcov --output-path lcov.info`
    - Cobertura XML: `cargo llvm-cov nextest --cobertura --output-path coverage.xml`
  - Clean coverage artifacts: `cargo llvm-cov clean --workspace`
  - Windows (MSVC) note: The MSVC toolchain is supported.
    Ensure the `llvm-tools-preview` component is installed (it is in
    `rust-toolchain.toml`). If you see linker tool issues, run from a Developer
    Command Prompt or ensure the MSVC build tools are in PATH.

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

## Coverage and CI Notes

- Coverage uses `cargo-llvm-cov` with nextest; ignored tests are included.
  - Quick summary: `cargo llvm-cov nextest --summary-only`.
  - Include ignored: add `--run-ignored all`.
  - LCOV for artifacts: `cargo llvm-cov nextest --lcov --output-path lcov.info`.
- Branch coverage:
  - Stable Rust does not emit branch data; PR comment omits “Missed Branches”.
  - Nightly + `--branch` is required for BRDA output (not used in CI).
- yamllint gotcha:
  - In CI, yamllint may auto-select the “github” format; tests force
    `-f standard` to keep output stable.

## CLI Behavior

- Accepts one or more inputs: files and/or directories.
- Directories: recursively scan `.yml`/`.yaml` files, honoring git ignore and
  git exclude; does not follow symlinks.
- Files: parsed as YAML even if the extension is not `.yml`/`.yaml`.
- Exit codes: `0` (ok/none), `1` (invalid YAML), `2` (usage error).
