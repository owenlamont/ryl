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
  is Rust 1.96.0).
- Keep `clippy.toml` `msrv` in sync with `rust-toolchain.toml` whenever the Rust
  toolchain channel is changed.
- Don't rely on your memory of libraries and APIs. All external dependencies evolve fast
  so ensure current documentation and/or repo is consulted when working with third party
  dependencies.
- When mirroring yamllint behaviour, spot-check tricky inputs with the ryl CLI so
  our diagnostics and message text match (e.g., mixed newline styles or config keys of
  type int/bool/null/tagged scalar).
- For questions about how YAML *itself* should parse (is an input valid, and what
  event/structure does it produce?), the source of truth is the YAML Parser Playground
  at <https://play.yaml.com/>: paste YAML in the top-left pane and read the canonical
  **Reference Parser** output in the bottom-left pane (the test-suite event stream —
  `+STR/+DOC/+MAP/+SEQ`, `=VAL`, `=ALI`, `&anchor`, tags, or a parse error such as
  "Parser finished before end of input"). The other panes are alternative parsers that
  need a local sandbox server and are normally left unconnected. Input can be driven via
  the URL hash as base64 of the YAML (`https://play.yaml.com/#<base64>`), which is handy
  for scripted/browser-automation checks. Scope caveat: the playground reports the
  *parse/event* layer (validity and structure), not *schema resolution* — it shows
  `=VAL :011`, never "int vs string". For type-resolution questions (does `011` resolve
  to int 11, an empty scalar to null, etc.) use a resolving loader instead (e.g.
  `ruamel.yaml` in 1.2 mode or PyYAML), since ryl targets the YAML 1.2 **core** schema.
- Keep YAML configuration strictly aligned with functionality that yamllint currently
  supports. Put any ryl-only settings, experimental rule options, or ahead-of-upstream
  behaviour in TOML configuration so future yamllint additions cannot clash with
  existing YAML semantics. A whole ryl-only *rule* (one yamllint lacks, e.g. `tags`)
  goes in `rules::RYL_ONLY_RULE_IDS`: the YAML config path rejects those rules and
  `config_schema::yaml_schema` prunes them from the YAML schema, so they are
  configurable only via TOML (`[rules.<id>]`).

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
- Codex reviews must be triggered with an `@codex review` comment; do **not** rely on
  auto-review. Only the initial PR open *sometimes* auto-triggers one — every subsequent
  review, including after each push you want re-reviewed, must be prompted by commenting
  `@codex review` (it takes minutes). So after pushing changes for review, always post the
  comment rather than waiting for an auto-review that will not come. Confirm Codex picked
  it up: within ~1 minute of the `@codex review` comment Codex adds an 👀 reaction (`eyes`,
  from `chatgpt-codex-connector[bot]`) on the triggering comment to acknowledge it has
  started. If that 👀 has not appeared after ~1 minute, the trigger did not take —
  comment `@codex review` again, and re-check for the 👀. Once the review
  finishes, Codex removes the 👀 and signals its verdict in one of three forms — a new PR
  review (when it has findings), a new issue comment (often its "no major issues"
  all-clear), or a 👍 reaction on the triggering comment — so poll for **any** of them;
  watching only one misses the result. (Codex can be slow: a verdict occasionally takes
  20+ minutes even after the 👀, so keep polling past a short timeout rather than assuming
  it failed.) Capture baseline counts of Codex reviews, Codex issue comments, and the
  trigger comment's reactions, then poll (~45s) for any to change, running the poller
  as a background command since a thorough review can exceed a 10-minute foreground
  timeout.
  The bot login is `chatgpt-codex-connector[bot]` for PR reviews, inline review
  comments, issue comments, and reactions alike. Filter reviews/comments with
  `select(.user.login=="chatgpt-codex-connector[bot]")` (the bare
  `chatgpt-codex-connector` without the `[bot]` suffix matches nothing).
- When referencing another repository's issues/PRs in GitHub issues, PRs, or comments
  (e.g. an upstream `yamllint` issue), always use the fully-qualified
  `adrienverge/yamllint#123` form. A bare `#123` auto-links to *this* repo
  (`owenlamont/ryl#123`) and silently points at the wrong issue. Use a bare `#123` only
  for ryl's own issues/PRs.
