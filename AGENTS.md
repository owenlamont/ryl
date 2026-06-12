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

- Code maintainability is the top priority: a new agent should get all needed context
  from the docs and code with no surprising behaviour (the pit-of-success principle —
  the most likely way to do something is also the correct way).
- Before implementing a new or changed rule — or any non-trivial feature — propose a
  short plan and agree the approach before writing code; don't jump straight to
  implementation.
- Separate judgment calls from mechanical work. When a change turns on user-facing
  behaviour or a spec/standard choice (what to flag, which YAML schema applies, a
  false-positive-vs-false-negative trade-off), lay out the options and let the maintainer
  decide rather than picking silently. Carry out mechanical fixes and clear-cut review
  feedback without asking.
- If you notice anything inaccurate or stale in this `AGENTS.md` while working, fix it as
  part of the change rather than leaving it for later.
- Keep code as succinct as practical: every line has a maintenance and read-time cost,
  so prefer good naming over comments, and remember every new conditional adds a testing
  burden. The best refactors remove lines while keeping functionality.
- Comment the *why*, not the *what*: capture non-obvious invariants, spec rationale,
  verified-behaviour notes ("verified against ruamel/PyYAML"), deliberate
  trade-offs/workarounds, and version-pin reasons (link the issue) &mdash; plus
  "looks-wrong-but-isn't" reasoning that stops a later reader "fixing" subtle logic. This
  codebase is maintained across many AI sessions, so that durable *why*-context is
  usually worth its tokens; lean toward including it. Don't narrate self-evident
  *what* — good names carry that.
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
- Verify behaviour against an authoritative source before asserting it (to the
  maintainer as much as in code): prefer the ryl CLI, real `yamllint`, the play.yaml.com
  reference parser, or a resolving loader over memory; and if an earlier claim turns out
  wrong, correct the record explicitly.
- When mirroring yamllint behaviour, spot-check tricky inputs with the ryl CLI so
  our diagnostics and message text match (e.g., mixed newline styles or config keys of
  type int/bool/null/tagged scalar).
- For how YAML *itself* parses (is an input valid, what events does it produce?), the
  source of truth is the YAML Parser Playground <https://play.yaml.com/>: paste YAML and
  read the **Reference Parser** pane (the test-suite event stream — `+STR/+DOC/+MAP/+SEQ`,
  `=VAL`, `=ALI`, `&anchor`, tags, or a parse error). It can be driven by a base64 URL
  hash (`https://play.yaml.com/#<base64>`) for scripted checks. Caveat: it reports the
  *parse/event* layer, not *schema resolution* (`=VAL :011`, never "int vs string"); for
  type-resolution (does `011` resolve to int 11, an empty scalar to null?) use a
  resolving loader (`ruamel.yaml` 1.2 mode or PyYAML), since ryl targets the YAML 1.2
  **core** schema.
- When parsers disagree on an input (e.g. granit vs yamllint/PyYAML vs `ruamel.yaml`)
  and a rule's behaviour turns on the disagreement, **decide against the YAML 1.2.2
  specification grammar and the play.yaml.com reference parser — they rank above
  yamllint as the authority.** yamllint (PyYAML) is a compatibility target, not ground
  truth, and is sometimes non-conformant. Quote the relevant spec production and the
  reference-parser event stream when deciding, prefer the spec-correct behaviour, and
  record any deliberate yamllint divergence (with an example and the rationale) in the
  "How ryl differs from yamllint" catalog in
  `docs/getting-started/migrating-from-yamllint.md`.
- Keep YAML configuration aligned with what yamllint currently supports; put any
  ryl-only settings, experimental options, or ahead-of-upstream behaviour in TOML so
  future yamllint additions can't clash with YAML semantics. A whole ryl-only *rule*
  (e.g. `tags`) goes in `rules::RYL_ONLY_RULE_IDS` — the YAML path rejects it and
  `config_schema::yaml_schema` prunes it, so it's configurable only via TOML
  (`[rules.<id>]`).

## Adding a New Rule

A rule touches several disconnected sites; a missing one usually fails a guard test
rather than shipping silently. Work this checklist (*Automated Tests* expands steps 5–6):

