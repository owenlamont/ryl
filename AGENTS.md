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
