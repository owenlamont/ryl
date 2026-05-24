# Coding Agent Instructions

Guidance on how to navigate and modify this codebase.

## What This Tool Does

ryl is a CLI tool for linting yaml files

## Project Structure

- **/src/** – All application code lives here.
- **/tests/** – Unit and integration tests.
- **/docs/** – Source content for the Zensical documentation site.
- **pyproject.toml** - Package configuration
- **zensical.toml** - Documentation site configuration
- **prek.toml** - Prek managed linters and some configuration

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
  is Rust 1.93.1).
- Keep `clippy.toml` `msrv` in sync with `rust-toolchain.toml` whenever the Rust
  toolchain channel is changed.
- Don't rely on your memory of libraries and APIs. All external dependencies evolve fast
  so ensure current documentation and/or repo is consulted when working with third party
  dependencies.
- When mirroring yamllint behaviour, spot-check tricky inputs with the ryl CLI so
  our diagnostics and message text match (e.g., mixed newline styles or config keys of
  type int/bool/null/tagged scalar).
- Keep YAML configuration strictly aligned with functionality that yamllint currently
  supports. Put any ryl-only settings, experimental rule options, or ahead-of-upstream
  behaviour in TOML configuration so future yamllint additions cannot clash with
  existing YAML semantics.

## Code Change Requirements

- Whenever any files are edited ensure all prek linters pass (run:
  `prek run --all-files`).
- `prek` already runs the key tooling (e.g., trim/fix whitespace, `cargo fmt`,
  `cargo clippy --fix`, `cargo clippy`, `rumdl` for Markdown/docs, etc.), so skip
  invoking those individually. Re-run `prek run --all-files` until the auto-fixes
  stabilise and a full pass succeeds without modifying files before running coverage.
- Whenever source files are edited ensure the full test suite passes (run
  `./scripts/coverage-missing.sh` (Unix) or
  `pwsh ./scripts/coverage-missing.ps1` (Windows) to regenerate coverage; it reports
  uncovered ranges and confirms when coverage is complete)
- After lint, tests, and coverage are green, review code size changes with
  `uv run scripts/source_size.py --compare-to <branch-or-ref>` (typically the branch
  point or `HEAD`). If the size increase looks large relative to the added
  functionality, look for opportunities to make the implementation DRYer, reuse shared
  helpers, or simplify it before committing.
- For any behaviour or feature changes ensure all documentation is updated
  appropriately.

## Development Environment / Terminal

- This repo runs on Mac, Linux, and Windows. Don't make assumptions about the shell
  you're running on without checking first (it could be a Posix shell like Bash or
  Windows Powershell).
- `prek`, `rg`, `rumdl`, `typos`, `yamllint`, and `zizmor` should be installed as global
  tools (if they don't appear to be installed raise that with the user).
- `gh` will be available in most, but not all environments to inspect GitHub.
- When reviewing PR feedback, prefer `gh pr view <number> --json comments,reviews` for
  summary threads and `gh api repos/<owner>/<repo>/pulls/<number>/comments` when you
  need inline review details without guesswork. Avoid flags that the GitHub CLI does not
  support (e.g., `--review-comments`).
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
- Do not add `#[cfg(test)]` test modules directly inside files under `src/`. Unit tests
  compiled alongside the library create duplicate LLVM coverage instantiations and break
  the "zero missed regions" guarantee enforced by CI. Add new coverage via CLI/system
  tests in `tests/` instead.

### Property Tests For Safe Fixes

`tests/property_safe_fix.rs` runs `proptest`-generated YAML through `apply_safe_fixes`
and asserts three invariants: idempotence, no remaining safe-fix-rule diagnostics
after fixing, and parse preservation (input that parses must produce output that
parses to an equal `YamlOwned` value). Deterministic sibling tests pin known-dirty
documents and known production-bug patterns (issues #184 and #206) through the same
checks so the property assertions cannot silently become a no-op if the generator
drifts.

The suite runs against a matrix of named configs to catch config-specific
regressions: five YAML configs (`yamllint-default`, `best-practice`, `strict-single`,
`strict-double`, `consistent`) exercising the surface yamllint exposes, plus one
TOML-backed config (`best-practice-toml`) loaded from a tempfile via
`discover_config` so ryl-only options like `allow-double-quotes-for-escaping` are
also covered.

When you add a new `FixSafety::Safe` rule:

1. Add its rule id to `SAFE_FIX_RULES` and to `COMMON_SAFE_FIX_RULES_YAML` in
   `tests/property_safe_fix.rs`. If the new rule introduces meaningful config
   axes, add a variant to `QUOTED_STRINGS_VARIANTS` (or a peer constant for that
   rule) so the matrix exercises each regime; ryl-only options must go through
   the TOML slot rather than YAML.
2. Extend the AST / renderer in that file so generated documents exercise the
   syntax the new fixer targets. Skipping this leaves the property tests green
   for the wrong reason — the fixer has nothing to do.
3. Run `cargo test --test property_safe_fix` and resolve any failures before
   landing the rule.
4. Add a focused CLI-level regression test in `tests/cli_fix.rs` (or the
   rule-specific file) for any production bug discovered along the way, so the
   property suite is backed by a deterministic guard.

Failing inputs are persisted at `tests/proptest-regressions/property_safe_fix.txt`
and replayed first on every run. That file is committed to git so the regression
follows the codebase, not the developer's machine.

### Rules Without A Safe `--fix`

These rules are intentionally not part of `SAFE_FIX_RULES`. Each entry is the
one-sentence reason `--fix` cannot rewrite the rule without risking changed
parsed values or unintended user-visible behaviour. Revisit this list when
considering a partial safe fix — if you can satisfy the property-test
invariants for some subset, move the rule into `SAFE_FIX_RULES` and document
the unsafe-trigger subset in that rule's module-level doc comment instead.

- `anchors` — Fixing requires choosing which anchor an undeclared alias
  should point at, which duplicate to keep, or whether an "unused" anchor is
  actually referenced from a template the linter cannot see.
- `colons` — Collapsing extra space around colons safely needs precise parser
  context tracking (plain scalars, alias keys, explicit `?`/`:` mappings)
  equivalent to re-implementing the YAML mapping scanner.
- `empty-values` — The rule's intent is to force the user to choose between
  `~`, `null`, or restructuring; auto-inserting a literal contradicts the
  rule's purpose and would silently change downstream behaviour.
- `float-values` — Rewrites such as `0.5 → .5`, `.5 → 0.5`, expanding
  `1e3 → 1000`, or replacing `.nan`/`.inf` all change the scalar's string
  representation and, in tagged or string-typed consumers, its semantic value.
- `hyphens` — Collapsing trailing spaces after `-` in a block sequence
  changes the indent of any nested block mapping/sequence that follows on
  subsequent lines and so can change the parsed structure.
- `indentation` — Re-indenting alters the block-structure boundaries the
  YAML grammar uses to delimit mappings, sequences, and scalars; any
  non-trivial fix risks changing the parsed value.
- `key-duplicates` — Resolving a duplicate requires deciding which key (and
  value) to keep; both choices alter the parsed mapping and need user intent.
- `key-ordering` — Reordering a mapping silently disassociates any comment
  the user placed above or beside a key from that key, losing information the
  YAML grammar does not carry.
- `line-length` — Splitting an over-long line requires line-folding decisions
  that depend on whether the scalar is plain, quoted, or block-styled, and on
  whether folding is semantically allowed; no single rewrite is universally
  safe.
- `octal-values` — Resolving `010` requires knowing whether the user meant
  the integer `8`, the integer `10`, or the string `"010"`; the YAML source
  alone cannot disambiguate.
- `truthy` — Rewriting `Yes/No/On/Off` requires choosing between quoting them
  (preserves the string), normalising to `true/false` (changes type), or
  rewording — all of which depend on the user's intent.

## Coverage Workflow

The CI enforces zero missed lines and zero missed regions. Use this workflow instead of
hunting through scattered tips:

1. First run `prek run --all-files` and rerun it until all automatic fixes have
   stabilised and a full pass succeeds without modifying files.
2. Quick status before pushing: run `./scripts/coverage-missing.sh` (Unix) or
   `pwsh ./scripts/coverage-missing.ps1` (Windows). It reruns the coverage suite and
   prints any uncovered ranges, or explicitly confirms when coverage is complete.
3. If the coverage script itself fails, run the relevant test suite manually first,
   fix the failing tests, then rerun the coverage script.
4. If the script reports files, extend CLI/system tests targeting those ranges until
   the script produces no output.
5. For richer artifacts (HTML, LCOV, etc.), follow the cargo-llvm-cov documentation
   after running the script. HTML is not easily machine readable though so not
   recommended.
6. When coverage points to tricky regions, prefer CLI/system tests in `tests/`
   that drive `env!("CARGO_BIN_EXE_ryl")` so you exercise the same paths as users.
7. When you need to observe the exact flow through an uncovered branch, run the
   failing test under `rust-lldb` (ships with the toolchain). Start with
   `cargo test --no-run` and then
   `rust-lldb target/debug/deps/<test-binary> -- <filter args>` to set breakpoints
   on the problematic lines.
8. If cached coverage lingers, clear `target/llvm-cov-target` and rerun.

## Code Size Workflow

After finishing feature work, use this order before committing:

1. Run `prek run --all-files` and rerun it until all automatic fixes have stabilised.
2. Run `./scripts/coverage-missing.sh` (Unix) or
   `pwsh ./scripts/coverage-missing.ps1` (Windows) and keep iterating until coverage
   is back to 100%.
3. If the coverage command fails for reasons unrelated to uncovered lines, run the
   affected tests manually, fix them, then rerun the coverage command.
4. Once lint, tests, and coverage are green, inspect code size with
   `uv run scripts/source_size.py --compare-to <branch-or-ref>`.
5. If the growth looks high for the functionality added, look for ways to reduce code
   size or make the implementation DRYer before committing.

### Coverage-Friendly Rust Idioms

- Guard invariants with `expect` (or an early `return Err(...)`) when the
  “else” branch is truly unreachable. Leaving a `return` in the unreachable path
  often shows up as a permanent uncovered region even though the condition is
  ruled out. Reserve `assert!` for test-only code or cases where a runtime panic
  is acceptable.
- When walking indices backwards, call `checked_sub(1).expect("…")` instead of
  matching on `checked_sub`; the `expect` documents the invariant and removes
  the uncovered `None` branch that instrumentation reports.
- When collecting spans, store the raw tuple `(start, end)` and filter once at
  the end instead of pushing `Range` conditionally; this keeps the guard logic
  centralized and ensures LLVM records the conversion branch exactly once.
- Normalize prefix checks with `strip_prefix(...).expect(...)` when downstream
  code already guarantees the prefix; this removes the otherwise uncovered
  `return` path that instrumentation would highlight.

Windows/MSVC: ensure the `llvm-tools-preview` component is installed (already listed in
`rust-toolchain.toml`). Run from a Developer Command Prompt if linker tools go missing.

### Common hotspots

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

CI will fail the build on any missed line or region, so keep local runs green by
sticking to the quick-status step above.

## Testing Tips

- For Unicode-heavy fixtures, assert behaviour with multibyte characters and reuse the
  helpers in `crate::rules::span_utils` instead of reinventing byte/char conversions.
  When writing tests, prefer inputs like `"café"` or `"å"` to ensure coverage of
  character vs byte offset logic.
- Use meaningful function and variable names in tests—comments are discouraged.
- `#[cfg(test)]` modules inside `src/` is forbidden; add coverage through integration
  tests in `tests/` so LLVM regions stay unique.
- The vendored SchemaStore yamllint snapshot lives at
  `tests/fixtures/schemastore-yamllint.json`; refresh it with
  `uv run scripts/update_yamllint_schemastore_snapshot.py` instead of fetching from
  the network in normal tests.
- The SchemaStore TOML projection comes from
  `uv run scripts/print_ryl_schemastore_schema.py`; it targets only
  `ryl.toml` / `.ryl.toml` because SchemaStore cannot attach directly to
  `[tool.ryl]` inside `pyproject.toml`.

## Release Checklist

- Bump versions in lockstep:
  - Cargo: update `Cargo.toml` `version`.
  - Python: update `pyproject.toml` `[project].version`.
  - NPM: update `package.json` `version`.
- Refresh lockfile and validate:
  - Run `cargo generate-lockfile` (or `cargo check`) to refresh `Cargo.lock`.
  - Stage: `git add Cargo.toml Cargo.lock pyproject.toml package.json`.
  - Run `prek run --all-files` (re-run if files were auto-fixed).
- Docs and notes:
  - Update README/AGENTS for behavior changes.
  - Summarize notable changes in the PR description or changelog (if present).
- Tag and push (when releasing):
  - `git tag -a vX.Y.Z -m "vX.Y.Z"`
  - `git push && git push --tags`
  - `.github/workflows/release.yml` validates that the pushed tag version
    matches `Cargo.toml`, `pyproject.toml`, and `package.json` versions
    before release jobs run.
- `.github/workflows/sync-schemastore.yml` projects `ryl.toml.schema.json`
  into SchemaStore's draft-07 format and updates the user's SchemaStore fork.
  The release workflow runs it after a successful release and prints a manual
  upstream PR handoff for `owenlamont/schemastore:ryl-schema-update`.
  - Publishing uses Trusted Publishing for all registries:
    - crates.io via GitHub OIDC (`rust-lang/crates-io-auth-action`)
    - PyPI via Trusted Publishing (`pypa/gh-action-pypi-publish`)
    - NPM via Trusted Publishing (`actions/setup-node` OIDC)
  - GitHub release creation is deferred until the end of the workflow, after
    crates.io, PyPI, and NPM publishing succeed.
  - GitHub release notes are generated automatically by GitHub when the release
    draft is created.
  - The workflow keeps GitHub releases as drafts until assets are uploaded and
    supports reruns by skipping crates.io/PyPI publish steps when that exact
    version already exists.

## Documentation Site

- The Zensical documentation source lives under `/docs/` with site
  configuration in `zensical.toml`. Built output goes to `/site/` (gitignored).
- Zensical is pinned via the `docs` dependency group in `pyproject.toml` and
  locked in `uv.lock`. Use the uv group commands so transitive deps stay in
  sync with the lockfile.
- Build the site: `uv run --group docs zensical build --clean`.
- Preview locally with live reload: `uv run --group docs zensical serve`.
- Bumping Zensical: edit the pin in `pyproject.toml`, run `uv lock`, then
  rebuild to confirm the new version still renders cleanly.

## CLI Behavior

- Accepts one or more inputs: files and/or directories.
- Directories: recursively scan `.yml`/`.yaml` files, honoring git ignore and
  git exclude; does not follow symlinks.
- Files: parsed as YAML even if the extension is not `.yml`/`.yaml`.
- Exit codes: `0` (ok/none), `1` (invalid YAML), `2` (usage error).