- Linters and tests may write outside the workspace (e.g., `~/.cache/prek`). If
  sandboxed, request permission escalation when running `prek`, `cargo test`,
  or coverage commands.
- Allow at least a 1-minute timeout per linter/test invocation; increase as
  needed for larger runs or CI.

## Automated Tests

- Convey a test's purpose with meaningful function and variable names, and convey
  what each check verifies with assertion messages. Comments in tests follow the
  same bar as the rest of the codebase (see Coding Standards): keep them minimal and
  reserve them for genuinely non-obvious trade-offs, opaque mechanics, or
  module/harness orientation (e.g. a `//!` header describing a property suite's
  invariants and reuse) — never to narrate what a self-evident test already says.
- Every line of code has a maintenance cost, so don't add tests that don't meaningfully
  increase code coverage. Aim for full branch coverage but also minimise the tests code
  lines to src code lines ratio.
- Do not add `#[cfg(test)]` test modules directly inside files under `src/`. Unit tests
  compiled alongside the library create duplicate LLVM coverage instantiations and break
  the "zero missed regions" guarantee enforced by CI. Add new coverage via CLI/system
  tests in `tests/` instead.
- When implementing a new rule or changing an existing one, extend the relevant
  property-test generator(s) so the new/updated syntax is actually exercised (each suite
  below lists exactly what to extend and the deterministic guard to add), then do a
  one-off **~1000× thorough run** before committing: e.g.
  `PROPTEST_CASES=512000 cargo test --release --test property_check` (the suites'
  in-CI default is 512 cases — tuned for speed, not exhaustiveness). Build `--release`
  and run it in the background; it routinely flushes rare interleavings the small count
  misses (a 512k-case run on #252 surfaced a pre-existing alias/key-value desync bug in
  `key-ordering` and `quoted-strings`). Commit only once it is green, and keep any
  newly-persisted seeds in `tests/proptest-regressions/`.

### Property Tests For Safe Fixes

