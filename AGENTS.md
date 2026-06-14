# Coding Agent Instructions

Guidance on how to navigate and modify this codebase.

## What This Tool Does

ryl is a CLI tool for linting yaml files

## Project Structure

- **/src/** – All application code lives here.
- **/src/lsp/** – the `ryl server` language server (LSP), behind the default-on `lsp`
  cargo feature; a thin protocol adapter (`lsp-server`+`lsp-types`) over the existing
  engine. See [CLI Behavior](#cli-behavior).
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

## Dev Skills

Task-scoped procedures live as on-demand skills in `.agents/skills/` (the shared
project-scope skills dir most non-Claude agents auto-load); load the matching one when
its task comes up rather than carrying it in this always-on file. Each is a
self-contained `SKILL.md`; the `coverage`, `codex-review-watch`, and `retrospective`
skills also carry a `uv`-runnable helper script.

- **`adding-a-rule`** (`.agents/skills/adding-a-rule/SKILL.md`) — the multi-site
  checklist for adding a new lint rule or changing an existing rule's wiring.
- **`property-tests`** (`.agents/skills/property-tests/SKILL.md`) — extending the
  safe-fix / rule-checker / markdown-fix / config property suites, the ~1000× pre-commit
  run, and the rules that intentionally have no safe `--fix`.
- **`coverage`** (`.agents/skills/coverage/SKILL.md`) — closing missed lines/regions for
  the CI gate, the `coverage-missing.py` workflow, and coverage-friendly Rust idioms.
- **`release`** (`.agents/skills/release/SKILL.md`) — the lockstep version bump,
  tag/push gate, and post-release SchemaStore + publishing flow.
- **`codex-review-watch`** (`.agents/skills/codex-review-watch/SKILL.md`) — trigger and
  monitor a Codex CI review on a PR and classify the verdict.
- **`filing-issues`** (`.agents/skills/filing-issues/SKILL.md`) — filing/editing issues
  and PRs for ryl or another repo: the cross-repo `#NNN` footgun, the draft-locally gate,
  the never-on-an-assumption rule, and verified Sources.
- **`retrospective`** (`.agents/skills/retrospective/SKILL.md`) — quantify development
  friction across recent sessions with a deterministic transcript extractor
  (`retro-extract.py`) feeding a small classifier-agent fan-out.

Claude Code does not auto-load `.agents/skills/`, so this list is the cross-tool
fallback: any agent that reads `AGENTS.md` is pointed here, and even a skill-unaware
agent can just open the file. `skills/` (no dot) is reserved for published downstream
user skills; `.agents/skills/` is in-repo contributor tooling and is never published.

## Code Change Requirements

- Whenever any files are edited ensure all prek linters pass (run:
  `prek run --all-files`).
- `prek` already runs the key tooling (e.g., trim/fix whitespace, `cargo fmt`,
  `cargo clippy --fix`, `cargo clippy`, `rumdl` for Markdown/docs, etc.), so skip
  invoking those individually. Re-run `prek run --all-files` until the auto-fixes
  stabilise and a full pass succeeds without modifying files before running coverage.
- Whenever source files are edited ensure the full test suite passes (run
  `uv run .agents/skills/coverage/coverage-missing.py` to regenerate coverage; it
  reports uncovered ranges and confirms when coverage is complete). See the `coverage`
  dev skill for the full workflow.
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
- Codex reviews need an explicit `@codex review` comment (auto-review is unreliable
  even on PR open, so re-post it after each push you want reviewed). The polling
  mechanics — the transient 👀 ack, the three verdict channels, the REST-not-GraphQL
  bot-login gotcha — are encoded in the `codex-review-watch` dev skill
  (`.agents/skills/codex-review-watch/`); run its `watch.py` rather than re-deriving the
  poll.
- Codex's adversarial review escalates indefinitely on file I/O (TOCTOU, partial/
  interrupted writes, cross-file non-atomicity). Converge on real bugs, document the
  rest as known limitations, and ship; do not re-introduce atomic temp+rename via a
  runtime `tempfile` dep (tried on #285, reverted: 0600-perms regression + `clippy::cargo
  multiple_crate_versions`). ryl's threat model (#246) is realistic payloads, not a
  concurrent fs racer.
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
- Wait on long-running work (tests, coverage, CI, a Codex review) via the harness's
  background-task notifications or the Monitor tool: launch it with `run_in_background`
  and act on the completion event. Don't hand-roll `for i in $(seq …); sleep` poll loops
  — they burn turns, and a stalled or broken job dead-polls to its timeout instead of
  surfacing the failure (see the `codex-review-watch` skill for the Codex-specific poll).

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
  property-test generator(s) so the new/updated syntax is actually exercised, then do a
  one-off **~1000× thorough run** before committing (e.g.
  `PROPTEST_CASES=512000 cargo test --release --test property_check`, built `--release`
  in the background). The `property-tests` dev skill
  (`.agents/skills/property-tests/SKILL.md`) details each suite (safe-fix / rule-checker
  / markdown-fix / config), exactly what each generator must be extended with, the
  deterministic guard to add, and the rules that intentionally have no safe `--fix`.

## Testing Tips

- For Unicode-heavy fixtures, assert with multibyte characters (e.g. `"café"`/`"å"`) and
  reuse `crate::rules::span_utils` rather than reinventing byte/char conversions, to
  cover character-vs-byte offset logic.
- Lean on meaningful function/variable names and assertion messages to make tests
  self-documenting; add a comment only where it explains a non-obvious trade-off or
  opaque mechanic that names cannot (the standard minimal-comment bar applies).
- `#[cfg(test)]` modules inside `src/` is forbidden; add coverage through integration
  tests in `tests/` so LLVM regions stay unique.
- **Config-discovery isolation.** ryl's project-config discovery climbs from each input
  through its ancestors up to `HOME`, so a test whose inputs live under the system temp
  dir can walk into that shared dir and discover a stray `ryl.toml`/`.ryl.toml`/
  `pyproject.toml`/`.yamllint*` left by another test, a concurrent process, or a manual
  smoke run — silently overriding the test's setup (and a TOML candidate outranks a
  tempdir's `.yamllint`, so an adjacent YAML config does not shield it). Any test that
  exercises discovery (does **not** pass `-c`/`-d`, and has no adjacent TOML config in its
  input's directory) must build its command via `common::cli::ryl(<its tempdir>)`, which
  sets `HOME` to bound the walk at the tempdir. Tests that pass `-c`/`-d` bypass discovery
  and need no isolation; tests that write an adjacent `.ryl.toml` are already shielded.
  Correspondingly, manual/agent smoke runs must keep scratch configs in a dedicated
  subdirectory and never drop a config-candidate-named file at the temp root.
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
  inject a `::command::` (it is a line-oriented command protocol); the streaming
  console formats run user text through `sanitize_control`. The `junit`/`gitlab`
  report formats are structured data, not command protocols, so the analogous risk is
  breaking out of an XML attribute / JSON string: `sanitize_control` first strips
  control chars, then `quick-xml` (XML) and `serde_json` (JSON) apply structural
  escaping, and fixed fields (`severity`, `check_name`, the testcase `name`) are
  derived from the rule/level, not the message. `tests/property_report.rs` fuzzes this
  (every output must stay well-formed XML / schema-valid JSON under hostile input).
  granit caps nesting recursion (~256), and config regexes
  (`key-ordering`/`quoted-strings`) are validated at parse time with the linear-time
  `regex` crate (no ReDoS). Guards:
  `tests/cli_alias_bomb.rs`, `cli_fix_symlink.rs`, `cli_config_data_error.rs`,
  `cli_toml_config.rs`, `config_extends_inline.rs`, `cli_format_options.rs`,
  `cli_markdown_embed.rs`, `property_config.rs`, `report_formats.rs`,
  `property_report.rs`.
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
  `github`/`parsable` default to **stderr**; the whole-document report formats `junit`
  (JUnit XML via `quick-xml`) and `gitlab` (GitLab Code Quality JSON via `serde_json`)
  default to **stdout**. `auto` never selects junit/gitlab. **Multiple outputs per run**
  (RuboCop/Biome model): `--format` is repeatable and each `-o/--output-file` binds to
  the most recent `--format` (`resolve_cli_targets` recovers CLI order via
  `ArgMatches::indices_of`, so `main` uses `Cli::command().get_matches()` +
  `from_arg_matches`); `-o -` is stdout, a path is a file, none is the format's default
  stream. Console + a report file in one run is therefore supported (closes #285's
  original ask), e.g. `--format auto --format gitlab -o gl.json`. An `[output]` **TOML
  table** (ryl-only, TOML-only — `config_schema::OutputTable`/`OutputDestination`,
  rejected in YAML config) configures the same per-format destinations
  (`[output.gitlab] path=…`; absent `path` = default stream, `"-"` = stdout). Precedence
  **CLI > config > default**: `resolve_targets` returns the CLI pairs if any `--format`
  was given, else `config_targets_from_table` of the run config's `[output]`, else one
  default auto-console target. The `[output]` is read run-level by `run_output_config`
  (the `-c`/`-d`/env global config, else the inputs-anchored project config so `ryl .`
  honors a project `.ryl.toml`; a malformed config is propagated — the empty-input case
  has no per-file discovery to surface it, so an invalid `[output]` still errors). `--diff`
  skips config `[output]` (it has its own unified-diff output), so only an explicit CLI
  `--format junit|gitlab` conflicts with it. Pipeline: `collect_records` does the shared
  filter+tally once into format-agnostic `FileRecord{path,kept,error}`; `write_targets`
  renders each target via `render_target` (`render_streaming` + an `append_*` fn for
  console formats; `render_junit`/`render_gitlab` over `build_entries` for reports, built
  once and shared) and `commit`s to each `open_destination` (a file is opened create+write
  **without** truncate, then truncated+written at commit, so an *existing* artifact survives
  a later target failing to open; a *freshly*-created destination may be left empty on a
  rejected run — cleaning it by path would race a concurrent writer, so it is left for the
  failed run, gate CI artifact use on the exit code). `open_targets` opens all destinations
  before `--fix` mutates (unopenable `-o` fails fast). Guards (each exit 2):
  `resolve_cli_targets` rejects an unpaired `-o` and a second `-o` on one `--format`;
  `validate_targets` rejects `--diff` with a report format and two outputs on one stream
  (`reject_duplicate_streams`, ≤1 stdout / ≤1 stderr); `open_targets` then rejects two
  outputs resolving to one file (`reject_colliding_output_files`, post-open so file
  identity resolves symlink/hard-link/aliased-parent destinations — `PathIdentity` =
  lexical + `same_file::Handle`; an unreadable existing destination matches lexically only,
  an adversarial case); `reject_input_collisions` refuses an output that is also a linted
  input or the `--stdin-filename` (same lexical + `same_file::Handle` match), so a report
  can never truncate the source. `--output-file` `conflicts_with` `--diff` in clap. An
  empty/all-ignored input set still emits a valid empty report per target (`emit_targets`
  with empty records → `[]` / `<testsuites .../>`). `report::ReportEntry`
  carries the report display path (relativized via `cli_support::report_display_path`
  against the project root = `CI_PROJECT_DIR` or cwd, like ruff; forward-slashed, no `./`
  prefix; a path outside the root gets `..` segments), the kept problems, and an optional
  processing-error message. GitLab severity maps error->`major`, warning->`minor`, a
  read/parse failure->`blocker`; its `fingerprint` is a stable SHA-256 (`sha2`) of
  `(path, rule, message)` — deliberately NOT line/column, so an edit that shifts the line
  keeps the issue tracked — salted to stay unique within a report (`DefaultHasher` would
  not be stable across toolchains). A clean file is a passing JUnit testcase and is
  omitted from GitLab. Output is validated against authoritative sources in tests: GitLab
  against the vendored `tests/fixtures/gitlab-code-quality.schema.json` (via the
  `jsonschema` dev-dep), JUnit by re-parsing with `quick-xml`. See
  `docs/output-formats.md`.
- Language server (`ryl server`, `src/lsp/`, behind the default-on `lsp` feature): a
  synchronous `lsp-server`+`lsp-types` adapter over the engine. `serve(&Connection)`
  runs the handshake + message loop; `run()` wires stdio and drops the connection
  before `io_threads.join()` so the writer thread finishes. It reuses
  `lint_str`/`lint_markdown_str` for diagnostics and `apply_safe_fixes`/`fix_markdown_str`
  for `source.fixAll.ryl` + `textDocument/formatting` (whole-file/per-rule fixes only —
  the engine has no per-occurrence fix; the fix-all action honours `context.only`). Config
  is resolved per document via `discover_config` (full CLI precedence incl.
  `YAMLLINT_CONFIG_FILE`); a rule-less/absent config lints nothing silently, a malformed
  one lints nothing but is surfaced once via `window/showMessage` (no hard exit-2).
  **Position encoding is the one
  load-bearing new piece:** LSP columns are UTF-16 code units by default (NOT ryl's
  1-based code-point columns); `encoding::problem_range` walks the line CR-aware via
  `line_syntax` and the negotiated encoding (UTF-8/16/32), so multibyte/astral-plane
  columns need real surrogate-pair fixtures (BMP `café`/`å` pass vacuously). `lsp-types`
  0.97 forces a benign `bitflags` 1-vs-2 duplicate, allowlisted in `clippy.toml`. The
  `lsp` feature must stay compilable out: CI runs `cargo clippy --no-default-features`
  (the LSP tests are `#![cfg(feature = "lsp")]`). See `docs/editor-integration.md`.
- Exit codes: `0` (ok/none), `1` (invalid YAML), `2` (usage error).
