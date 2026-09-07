---
name: output-formats
description: >-
  Use when changing `--format`/`--output-file`, the `[output]` TOML table, or
  the JUnit/GitLab report writers. Covers the repeatable-target model, the
  CLI > config > default precedence, every exit-2 guard on conflicting
  destinations, and the GitLab fingerprint contract.
---

# Output Formats

User docs: `docs/output-formats.md`.

## Formats and default streams

Selected with `--format`/`-f`. The streaming console formats
`standard`/`colored`/`github`/`parsable` default to **stderr**; the whole-document
report formats `junit` (JUnit XML via `quick-xml`) and `gitlab` (GitLab Code Quality
JSON via `serde_json`) default to **stdout**. `auto` never selects junit/gitlab.

## Multiple outputs per run

The RuboCop/Biome model: `--format` is repeatable and each `-o/--output-file` binds to the
most recent `--format` (`resolve_cli_targets` recovers CLI order via
`ArgMatches::indices_of`, so `main` uses `Cli::command().get_matches()` +
`from_arg_matches`); `-o -` is stdout, a path is a file, none is the format's default
stream. Console + a report file in one run is therefore supported (closes #285's original
ask), e.g. `--format auto --format gitlab -o gl.json`.

## The `[output]` table

An `[output]` **TOML table** (ryl-only, TOML-only — `config_schema::OutputTable`/
`OutputDestination`, rejected in YAML config) configures the same per-format destinations
(`[output.gitlab] path=…`; absent `path` = default stream, `"-"` = stdout).

Precedence **CLI > config > default**: `resolve_targets` returns the CLI pairs if any
`--format` was given, else `config_targets_from_table` of the run config's `[output]`,
else one default auto-console target. The `[output]` is read run-level by
`run_output_config` (the `-c`/`-d`/env global config, else the inputs-anchored project
config so `ryl .` honors a project `.ryl.toml`; a malformed config is propagated — the
empty-input case has no per-file discovery to surface it, so an invalid `[output]` still
errors). `--diff` skips config `[output]` (it has its own unified-diff output), so only
an explicit CLI `--format junit|gitlab` conflicts with it.

## Pipeline

`collect_records` does the shared filter+tally once into format-agnostic
`FileRecord{path,kept,error}`; `write_targets` renders each target via `render_target`
(`render_streaming` + an `append_*` fn for console formats; `render_junit`/
`render_gitlab` over `build_entries` for reports, built once and shared) and `commit`s to
each `open_destination`.

A file is opened create+write **without** truncate, then truncated+written at commit, so
an *existing* artifact survives a later target failing to open; a *freshly*-created
destination may be left empty on a rejected run — cleaning it by path would race a
concurrent writer, so it is left for the failed run, gate CI artifact use on the exit
code. `open_targets` opens all destinations before `--fix` mutates (unopenable `-o` fails
fast). An empty/all-ignored input set still emits a valid empty report per target
(`emit_targets` with empty records → `[]` / `<testsuites .../>`).

## Guards (each exit 2)

- `resolve_cli_targets` rejects an unpaired `-o` and a second `-o` on one `--format`.
- `validate_targets` rejects `--diff` with a report format, and two outputs on one stream
  (`reject_duplicate_streams`, ≤1 stdout / ≤1 stderr).
- `open_targets` rejects two outputs resolving to one file
  (`reject_colliding_output_files`, post-open so file identity resolves
  symlink/hard-link/aliased-parent destinations — `PathIdentity` = lexical +
  `same_file::Handle`; an unreadable existing destination matches lexically only, an
  adversarial case).
- `reject_input_collisions` refuses an output that is also a linted input or the
  `--stdin-filename` (same lexical + `same_file::Handle` match), so a report can never
  truncate the source.
- `--output-file` `conflicts_with` `--diff` in clap.

## Report entries

`report::ReportEntry` carries the report display path (relativized via
`cli_support::report_display_path` against the project root = `CI_PROJECT_DIR` or cwd,
like ruff; forward-slashed, no `./` prefix; a path outside the root gets `..` segments),
the kept problems, and an optional processing-error message.

GitLab severity maps error->`major`, warning->`minor`, a read/parse failure->`blocker`;
its `fingerprint` is a stable SHA-256 (`sha2`) of `(path, rule, message)` — deliberately
NOT line/column, so an edit that shifts the line keeps the issue tracked — salted to stay
unique within a report (`DefaultHasher` would not be stable across toolchains). A clean
file is a passing JUnit testcase and is omitted from GitLab.

Output is validated against authoritative sources in tests: GitLab against the vendored
`tests/fixtures/gitlab-code-quality.schema.json` (via the `jsonschema` dev-dep), JUnit by
re-parsing with `quick-xml`.