1. **Rule module** `src/rules/<rule>.rs`: a `pub const ID`, a `Config` with
   `resolve(&YamlLintConfig)`, a `check(...) -> Vec<Violation>` (or `Option`), and a
   `Violation { line, column, message }`. Open with a `//!` header — one-line purpose,
   a `Sources:` line (spec / yamllint / authoritative refs), and the "no safe `--fix`"
   note where applicable. Prefer granit scanner/event tokens over char heuristics; if
   the rule tracks key/value position, advance the shared
   `support::mapping_key_walker::Walker` on *every* node event, including
   `Event::Alias` (`Walker::skip_node`), or key/value alternation desyncs.
2. **Register** in `src/rules/mod.rs`: `pub mod <rule>;` plus the `ID` in `ALL_RULE_IDS`
   — and in `RYL_ONLY_RULE_IDS` when yamllint has no equivalent (that reserves it to
   TOML config; see the YAML-vs-TOML note above).
3. **Dispatch**: one `lint_rule!(...)` call in `src/lint.rs`, in the right
   reported-order slot of the matching batch fn (`collect_layout` / `collect_value` /
   `collect_block_diagnostics`). Pick the arm matching the rule's shape (config or not,
   `Vec`/`Option`, per-violation or fixed `MESSAGE`).
4. **TOML config wiring** (`src/config_schema.rs` + `config_schema/serialization.rs`):
   a `RuleName` variant + `as_str` arm, a `RulesTable` field with its `…Options` type,
   and the `insert_serialized` line in `rules_table_to_value`. These four parallel lists
   have no compile-time cross-check; the `every_rule_round_trips_through_toml_serialization`
   guard test catches a forgotten serialization line. Regenerate the committed
   `ryl.{toml,yaml}.schema.json` (see *Testing Tips*) and run `prek`.
5. **Tests**: add the rule to `property_check`'s `collect_spans` + a `RULE_TRIGGERS`
   row; if it has a safe `--fix`, also `SAFE_FIX_RULES` and the safe-fix generator. Add
   a CLI test `tests/cli_<rule>_rule.rs` (use the shared `common::cli` harness) and an
   embedded-markdown regression test in `tests/cli_markdown_embed.rs`.
6. **Docs**: a `docs/rules/<rule>.md` page + the index, and a "How ryl differs from
   yamllint" entry for any deliberate divergence.

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
- `prek`, `rg`, `rumdl`, `typos`, `yamllint`, `zizmor`, and `cargo-audit` should be
  installed as global tools (if they don't appear to be installed raise that with the
  user). `cargo-audit` backs the `cargo audit --deny warnings` prek hook; install it with
  `cargo install cargo-audit --locked`.
- `gh` will be available in most, but not all environments to inspect GitHub.
- For PR feedback, use `gh pr view <n> --json comments,reviews` for summary threads and
  `gh api repos/<owner>/<repo>/pulls/<n>/comments` for inline review details (avoid
  unsupported flags like `--review-comments`).
- Codex reviews need an explicit `@codex review` comment; auto-review is unreliable
  (even on PR open), so post it again after each push you want reviewed. Within ~1 min
  Codex acks with a 👀 (`eyes`) reaction on the trigger comment; if it doesn't appear,
  re-comment. The verdict then arrives as one of: a new PR review, a new issue comment
  (often the "no major issues" all-clear), or a 👍 on the trigger comment — poll for
  **any** of them (a verdict can take 20+ min). Capture baseline counts of reviews,
  issue comments, and trigger-comment reactions, then poll (~45s) in a background
  command. The bot login is `chatgpt-codex-connector[bot]`; filter with
  `select(.user.login=="chatgpt-codex-connector[bot]")` (the bare login without `[bot]`
  matches nothing).
- When referencing another repository's issues/PRs in GitHub issues, PRs, or comments
  (e.g. an upstream `yamllint` issue), always use the fully-qualified
  `adrienverge/yamllint#123` form. A bare `#123` auto-links to *this* repo
  (`owenlamont/ryl#123`) and silently points at the wrong issue. Use a bare `#123` only
  for ryl's own issues/PRs.