`tests/property_safe_fix.rs` runs `proptest`-generated YAML through `apply_safe_fixes`
and asserts four invariants: idempotence, no remaining safe-fix-rule diagnostics
after fixing, parse preservation (input that parses must produce output that
parses to an equal `YamlOwned` value), and that a leading `# ryl disable` makes the
fix a byte-for-byte no-op (the strongest form of "`--fix` never rewrites a disabled
line"). Deterministic sibling tests pin known-dirty
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

### Property Tests For Rule Checkers

`tests/property_check.rs` property-tests the **detection** path: it runs every
rule's `check()` (including the unfixable rules above) over generated YAML and
asserts two oracle-free invariants — `check()` never panics, and every reported
violation has an in-bounds, **character-aligned** span (`1 <= line <=
line_count`, `1 <= column <= chars_on_line + 1`). Two further invariants exercise
the directive engine through `lint_str`: a leading `# ryl disable` mutes every
rule (only a syntax error can survive), and block-disabling a rule that fires on
a document removes all of that rule's diagnostics. This is the fast,
yamllint-free complement to the slow `tests/yamllint_compat_*` differential
suite; it targets ryl's historically fragile byte<->char offset arithmetic
(issue #232) rather than semantic correctness.

Layout mirrors the safe-fix suite: `property_check/strategy.rs` generates
documents biased toward triggering every rule (truthy words, octal/float
scalars, duplicate/unordered keys, flow spacing, anchors, over-long lines, odd
indentation, trailing spaces) interleaved with multibyte characters and mixed
LF/CRLF endings (never a bare `\r`, so line counting always agrees with the
rules). `property_check/harness.rs` holds the trigger-all config (rule options
are tuned so each rule actually emits), the per-rule `check()` dispatch, and the
bounds invariant. The dispatch calls each `check()` directly rather than
`lint_str` on purpose: `lint_str` discards every rule's spans in favour of the
syntax error when a document fails to parse, but this suite must still bounds-check
the spans rules emit on input that fails to parse.

When you add a new rule, extend `collect_spans` in `harness.rs` to call its
`check()` and add a `(rule-id, triggering-input)` row to `RULE_TRIGGERS` in
`property_check.rs`. The deterministic `each_rule_triggers_and_reports_in_bounds_spans`
test asserts each rule fires on its crafted input, so the property assertions
cannot silently pass vacuously if the generator drifts. Failing inputs persist
to the committed `tests/proptest-regressions/property_check.txt`. Run with
`cargo test --test property_check`.

### Property Tests For Markdown `--fix`

`tests/property_markdown_fix.rs` property-tests `fix::fix_markdown_str` — the
write-back of safe fixes into YAML embedded in Markdown. It reuses the safe-fix
generator via `#[path]` (`property_safe_fix/{ast,strategy,config}.rs`), wraps the
generated `Document`s into a Markdown host (`property_markdown_fix/wrap.rs`), and
asserts four oracle-free invariants across the safe-fix config matrix: host bytes
outside regions stay byte-identical (with region count/kinds stable), each region's
parsed value is preserved, each region is either untouched or rewritten to exactly
its `apply_safe_fixes_filtered` form, and the operation is idempotent. Deterministic
siblings pin known-dirty / CRLF / ragged / fence-crossing-front-matter cases so the
random invariants cannot pass vacuously.

Extend this suite only when the Markdown extractor/wrapper grows new region shapes:
add a `wrap.rs` variant and a deterministic sibling. Failing inputs persist to the
committed `tests/proptest-regressions/property_markdown_fix.txt`; run with
`cargo test --test property_markdown_fix`.

### Property Tests For Config Parsing

`tests/property_config.rs` property-tests **configuration robustness** (issue #246
hardening): `property_config/strategy.rs` generates randomized configs — random
subsets of rules with random levels and options, mixing valid values with hostile
ones (invalid regexes, ill-typed/out-of-range scalars, bogus locales) — and renders
each model to both YAML and TOML. The oracle-free invariant is that the whole
pipeline errors or succeeds but **never panics**: YAML goes through
`YamlLintConfig::from_yaml_str` and, when it parses, lints sample documents (driving
the `.expect()` calls in `key-ordering`/`quoted-strings` `resolve()`); TOML goes
through `parse_toml_config_str -> validate_toml_config -> normalize_toml_config`.
Deterministic siblings pin the empty-config (YAML and TOML), invalid-regex,
billion-laughs, and rich-valid-config cases so the random invariant cannot pass
vacuously. When a rule gains a config-compiled regex or a new typed option, add its
real option key(s) to `CATALOG` in `strategy.rs`. Failing inputs persist to the
committed `tests/proptest-regressions/property_config.txt`; run with
`cargo test --test property_config`.

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
- `tags` — Rewriting or removing a flagged tag changes the node's resolved
  type (`!!omap` to a plain mapping, `!env` to a string, …) or requires
  guessing the intended value, so no rewrite is universally safe.
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
- Rules using the shared `crate::rules::support::mapping_key_walker::Walker` to track
  key/value position must advance it for *every* node-producing event, including
  `Event::Alias` (call `Walker::skip_node`). An alias in value position (`k: *a`, or a
  `<<: *base` merge) that does not advance the walker desyncs the key/value alternation,
  so the following key is read as a value and vice-versa (this caused a phantom-key bug
  in `key-ordering` and key/value misclassification in `quoted-strings`). Exercise rules
  with aliases in both key and value position.
- Resolving a scalar to its typed value (int/bool/null/float/string) is centralised in
  `crate::yaml_dom::scalar` (`resolve_scalar` / `resolve_plain_scalar`); reuse it rather
  than reinventing parsing. ryl resolves scalars per the YAML 1.2 **core** schema
  everywhere (leading-zero decimal is an int, an empty plain scalar is null, `0x`/`0o`
  radixes, full bool/null spelling sets); keep that schema choice consistent across rules
  instead of switching to JSON/1.1 semantics in any single rule.

CI will fail the build on any missed line or region, so keep local runs green by
sticking to the quick-status step above.

## Testing Tips

- For Unicode-heavy fixtures, assert behaviour with multibyte characters and reuse the
  helpers in `crate::rules::span_utils` instead of reinventing byte/char conversions.
  When writing tests, prefer inputs like `"café"` or `"å"` to ensure coverage of
  character vs byte offset logic.
- Lean on meaningful function/variable names and assertion messages to make tests
  self-documenting; add a comment only where it explains a non-obvious trade-off or
  opaque mechanic that names cannot (the standard minimal-comment bar applies).
- `#[cfg(test)]` modules inside `src/` is forbidden; add coverage through integration
  tests in `tests/` so LLVM regions stay unique.
- CLI/system tests that drive `env!("CARGO_BIN_EXE_ryl")` run under CI's environment,
  where `GITHUB_ACTIONS` makes ryl auto-select the GitHub output format
  (`::error file=…,line=L,col=C::L:C [rule] message`) rather than the standard format
  (`  L:C  level  message  (rule)`). Assert **format-agnostically**: match the bare
  `line:col` (present verbatim in both formats) and the **bare rule id**
  (`colons`, never `(colons)` — the GitHub format renders it `[colons]`). Do not
  force `--format` to dodge this and do not assert a specific format's `(rule)`
  parens or ANSI. The `cli_*_rule` tests follow this; only tests that exercise
  formatting itself (`cli_format_options`, `yamllint_compat_*`) pin or scrub the
  format via `--format`/`env_remove`.
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

- Accepts one or more inputs: files, directories, or `-` to read from stdin.
- Directories: recursively scanned, honoring git ignore and git exclude; does not
  follow symlinks. Each file's source kind is resolved from the `[files]` globs
  (TOML) or `yaml-files` (YAML); files matching no kind are skipped.
- Files named explicitly are linted as their resolved source kind; one that
  matches no `[files]` kind is rejected with an error (rather than silently
  treated as YAML).
- Source kinds (`config::SourceKind`): the `[files]` TOML table maps `yaml` and
  `markdown` to glob lists (`yaml` defaults to `*.yaml`/`*.yml`/`.yamllint`). A
  file matching two kinds is a hard error. `yaml-files` is rejected in TOML (use
  `[files].yaml`); it remains valid in the legacy YAML config.
- Markdown embedding (off by default; enabled by listing `[files].markdown`
  globs, or for one run with the `--markdown` flag which injects default markdown
  globs): ryl extracts front matter and fenced `yaml`/`yml` blocks (each linted as
  its own document), mapping diagnostics back to the Markdown file. The
  `[markdown]` table's `front-matter`/`fenced-blocks` booleans (default true)
  select sources. The extractor lives in `src/markdown_embed/` (fenced blocks via
  `pulldown-cmark`; front matter via a line scan); each `EmbeddedRegion` carries the
  raw source `raw_span` used by write-back and the per-line column remap.
  `document-start`, `document-end`, `new-line-at-end-of-file`, and `new-lines` are
  suppressed inside embedded regions via `fix::suppressed_rules(kind)` (shared by
  the check and fix paths). `--fix` writes safe fixes back into the Markdown
  (`fix::fix_markdown_str`): it re-applies each line's stripped prefix (spaces, a
  blockquote `> `, or a tab), preserves CRLF, and only rewrites a region when that
  reproduces the original bytes exactly (the reconstruct-and-verify guard) — a
  region whose lines lack a single shared prefix (ragged) is reported but left
  untouched, so write-back cannot corrupt a document. See `docs/markdown.md`.
- Malicious-payload hardening (issue #246): `--fix` never writes through a
  symlink — `fix::refuse_symlink` skips a symlinked input (still linted) with a
  warning, so an untrusted tree cannot redirect an in-place write outside itself.
  The write target is always the input path, never derived from YAML content. The
  YAML config loader (`yaml_dom::loader`, used only for YAML config — `lint_str`
  builds no DOM) bounds alias expansion at `MAX_EXPANDED_NODES`, rejecting
  billion-laughs configs (`-c`/`-d`/discovered `.yamllint`) instead of exhausting
  memory; cyclic `extends` is bounded by `MAX_EXTENDS_DEPTH` (was a stack overflow).
  An empty/whitespace/comment-only YAML config reports "not a mapping"; an empty
  TOML config (incl. an empty `[tool.ryl]`) reports "configuration is empty" instead
  of silently linting zero rules — neither panics. The markdown extractor derives
  each fenced block's line offset by binary search over precomputed newline
  positions (`markdown_embed::collect_fenced_blocks`), not by rescanning from the
  document start, so many embedded blocks stay linear rather than quadratic. Output
  is injection-safe: the GitHub format escapes user text (`github_escape_data`/
  `_property`, covering `::group::` and `file=`) so a crafted key/anchor/filename
  cannot inject a `::command::` in CI, and the standard/colored/parsable formats run
  user text through `sanitize_control` so control chars can't inject terminal escapes
  or split a diagnostic line. Regression guards: `tests/cli_alias_bomb.rs`,
  `tests/cli_fix_symlink.rs`, `tests/cli_config_data_error.rs`,
  `tests/cli_toml_config.rs`, `tests/config_extends_inline.rs`,
  `tests/cli_format_options.rs`, `cli_markdown_embed.rs`, and the randomized
  `tests/property_config.rs`. granit-parser itself caps nesting recursion (~256), so
  deep-nesting payloads are rejected before a deep DOM is built. Config-supplied
  regexes (`key-ordering`, `quoted-strings`) are validated at config-parse time and
  the `regex` crate is linear-time, so ReDoS is not reachable.
- Stdin (`-`): bytes are read raw and decoded with the same BOM/encoding
  detection as files. `-` cannot be combined with other inputs and is not
  compatible with `--fix`. `--stdin-filename <PATH>` (ruff convention) sets
  the diagnostic label, anchors project-config discovery at the given path's
  parent, resolves the source kind from `[files]` (so a `markdown`-matching path
  is linted as embedded YAML), and runs `yaml-files`, per-file-ignore, and per-rule
  `ignore` matching against that path. Omitting `--stdin-filename` labels
  diagnostics as `<stdin>`, anchors config discovery at CWD, and skips all
  path-based filtering (`yaml-files`, per-file-ignores, per-rule `ignore`) so every
  enabled rule runs against the input; pass `--markdown` to lint that input as
  Markdown rather than plain YAML.
- Inline directives (`src/directives.rs`): `# ryl disable` / `enable` /
  `disable-line` comments (and the `# yamllint …` compat aliases) suppress rules
  for a block or a single line, mirroring yamllint's exact grammar
  (`yamllint/linter.py`). A first-line `# ryl/yamllint disable-file`
  (`directives::disables_file`) skips the whole file &mdash; no diagnostics, not
  even syntax errors, and no `--fix`. Handling is global, not per-rule: `lint::lint_str`
  filters every diagnostic through `Directives::is_disabled` before the
  syntax-error check, and `fix` reverts a fixer's edits to disabled lines via
  `Directives::reconcile`. Directives work region-locally inside embedded
  Markdown. Match yamllint's semantics exactly (validate with
  `tests/yamllint_compat_directives.rs`); the bare rule ids live in
  `rules::ALL_RULE_IDS`. User docs: `docs/directives.md`.
- ryl never enables a rule that wasn't explicitly turned on — there are no
  "default-on" rules. Two cases converge on this, both rejected loudly by the lint
  commands (exit 2) and both intentionally stricter than yamllint:
  - **No config found anywhere** (no `-c`/`-d`, no `YAMLLINT_CONFIG_FILE`, no
    discovered project/user-global config): resolution falls back to an *empty*
    config (`config::ConfigContext::config_found == false`) rather than the `default`
    preset, and the lint paths report `main::NO_CONFIG_ERROR`. yamllint instead lints
    with `extends: default`.
  - **A resolved config that enables no rules** (`rules: {}`, an empty
    `[rules]`/`[tool.ryl]`, a `[files]`-only TOML config, or one disabling
    everything): reported as `main::NO_RULES_ENABLED_ERROR`. yamllint silently lints
    nothing.
  Both checks live in the lint entrypoints via `YamlLintConfig::enables_any_rule`;
  `main::no_rules_error(config_found)` picks the message. `config_found` flows from
  `ConfigContext` through `resolve_ctx`/`gather_lint_files` (run-level) and
  `resolve_stdin_ctx`. The `default`/`relaxed`/`empty` presets stay available as
  explicit opt-ins via `extends:` (YAML config only). `--migrate-configs` (converts
  configs, does not lint — it warns when a migrated config enables no rules) and
  `--list-files` (a file query) are exempt; only the actual lint paths enforce it.
- Exit codes: `0` (ok/none), `1` (invalid YAML), `2` (usage error).
