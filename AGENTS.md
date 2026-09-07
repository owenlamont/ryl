# Coding Agent Instructions

## What This Tool Does

ryl is a CLI tool for linting yaml files

## Project Structure

- **/src/** – All application code lives here.
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
- Before implementing a new or changed rule — or any non-trivial feature — propose a
  short plan and agree the approach before writing code; don't jump straight to
  implementation.
- Separate judgment calls from mechanical work. Where a change turns on user-facing
  behaviour or a spec choice (what to flag, which YAML schema applies, a
  false-positive-vs-false-negative trade-off), lay out the options and let the maintainer
  decide. Carry out mechanical fixes and clear-cut review feedback without asking.
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
  as much as in code: prefer the ryl CLI, real `yamllint`, the play.yaml.com reference
  parser, or a resolving loader over memory, and correct an earlier claim explicitly once
  it turns out wrong. When mirroring yamllint, spot-check tricky inputs with the ryl CLI
  so diagnostics and message text match (e.g. mixed newline styles, or config keys of
  type int/bool/null/tagged scalar).
- For how YAML *itself* parses (is an input valid, what events does it produce?), the
  source of truth is the **Reference Parser** pane of the YAML Parser Playground
  <https://play.yaml.com/> — the test-suite event stream (`+STR/+DOC/+MAP/+SEQ`, `=VAL`,
  `=ALI`, `&anchor`, tags, or a parse error), drivable as
  `https://play.yaml.com/#<base64>` for scripted checks. It reports the *parse/event*
  layer, not *schema resolution* (`=VAL :011`, never "int vs string"); for type
  resolution (does `011` resolve to int 11, an empty scalar to null?) use a resolving
  loader (`ruamel.yaml` 1.2 mode or PyYAML), since ryl targets the YAML 1.2 **core**
  schema.
- When parsers disagree (e.g. granit vs yamllint/PyYAML vs `ruamel.yaml`) and a rule's
  behaviour turns on it, **decide against the YAML 1.2.2 specification grammar and the
  play.yaml.com reference parser — they rank above yamllint as the authority.** yamllint
  (PyYAML) is a compatibility target, not ground truth, and is sometimes non-conformant.
  Quote the spec production and the reference-parser event stream when deciding, prefer
  the spec-correct behaviour, and record any deliberate divergence (example + rationale)
  in the "How ryl differs from yamllint" catalog in
  `docs/getting-started/migrating-from-yamllint.md`.
- Keep YAML configuration aligned with what yamllint currently supports; put any
  ryl-only settings, experimental options, or ahead-of-upstream behaviour in TOML so
  future yamllint additions can't clash with YAML semantics. A whole ryl-only *rule*
  (e.g. `tags`) goes in `rules::RYL_ONLY_RULE_IDS` — the YAML path rejects it and
  `config_schema::yaml_schema` prunes it, so it's configurable only via TOML
  (`[rules.<id>]`).

## Dev Skills

Task-scoped procedures and reference material live as on-demand skills in
`.agents/skills/` (the shared project-scope skills dir most non-Claude agents auto-load);
load the matching one when its task comes up rather than carrying it in this always-on
file. Each is a self-contained `SKILL.md`; `coverage` also carries a `uv`-runnable
helper script.

Working on the codebase:

- `.agents/skills/adding-a-rule/SKILL.md` — the multi-site rule checklist and the granit
  event/span gotchas.
- `.agents/skills/lint-pipeline/SKILL.md` — inputs, source kinds, stdin, directives,
  Markdown embedding, `--fix`/`--diff`.
- `.agents/skills/config-discovery/SKILL.md` — config precedence, the candidate walk, and
  the explicit-opt-in exit-2 cases.
- `.agents/skills/output-formats/SKILL.md` — `--format`/`--output-file` targets, the
  `[output]` table, JUnit/GitLab reports.
- `.agents/skills/lsp-server/SKILL.md` — `ryl server` diagnostics, code actions, and
  position encoding.
- `.agents/skills/payload-hardening/SKILL.md` — the invariants that contain a hostile
  YAML payload.

Verifying a change:

- `.agents/skills/property-tests/SKILL.md` — the four property suites and the ~1000×
  pre-commit run.
- `.agents/skills/testing-traps/SKILL.md` — traps that make a test pass vacuously, plus
  regenerating committed schemas and snapshots.
- `.agents/skills/coverage/SKILL.md` — closing missed lines/regions for the CI gate.

Shipping and process:

- `.agents/skills/release/SKILL.md` — the lockstep version bump, tag/push gate, and
  publishing flow.
- `.agents/skills/filing-issues/SKILL.md` — filing issues and PRs here and on other
  people's repos.
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
  succeeds without modifying files before running coverage.
- When editing **feature-gated** code (e.g. anything `#[cfg(feature = "lsp")]`), reproduce
  CI's two clippy gates locally with `-D warnings` (prek's clippy does not, so it misses
  these): `cargo clippy --all-targets -- -D warnings` and `cargo clippy --all-targets
  --no-default-features -- -D warnings`. The `-D warnings` is what promotes a `dead_code`
  warning to an error — e.g. an `lsp`-only helper with no caller once the feature is off
  fails the minimal build, which a plain `cargo clippy` run shows only as a warning and
  silently passes.
- Whenever source files are edited ensure the full test suite passes (run
  `uv run .agents/skills/coverage/coverage-missing.py` to regenerate coverage; it
  reports uncovered ranges and confirms when coverage is complete).
- After lint, tests, and coverage are green, review code size changes with
  `uv run scripts/source_size.py --compare-to <branch-or-ref>` (the branch point or
  `HEAD`); it reports bytes/lines plus a per-root code/doc/comment split via `tokei`. If
  the increase looks large relative to the added functionality, make the implementation
  DRYer or simplify it before committing. The `comment-ratio` prek hook (also run in CI)
  gates `src` at `--max-comment-ratio 0.10`, so comments cannot outgrow code.
- For any behaviour or feature changes ensure all documentation is updated
  appropriately.

## Development Environment / Terminal

- This repo runs on Mac, Linux, and Windows. Don't make assumptions about the shell
  you're running on without checking first (it could be a Posix shell like Bash or
  Windows Powershell).
- `prek`, `rg`, `rumdl`, `typos`, `yamllint`, `zizmor`, `cargo-audit`, and `tokei` are
  expected on `PATH` as global tools; raise it with the user if one is missing.
  `cargo-audit` backs the `cargo audit --deny warnings` hook
  (`cargo install cargo-audit --locked`); `tokei` backs `comment-ratio`
  (`pixi global install tokei`, pinned separately for CI in `ci.yml` — bump both
  together). The `lychee` hook (an online link check of docs) installs itself from the
  `owenlamont/lychee-pre-commit` mirror; bump its `rev` in `prek.toml`.
- `gh` will be available in most, but not all environments to inspect GitHub.
- Linters/tests may write outside the workspace (e.g. `~/.cache/prek`); if sandboxed,
  request permission escalation for `prek`/`cargo test`/coverage. Allow ≥1-minute
  timeouts per invocation (more for larger runs/CI).
- Wait on long-running work (tests, coverage, CI, a Codex review) via the harness's
  background-task notifications or the Monitor tool: launch with `run_in_background` and
  act on the completion event. A hand-rolled `for i in $(seq …); sleep` poll loop burns
  turns and dead-polls a stalled job to its timeout instead of surfacing the failure.

## Automated Tests

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
  property-test generator(s) so the new/updated syntax is actually exercised, then do a
  one-off **~1000× thorough run** before committing (e.g.
  `PROPTEST_CASES=512000 cargo test --release --test property_check`, built `--release`
  in the background). See the `property-tests` dev skill.
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

`ryl check <inputs>` (the lint subcommand) and bare `ryl <inputs>` lint identically;
`check` is the recommended form and bare is being phased out. Inputs are files,
directories, or `-` for stdin. Exit codes: `0` (ok/none), `1` (invalid YAML), `2` (usage
error). ryl never enables a rule that wasn't explicitly turned on, so a run with no
config, or one enabling nothing, exits `2`.

Each surface — inputs and `--fix`/`--diff`, config discovery, output formats, the
language server — is documented in the matching dev skill above; user docs are in
`/docs/`.