- Filing an issue/PR on **another** project's repo (`granit`, upstream `yamllint`, …):
  don't open it directly — these go out under the maintainer's name. Build a
  self-contained draft for proof-read first, and only file once approved. Two
  non-negotiables: (1) **never report on an assumption** — verify every claim by
  *running that project itself* at the version in use, never inferred from its lineage,
  a sibling tool, docs, or the spec; (2) **ship a one-command reproduction** in its own
  `<repo>-<topic>-repro/` directory outside the ryl repo (ask where drafts live), pinned
  to the dependency's **latest** version, printing observed-vs-expected. No repro, no
  report. Keep it succinct: concrete ask, runnable repro, then authoritative evidence
  (spec quote / play.yaml.com event stream).
- Linters/tests may write outside the workspace (e.g. `~/.cache/prek`); if sandboxed,
  request permission escalation for `prek`/`cargo test`/coverage. Allow ≥1-minute
  timeouts per invocation (more for larger runs/CI).

## Automated Tests

- Convey a test's purpose with meaningful function and variable names, and convey
  what each check verifies with assertion messages. Comments in tests follow the same
  *why*-not-*what* bar as the rest of the codebase (see Coding Standards): a one-line
  note on the invariant a test pins, why an input is crafted a certain way (e.g. the
  `café` char-vs-byte column rationale), a regression's issue link, or a `//!` header
  describing a property suite's invariants and reuse is welcome where it adds durable
  context &mdash; just don't restate what a self-evident assertion already says.
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
  misses. Commit only once it is green, and keep any newly-persisted seeds in
  `tests/proptest-regressions/`.

### Property Tests For Safe Fixes

`tests/property_safe_fix.rs` runs generated YAML through `apply_safe_fixes` and asserts
three *soundness* invariants (a safe fix must never change meaning, but need not be
complete — so it does *not* assert "no diagnostics remain"): idempotence, parse
preservation (parses to an equal `YamlOwned`), and a leading `# ryl disable` making the
fix a byte-for-byte no-op. It runs a matrix of named configs — five YAML
(`yamllint-default`, `best-practice`, `strict-single`, `strict-double`, `consistent`)
plus one TOML-backed (`best-practice-toml`, covering ryl-only options like
`allow-double-quotes-for-escaping`). Deterministic siblings pin known-dirty /
production-bug inputs through the same checks (and assert the fixer clears them) so the
property can't pass vacuously.

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

`tests/property_check.rs` property-tests the **detection** path: it runs every rule's
`check()` over generated YAML and asserts oracle-free invariants — `check()` never
panics, every span is in-bounds and **character-aligned** (`1 <= line <= line_count`,
`1 <= column <= chars_on_line + 1`), a leading `# ryl disable` mutes every rule (only a
syntax error survives), and block-disabling a firing rule removes its diagnostics. It
targets ryl's fragile byte↔char offset arithmetic rather than semantic correctness (the
fast complement to the slow `yamllint_compat_*` differential suite).
`property_check/strategy.rs` generates documents biased to trigger every rule (truthy
words, octal/float scalars, duplicate/unordered keys, flow spacing, anchors, long lines,
odd indentation, trailing spaces) interleaved with multibyte chars, raw NEL/LS/PS, and
mixed LF/CRLF/bare-CR (a bare `\r` is a YAML 1.2 line break everywhere, so the oracle
`line_char_lengths` is CR-aware too). `harness.rs` holds the
trigger-all config and the per-rule dispatch, which calls each `check()` directly (not
`lint_str`, which drops rule spans on a parse error) so spans are bounds-checked even on
input that fails to parse.

When you add a new rule, extend `collect_spans` in `harness.rs` to call its
`check()` and add a `(rule-id, triggering-input)` row to `RULE_TRIGGERS` in
`property_check.rs`. The deterministic `each_rule_triggers_and_reports_in_bounds_spans`
test asserts each rule fires on its crafted input, so the property assertions
cannot silently pass vacuously if the generator drifts. Failing inputs persist
to the committed `tests/proptest-regressions/property_check.txt`. Run with
`cargo test --test property_check`.

### Property Tests For Markdown `--fix`

