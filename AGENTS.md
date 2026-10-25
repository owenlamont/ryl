# Coding Agent Instructions

## What This Tool Does

ryl lints and formats YAML files

## Project Structure

- **/src/** – All application code lives here.
- **/crates/ryl-yaml-parser/** – ryl's YAML parser, a fork of granit-parser 2.0.0; it
  must never depend on ryl. It follows these same standards and is held to 100% coverage
  across its generic instantiations; `tests/property_parser.rs` replaces fuzzing with stable
  proptest suites.
- **/src/lsp/** – the `ryl server` language server, behind the default-on `lsp` cargo
  feature; a thin protocol adapter over the engine.
- **/tests/** – Unit and integration tests.
- **/docs/** – Source content for the Zensical documentation site.
- **pyproject.toml** - Package configuration
- **zensical.toml** - Documentation site configuration
- **prek.toml** - Prek managed linters and some configuration

## Coding Standards

- Code maintainability is the top priority: a new agent should get all needed context
  from the docs and code with no surprising behaviour (the pit-of-success principle —
  the most likely way to do something is also the correct way).
- Before a feature request, check [`docs/scope.md`](docs/scope.md) and the public
  [design decisions](docs/design/decisions.md). Bring requests outside scope to the
  maintainer.
- Before implementing a new or changed rule — or any non-trivial feature — propose a
  short plan and agree the approach before writing code; don't jump straight to
  implementation.
- Separate judgment calls from mechanical work. Where a change turns on user-facing
  behaviour or a spec choice (what to flag, which YAML schema applies, a
  false-positive-vs-false-negative trade-off), lay out the options and let the maintainer
  decide. Carry out mechanical fixes and clear-cut review feedback without asking.
- With those options, give the precedent in ruff, rumdl, ty and biome — tested against
  the installed binaries where possible, not recalled. ryl models itself on them;
  yamllint is a migration target, so it decides only where migrating users would break.
- If you notice anything inaccurate or stale in this `AGENTS.md` or in a dev skill while
  working, fix it as part of the change rather than leaving it for later.
- Keep code as succinct as practical: every line has a maintenance and read-time cost,
  and every new conditional adds a testing burden. The best refactors remove lines while
  keeping functionality.
- Comments earn their place; default to deleting one. Code is the primary documentation:
  reach for a clearer name, type, or signature first (`missing_docs` is not enforced, so
  a self-documenting signature often needs no doc comment). A *why* comment is warranted
  only where the justification is not locally apparent: an unavoidable code smell (often
  third-party-imposed), a constraint at a distance, a non-obvious invariant, or
  "looks-wrong-but-isn't" logic that stops a later reader "fixing" it (a "verified
  against ruamel/PyYAML" note counts). Background, motivation, and history do not
  qualify, even for a rule's own rationale. Satisfy a clippy-mandated doc section
  (`# Errors`/`# Panics`) minimally. No issue/PR references, no historical narration.
- Lean on the linters and formatters — auto-fixing is far cheaper than correcting by
  hand, so only fix what they can't. prek won't scan a new module until it is `git
  add`ed, so stage new files first.
- Keep `Cargo.toml`'s `rust-version` in step with the channel in `rust-toolchain.toml`
  whenever the toolchain is bumped. Nothing checks it: a stale, lower `rust-version` still
  satisfies both clippy gates, so it fails silently.
- Don't rely on your memory of libraries and APIs; consult current documentation or the
  dependency's own repo.
- Verify behaviour against an authoritative source before asserting it, to the maintainer
  as much as in code: prefer the ryl CLI, the play.yaml.com reference parser, or a
  resolving loader over memory, and correct an earlier claim explicitly once it turns
  out wrong. Check tricky inputs with the ryl CLI so diagnostics and message text match
  the documented behaviour (e.g. mixed newline styles, or config keys of type
  int/bool/null/tagged scalar).
- For how YAML *itself* parses (is an input valid, what events does it produce?), the
  source of truth is the **Reference Parser** pane of the YAML Parser Playground
  <https://play.yaml.com/> — the test-suite event stream (`+STR/+DOC/+MAP/+SEQ`, `=VAL`,
  `=ALI`, `&anchor`, tags, or a parse error), drivable as
  `https://play.yaml.com/#<base64>` for scripted checks. It reports the *parse/event*
  layer, not *schema resolution* (`=VAL :011`, never "int vs string"); for type
  resolution (does `011` resolve to int 11, an empty scalar to null?) use a resolving
  loader (`ruamel.yaml` 1.2 mode or PyYAML), since ryl targets the YAML 1.2 **core**
  schema.
- When parsers disagree (e.g. ryl-yaml-parser vs PyYAML vs `ruamel.yaml`) and a rule's
  or the formatter's behaviour turns on it, **decide against the YAML 1.2.2 spec grammar
  and the play.yaml.com reference parser.** PyYAML and ruamel are non-conformant and
  never outvote it, even split 2–2; eemeli's `yaml` (near-conformant) is the
  implementation to cross-check. Follow the reference parser even when every
  implementation disagrees. Quote the spec production and event stream, and record any
  divergence (example + rationale) in the "How ryl differs from yamllint" catalog in
  `docs/getting-started/migrating-from-yamllint.md`.
- Keep YAML configuration aligned with what yamllint currently supports; put any
  ryl-only settings, experimental options, or ahead-of-upstream behaviour in TOML so
  future yamllint additions can't clash with YAML semantics. A whole ryl-only *rule*
  (e.g. `tags`) goes in `rules::RYL_ONLY_RULE_IDS` — the YAML path rejects it and
  `config_schema::yaml_schema` prunes it, so it's configurable only via TOML
  (`[lint.rules.<id>]`).

## Dev Skills

Task-scoped procedures live in `.agents/skills/`; load the matching skill when
needed. Each has a `SKILL.md`; `coverage` also has a `uv`-runnable helper.

Working on the codebase:

- `.agents/skills/adding-a-rule/SKILL.md` — the multi-site rule checklist and the parser
  event/span gotchas.
- `.agents/skills/lint-pipeline/SKILL.md` — inputs, source kinds, stdin, directives,
  Markdown embedding, `--fix`/`--diff`.
- `.agents/skills/config-discovery/SKILL.md` — config precedence, the candidate walk, and
  the explicit-opt-in exit-2 cases.
- `.agents/skills/output-formats/SKILL.md` — `check` output targets, the
  `[output]` table, JUnit/GitLab reports.
- `.agents/skills/lsp-server/SKILL.md` — `ryl server` diagnostics, code actions, and
  position encoding.
- `.agents/skills/payload-hardening/SKILL.md` — the invariants that contain a hostile
  YAML payload.

Verifying a change:

- `.agents/skills/property-tests/SKILL.md` — the property suites and the 10-minute
  pre-commit run.
- `.agents/skills/testing-traps/SKILL.md` — traps that make a test pass vacuously, plus
  regenerating committed schemas and snapshots.
- `.agents/skills/coverage/SKILL.md` — closing missed lines/regions for the CI gate.

Shipping and process:

- `.agents/skills/release/SKILL.md` — the lockstep version bump, tag/push gate, and
  publishing flow.
- `.agents/skills/winget-defender-fp/SKILL.md` — a winget-pkgs PR blocked by a Defender
  false positive.

Claude Code does not auto-load `.agents/skills/`, so this list is the cross-tool
fallback: any agent that reads `AGENTS.md` is pointed here, and even a skill-unaware
agent can just open the file. `skills/` (no dot) is reserved for published downstream
user skills; `.agents/skills/` is in-repo contributor tooling and is never published.

## Code Change Requirements

- Whenever any files are edited ensure all prek linters pass (run:
  `prek run --all-files`). prek already runs the key tooling (trim/fix whitespace,
  `cargo fmt`, `cargo clippy --fix`, `cargo clippy`, `rumdl` for Markdown/docs, …), so
  skip invoking those individually. Re-run until the auto-fixes stabilise and a full pass
  succeeds without modifying files.
- When editing **feature-gated** code (e.g. anything `#[cfg(feature = "lsp")]`), reproduce
  CI's two clippy gates locally: `cargo clippy --all-targets -- -D warnings` and
  `cargo clippy --all-targets --no-default-features -- -D warnings`. Prek enforces warnings
  with all features enabled; the minimal build also catches an `lsp`-only helper left
  unused when the feature is off.
- Whenever source files are edited, run the tests the change touches
  (`cargo nextest run --test <suite>`). CI runs the full suite on Linux and Windows
  plus coverage, so leave those to CI; `coverage-missing.py` reproduces its gap list.
- After lint and tests are green, review code size changes with
  `uv run scripts/source_size.py --compare-to <branch-or-ref>` (the branch point or
  `HEAD`); it reports bytes/lines plus a per-root code/doc/comment split via `tokei`. If
  the increase looks large relative to the added functionality, make the implementation
  DRYer or simplify it before committing. The `comment-ratio` prek hooks (also run in CI)
  gate `src` at `--max-comment-ratio 0.10` and `crates/ryl-yaml-parser/src` at `0.20`,
  where `missing_docs` requires every public item to be documented.
- For any behaviour or feature changes ensure all documentation is updated
  appropriately.

## Development Environment / Terminal

- This repo runs on Mac, Linux, and Windows. Don't make assumptions about the shell
  you're running on without checking first (it could be a Posix shell like Bash or
  Windows Powershell).
- `prek`, `rg`, `rumdl`, `typos`, `zizmor`, `cargo-audit`, `cargo-nextest`,
  `cargo-llvm-cov`, and `tokei` are expected on `PATH` as global tools; raise it with
  the user if one is missing.
  `cargo-audit` backs the `cargo audit --deny warnings` hook
  (`cargo install cargo-audit --locked`); `tokei` backs `comment-ratio`
  (`pixi global install tokei`, pinned separately for CI in `ci.yml` — bump both
  together). The `lychee` hook (an online link check of docs) installs itself from the
  `owenlamont/lychee-pre-commit` mirror; bump its `rev` in `prek.toml`.
- Node (CI pins 24.15.0) runs the YAML reference parser the formatter property suites
  check values against; fetch it with `git submodule update --init
  tests/yaml-reference-parser`.
- `gh` will be available in most, but not all environments to inspect GitHub.
- Linters/tests may write outside the workspace (e.g. `~/.cache/prek`); if sandboxed,
  request permission escalation for `prek`/`cargo test`/coverage. Allow ≥1-minute
  timeouts per invocation (more for larger runs/CI).
- Wait on long-running work (tests, coverage, CI, a Codex review) via the harness's
  background-task notifications or the Monitor tool: launch with `run_in_background` and
  act on the completion event. A hand-rolled `for i in $(seq …); sleep` poll loop burns
  turns and dead-polls a stalled job to its timeout instead of surfacing the failure.

## Automated Tests

- Deterministic tests own coverage: every covered region must have a named example test.
  CI disables random property cases; saved regression seeds replay separately from coverage.
  Local property runs keep their default counts (see below for new tests).
- Convey a test's purpose through function and variable names and assertion messages.
  Test comments meet the same bar as the rest of the codebase: keep one only for why an
  input is crafted a certain way (e.g. the `café` char-vs-byte column rationale), a
  non-obvious invariant a test pins, or a `//!` suite header describing reusable
  invariants.
- Aim for full branch coverage while minimising the test-to-src line ratio; a test that
  doesn't meaningfully increase coverage is a maintenance cost with no return.
- Do not add `#[cfg(test)]` test modules directly inside files under `src/`. Unit tests
  compiled alongside the library create duplicate LLVM coverage instantiations and break
  the "zero missed regions" guarantee enforced by CI. Add new coverage via CLI/system
  tests in `tests/` instead.
- When implementing a new rule or changing an existing one, extend the relevant
  property-test generator(s) so the new/updated syntax is actually exercised. Before
  committing, run every suite at its default count, plus a **20× run of each new
  `proptest!` function only** (`PROPTEST_CASES=<20 × its cases> cargo test --release
  --test <suite> <name> -- --exact`), under 10 minutes. Release gates run every suite at
  its default count. See the `property-tests` dev skill.
- The full corpus runs (output parity over every corpus repo and profile, and the
  preservation/idempotence check) take hours, so they are optional tools, never a PR or
  release gate. PRs and releases rely on the test and property suites plus
  `--fixtures-only` parity. The `property-tests` skill has the commands.
- Several traps make a test pass vacuously or fail only in CI — config discovery walking
  out of a tempdir, and CI's `GITHUB_ACTIONS` switching the output format. See the
  `testing-traps` dev skill before writing a test that drives the binary.

## Documentation Site

- Zensical docs source is under `/docs/` (config in `zensical.toml`); built output goes
  to `/site/` (gitignored). Zensical is pinned via the `docs` dependency group in
  `pyproject.toml`/`uv.lock` — use the uv group commands. Build: `uv run --group docs
  zensical build --clean`; preview: `uv run --group docs zensical serve`. To bump, edit
  the pin, run `uv lock`, and rebuild to confirm it renders.
- Config examples in `docs/` are validated through ryl's finalized config path by
  `tests/docs_config_examples.rs`, so a misspelled rule or option fails the build; the
  `testing-traps` skill has the recognition rules and the skip marker.

## CLI Behavior

`ryl check <inputs>` lints files, directories or `-` for stdin. Bare `ryl <inputs>`
still lints but is deprecated and warns. `check` exits `0` with no errors, `1` for
errors or invalid YAML, and `2` for usage/config errors or `--strict` warnings.
Without `--enable`, a missing config or one enabling no rules exits `2`.

Each surface — inputs and `--fix`/`--diff`, config discovery, output formats, the
language server — is documented in the matching dev skill above; user docs are in
`/docs/`.