`tests/property_markdown_fix.rs` property-tests `fix::fix_markdown_str` (write-back into
embedded YAML). It reuses the safe-fix generator via `#[path]` and wraps the documents
into a Markdown host (`property_markdown_fix/wrap.rs`), asserting four oracle-free
invariants across the config matrix: host bytes outside regions stay byte-identical
(region count/kinds stable), each region's parsed value is preserved, each region is
untouched or rewritten to exactly its `apply_safe_fixes_filtered` form, and it's
idempotent. Deterministic siblings pin known-dirty / CRLF / ragged /
fence-crossing-front-matter cases.

Extend this suite only when the Markdown extractor/wrapper grows new region shapes:
add a `wrap.rs` variant and a deterministic sibling. Failing inputs persist to the
committed `tests/proptest-regressions/property_markdown_fix.txt`; run with
`cargo test --test property_markdown_fix`.

### Property Tests For Config Parsing

`tests/property_config.rs` property-tests **configuration robustness**:
`property_config/strategy.rs` generates randomized configs (random rule subsets, levels,
and options, mixing valid with hostile values — invalid regexes, ill-typed/out-of-range
scalars, bogus locales) rendered to both YAML and TOML. The oracle-free invariant: the
pipeline errors or succeeds but **never panics** — YAML via `YamlLintConfig::from_yaml_str`
(then linting samples, to drive the `.expect()`s in `key-ordering`/`quoted-strings`
`resolve()`), TOML via `parse_toml_config_str -> validate_toml_config ->
normalize_toml_config`. Deterministic siblings pin empty-config, invalid-regex,
billion-laughs, and rich-valid cases. When a rule gains a config-compiled regex or typed
option, add its key(s) to `CATALOG` in `strategy.rs`.

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
- `block-scalar-chomping` — YAML has no explicit *clip* indicator (only `-`
  strip and `+` keep exist), so a bare `|`/`>` cannot be annotated without
  switching it to strip or keep, which changes the scalar's trailing newlines
  and resolved value; the choice is the author's intent.
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
  subsequent lines and so can change the parsed structure; the `dash-on-own-line`
  option is likewise no-fix, since breaking the `-` onto its own line re-indents
  the mapping body.
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
- `merge-keys` — Removing a `<<` merge requires inlining the merged mapping's
  resolved keys/values (which the source text alone does not carry) and would
  change the document's structure; quoting the `<<` silently drops the merge, so
  no rewrite is universally safe.
- `octal-values` — Resolving `010` requires knowing whether the user meant
  the integer `8`, the integer `10`, or the string `"010"`; the YAML source
  alone cannot disambiguate.
- `tags` — Rewriting or removing a flagged tag changes the node's resolved
  type (`!!omap` to a plain mapping, `!env` to a string, …) or requires
  guessing the intended value, so no rewrite is universally safe.
- `truthy` — Rewriting `Yes/No/On/Off` requires choosing between quoting them
  (preserves the string), normalising to `true/false` (changes type), or
  rewording — all of which depend on the user's intent.
- `unicode-line-breaks` — The `\N`/`\L`/`\P` escape is valid only inside a
  double-quoted scalar; rewriting a raw NEL/LS/PS in a plain or single-quoted
  scalar, a comment, or a block scalar would require changing the quoting style
  or guessing intent, so no rewrite is universally safe.

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
5. For richer artifacts (HTML/LCOV), see the cargo-llvm-cov docs (HTML isn't easily
   machine-readable).
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
- When collecting spans, store the raw `(start, end)` and filter once at the end rather
  than pushing `Range` conditionally, so LLVM records the conversion branch once.
- Normalize prefix checks with `strip_prefix(...).expect(...)` when the prefix is already
  guaranteed; this removes the otherwise-uncovered `return` path.

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

## Testing Tips

- For Unicode-heavy fixtures, assert with multibyte characters (e.g. `"café"`/`"å"`) and
  reuse `crate::rules::span_utils` rather than reinventing byte/char conversions, to
  cover character-vs-byte offset logic.
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
- The committed `ryl.toml.schema.json` / `ryl.yaml.schema.json` are generated by
  `ryl --print-toml-config-schema` / `--print-yaml-config-schema`. **Always run `prek`
  after regenerating**: `--print` emits schemars *insertion* order, but the
  `pretty-format-json` hook rewrites JSON with recursively *sorted* keys, so the
  committed form is sorted (committing raw `--print` output causes the recurring "prek
  reordered the schema" churn). Because the canonical form is sorted, regeneration only
  changes lines when schema *content* changes — a structural change that doesn't alter
  content yields a zero diff after sorting, so don't commit a reordered file (leave it at
  `HEAD`). `tests/config_schema.rs` compares order-insensitively, but the files must
  still be committed sorted to keep `prek` idempotent; when an options type is renamed,
  update the `RuleEntryFor…`/`RuleOptionsFor…` names asserted there too.

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
- After a successful release, `.github/workflows/sync-schemastore.yml` projects
  `ryl.toml.schema.json` into SchemaStore's draft-07 format, updates the user's
  SchemaStore fork, and prints a manual upstream PR handoff for
  `owenlamont/schemastore:ryl-schema-update`.
- Publishing uses Trusted Publishing on all registries (crates.io via GitHub OIDC, PyPI
  via `pypa/gh-action-pypi-publish`, NPM via `actions/setup-node` OIDC). GitHub release
  creation is deferred until after crates.io/PyPI/NPM publishing succeeds, kept as a
  draft until assets upload, with auto-generated notes; reruns skip publish steps for a
  version that already exists.

## Documentation Site

- Zensical docs source is under `/docs/` (config in `zensical.toml`); built output goes
  to `/site/` (gitignored). Zensical is pinned via the `docs` dependency group in
  `pyproject.toml`/`uv.lock` — use the uv group commands. Build: `uv run --group docs
  zensical build --clean`; preview: `uv run --group docs zensical serve`. To bump, edit
  the pin, run `uv lock`, and rebuild to confirm it renders.

## CLI Behavior

- Accepts one or more inputs: files, directories, or `-` to read from stdin.
- Directories: recursively scanned, honoring git ignore and git exclude; does not
  follow symlinks. Each file's source kind is resolved from the `[files]` globs
  (TOML) or `yaml-files` (YAML); files matching no kind are skipped.
- Files named explicitly are linted as their resolved source kind; one that
  matches no `[files]` kind is rejected with an error (rather than silently
  treated as YAML).
- Inputs are de-duplicated: a file reached by two spellings (`ryl . f.yaml`, `f.yaml`
  twice, or `f.yaml sub/../f.yaml`) is processed once. `gather_lint_files` keys a `seen`
  set on `main::canonical_input` (`std::path::absolute` + lexical `..` normalization —
  purely lexical, no symlink resolution, so a symlink stays distinct from its target),
  spanning lint/`--fix`/`--diff`/`--list-files`. Stricter than yamllint (which keeps
  duplicates); for `--diff` a duplicate would emit a repeat patch block that fails to
  apply on the second copy.
- Source kinds (`config::SourceKind`): the `[files]` TOML table maps `yaml` and
  `markdown` to glob lists (`yaml` defaults to `*.yaml`/`*.yml`/`.yamllint`). A
  file matching two kinds is a hard error. `yaml-files` is rejected in TOML (use
  `[files].yaml`); it remains valid in the legacy YAML config.
- Markdown embedding (off by default; enabled by `[files].markdown` globs, or per-run
  via `--markdown` which injects default globs): ryl extracts front matter and fenced
  `yaml`/`yml` blocks (each linted as its own document) and maps diagnostics back to the
  Markdown file. The `[markdown]` `front-matter`/`fenced-blocks` booleans (default true)
  select sources. Extractor in `src/markdown_embed/` (fenced blocks via `pulldown-cmark`,
  front matter via a line scan); each `EmbeddedRegion` carries the `raw_span` and per-line
  column remap. `document-start`/`document-end`/`new-line-at-end-of-file`/`new-lines` are
  suppressed in regions via `fix::suppressed_rules(kind)`. `--fix` writes back
  (`fix::fix_markdown_str`): re-applies each line's stripped prefix (spaces, `> `, or a
  tab), preserves CRLF, and only rewrites a region when that reproduces the original
  bytes exactly — a ragged region (no single shared prefix) is reported but left
  untouched. See `docs/markdown.md`. A Markdown file with a bare `\r` (CR not in CRLF)
  anywhere is skipped loudly (`markdown_has_unsupported_cr` guards `lint_markdown_str`/
  `fix_markdown_str`/`markdown_parse_skips`: lint error + `--fix`/`--diff` notice):
  `pulldown-cmark` can't find fences in a `\r` host and the `\n`-based remap can't place
  a region `\r`. LF/CRLF embedded YAML is linted CR-aware.
- `--fix` never mutates a file that does not fully parse:
  `fix::apply_safe_fixes_filtered` gates the whole pipeline on `lint::parse_error`
  (stricter than lint's `syntax_diagnostic` — it does *not* tolerate undefined
  aliases), so *any* granit parse error ⇒ the input is returned byte-for-byte
  unchanged and `apply_safe_fixes_in_place` returns `FixOutcome::Skipped(problem)`; the
  CLI prints a `<path>:L:C skipped by --fix: <error>` notice. Lint behavior is
  unchanged: an undefined alias is still not a lint syntax error (the `anchors` rule
  reports it, matching yamllint); only `--fix` applies the stricter gate, through the
  in-place and per-region Markdown paths.
- `--diff` (#269) previews `--fix` without writing: prints a unified diff (3 lines of
  context) per changed file to **stdout** and exits `1` iff any file would change,
  mirroring `ruff check --diff`. `conflicts_with` `--fix`, ignores `--format`, supports
  stdin. Diff-only: remaining *unfixable* findings are neither printed nor counted (a
  file tripping only an unfixable rule exits `0`). Reuses the fix pipeline
  (`fix::diff_safe_fixes_for_files` → `fix::diff_outcome`), inheriting the parse-error
  gate and symlink skip (both → a `skipped by --diff` notice, no exit effect). A
  non-UTF-8/BOM input is likewise skipped (`fix::non_utf8_diff_skip`; files via
  `DecodedFile::is_plain_utf8`, stdin via decoded==raw bytes) — a text diff can't apply
  back to transcoded bytes, so `--fix` (which re-encodes) is the path for those — as is
  a filename with control characters (no representable header). Markdown diffs at
  host-file level. The diff *body* is verbatim (hk re-applies it byte-for-byte); the
  header path is sanitized and relativized to CWD (like ruff) so it applies via
  `git apply -p0`. A bare `\r` is rendered as diff *content* (`render_unified_diff`
  splits hunk lines on `\n` only), so a mid-line/mixed `\r` round-trips; content that
  *ends* in a bare `\r` is skipped (`fix::ends_in_bare_cr` — `similar` can't render it;
  use `--fix`).
- Malicious-payload hardening (#246) — invariants to preserve: `--fix`/`--diff` never
  write/read through a symlink (`fix::refuse_symlink`) and the write target is always
  the input path, never derived from YAML content. The YAML config loader
  (`yaml_dom::loader`; `lint_str` builds no DOM) bounds alias expansion at
  `MAX_EXPANDED_NODES` and `extends` depth at `MAX_EXTENDS_DEPTH`, so billion-laughs and
  cyclic-`extends` configs error instead of exhausting memory/stack. An empty
  YAML/TOML config errors ("not a mapping" / "configuration is empty") rather than
  silently linting nothing. Output is injection-safe: the GitHub format escapes user
  text (`github_escape_data`/`_property`) so a crafted key/anchor/filename can't
  inject a `::command::`; the other formats run user text through `sanitize_control`.
  granit caps nesting recursion (~256), and config regexes
  (`key-ordering`/`quoted-strings`) are validated at parse time with the linear-time
  `regex` crate (no ReDoS). Guards:
  `tests/cli_alias_bomb.rs`, `cli_fix_symlink.rs`, `cli_config_data_error.rs`,
  `cli_toml_config.rs`, `config_extends_inline.rs`, `cli_format_options.rs`,
  `cli_markdown_embed.rs`, `property_config.rs`.
- Stdin (`-`): bytes are read raw and decoded with the same BOM/encoding detection as
  files; `-` can't be combined with other inputs or with `--fix`. `--stdin-filename
  <PATH>` (ruff convention) sets the diagnostic label, anchors config discovery at the
  path's parent, resolves the source kind from `[files]` (a `markdown` path → embedded
  YAML), and runs `yaml-files`/per-file-ignore/per-rule `ignore` against it. Without it,
  diagnostics are `<stdin>`, config is anchored at CWD, and all path-based filtering is
  skipped so every enabled rule runs; `--markdown` forces Markdown.
- Inline directives (`src/directives.rs`): `# ryl disable` / `enable` / `disable-line`
  (and `# yamllint …` aliases) suppress rules for a block or line, mirroring yamllint's
  grammar (`yamllint/linter.py`); a first-line `# ryl/yamllint disable-file`
  (`directives::disables_file`) skips the whole file (no diagnostics, not even syntax
  errors, no `--fix`). Handling is global: `lint_str` filters every diagnostic through
  `Directives::is_disabled` before the syntax-error check, and `fix` reverts edits to
  disabled lines via `Directives::reconcile`. Works region-locally in embedded Markdown.
  Validate against yamllint with `tests/yamllint_compat_directives.rs`. User docs:
  `docs/directives.md`.
- ryl never enables a rule that wasn't explicitly turned on (no "default-on" rules). Two
  cases exit `2`, both stricter than yamllint: **no config found anywhere** (resolution
  falls back to an *empty* config — `ConfigContext::config_found == false` — not the
  `default` preset; reports `main::NO_CONFIG_ERROR`; yamllint lints with `extends:
  default`), and **a resolved config that enables no rules** (`rules: {}`, empty
  `[rules]`/`[tool.ryl]`, a `[files]`-only TOML config, or one disabling everything;
  reports `main::NO_RULES_ENABLED_ERROR`; yamllint silently lints nothing). Both via
  `YamlLintConfig::enables_any_rule`; `main::no_rules_error(config_found)` picks the
  message. The `default`/`relaxed`/`empty` presets stay available via `extends:` (YAML
  only). `--migrate-configs` (warns instead) and `--list-files` are exempt.
- Output formats (`--format`/`-f`): the streaming console formats `standard`/`colored`/
  `github`/`parsable` write per-diagnostic lines to **stderr** (unchanged); the
  whole-document report formats `junit` (JUnit XML via `quick-xml`) and `gitlab` (GitLab
  Code Quality JSON via `serde_json`) buffer every file and serialize once to **stdout**.
  `auto` never selects junit/gitlab. `process_results` (in `main`) does the shared
  filter+tally pass for all formats then either streams or hands a `Vec<ReportEntry>` to
  `ryl::report::render_junit`/`render_gitlab`. `-o/--output-file` redirects the selected
  format to a file (any format), `conflicts_with` `--diff`; `--format junit|gitlab` with
  `--diff` is rejected by `reject_diff_with_report_format`. An `--output-file` that
  lexically resolves to a linted input or the `--stdin-filename` is refused
  (`reject_output_file_collision`: lexical-path match, plus a `same_file::Handle`
  file-identity match for an existing destination so a symlinked or hard-linked `-o` is
  caught too) so the report cannot truncate the source — stricter than ruff, which guards
  nothing; other destinations (e.g. a config file) are overwritten as directed. An
  empty/all-ignored input set still emits a valid empty report (`emit_empty_report`:
  `[]` / `<testsuites .../>`) so CI artifact ingestion does not see a missing file.
  `report::ReportEntry` carries the report display path (relativized via
  `cli_support::report_display_path` against the project root =
  `CI_PROJECT_DIR` or cwd, like ruff; forward-slashed, no `./` prefix; a path outside the
  root keeps its absolute form), the kept problems, and an optional processing-error
  message. GitLab severity maps error->`major`, warning->`minor`, a read/parse
  failure->`blocker`; its `fingerprint` is a stable SHA-256 (`sha2`) of
  `(path, rule, message)` — deliberately NOT line/column, so an edit that shifts the line
  keeps the issue tracked — salted to stay unique within a report (`DefaultHasher` would
  not be stable across toolchains). A clean file is a passing JUnit testcase and is omitted
  from GitLab. Output is validated against authoritative sources in tests: GitLab against
  the vendored `tests/fixtures/gitlab-code-quality.schema.json` (via the `jsonschema`
  dev-dep), JUnit by re-parsing with `quick-xml`. See `docs/output-formats.md`. ryl follows
  ruff's model (single format + `--output-file`); it does not (yet) emit a report file and
  console output simultaneously.
- Exit codes: `0` (ok/none), `1` (invalid YAML), `2` (usage error).
