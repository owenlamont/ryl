---
name: lint-pipeline
description: >-
  Use when changing how ryl takes input or writes files back — the `check`
  subcommand and its flags, directory walking, input de-duplication, source
  kinds, stdin, inline directives, Markdown embedding, or `--fix`/`--diff`.
  Records the invariants each stage holds and why, so a change does not
  quietly break one.
---

# Lint Pipeline

Exit codes: `0` (no errors; warnings alone pass), `1` (lint errors or invalid YAML), `2`
(usage or config error, no rule enabled, or warnings under strict mode).
TOML `[lint] strict = true` enables strict mode; `--strict`/`--no-strict` override
the config. `--diff` retains its change-based exit codes.

## Subcommand shape

`ryl check <inputs>` (the lint subcommand, #369) and bare `ryl <inputs>` lint
identically: `LintArgs` (`clap::Args`) is flattened both at the top level and under
`Commands::Check`, and the dispatch routes `check` through the subcommand's own
`ArgMatches` so the repeatable `-f`/`--output-file` `indices_of` recovery reads
the right scope. `check` is the recommended form; bare is deprecated: `run_cli` prints
a one-line stderr warning (skipped under `--no-warnings` or with no inputs) after every
meta-action has returned, and removal is a later sibling of the #238 lint/format split.
Meta-actions
(`--migrate-*`, `--print-*-config-schema`, `--generate-completions`) stay top-level.

## Inputs

- Accepts one or more inputs: files, directories, or `-` to read from stdin.
- Directories: recursively scanned, honoring git ignore and git exclude; does not
  follow symlinks. Each file's source kind is resolved from the `[files]` globs
  (TOML) or `yaml-files` (YAML); files matching no kind are skipped.
- Files named explicitly are linted as their resolved source kind; one that matches no
  `[files]` kind is rejected with an error (rather than silently treated as YAML).
- Config exclusions and `--exclude` skip even explicitly named files; `--exclude`
  replaces config patterns and `exclude-from-file` with cwd-relative globs.
- Inputs are de-duplicated: a file reached by two spellings (`ryl check . f.yaml`, `f.yaml`
  twice, or `f.yaml sub/../f.yaml`) is processed once. `gather_lint_files` keys a `seen`
  set on `main::canonical_input` (`std::path::absolute` + lexical `..` normalization —
  purely lexical, no symlink resolution, so a symlink stays distinct from its target),
  spanning lint/`--fix`/`--diff`/`--list-files`. Stricter than yamllint (which keeps
  duplicates); for `--diff` a duplicate would emit a repeat patch block that fails to
  apply on the second copy.
- Source kinds (`config::SourceKind`): the `[files]` TOML table maps `yaml` and
  `markdown` to glob lists (`yaml` defaults to `*.yaml`/`*.yml`/`.yamllint`). A file
  matching two kinds is a hard error. `yaml-files` is rejected in TOML (use
  `[files].yaml`); it remains valid in the legacy YAML config.

## Stdin

Bytes are read raw and decoded with the same BOM/encoding detection as files; `-` can't
be combined with other inputs or with `--fix`. `--stdin-filename <PATH>` (ruff
convention) sets the diagnostic label, anchors config discovery at the path's parent,
resolves the source kind from `[files]` (a `markdown` path → embedded YAML), and runs
`yaml-files`/per-file-ignore/per-rule `ignore` against it. Without it, diagnostics are
`<stdin>`, config is anchored at CWD, and all path-based filtering is skipped so every
enabled rule runs; `--markdown` forces Markdown.

## Inline directives

`src/directives.rs`: `# ryl disable` / `enable` / `disable-line` (and `# yamllint …`
aliases) suppress rules for a block or line, mirroring yamllint's grammar
(`yamllint/linter.py`); only the `# ryl` spelling may share a comment with other `#`
text. A first-line `# ryl/yamllint disable-file`
(`directives::disables_file`) skips the whole file (no diagnostics, not even syntax
errors, no `--fix`). Handling is global: `lint_str` filters every diagnostic through
`Directives::is_disabled` before the syntax-error check, and `fix` reverts edits to
disabled lines via `Directives::reconcile`. Works region-locally in embedded Markdown.
Direct and CLI coverage lives in `tests/directives.rs` and `tests/cli_directives.rs`.
User docs: `docs/directives.md`.

## Markdown embedding

Off by default; enabled by `[files].markdown` globs, or per-run via `--markdown` which
injects default globs. ryl extracts front matter and fenced `yaml`/`yml` blocks (each
linted as its own document) and maps diagnostics back to the Markdown file. The
`[markdown]` `front-matter`/`fenced-blocks` booleans (default true) select sources.
Extractor in `src/markdown_embed/` (fenced blocks via `pulldown-cmark`, front matter via
a line scan); each `EmbeddedRegion` carries the `raw_span` and per-line column remap.
`document-start`/`document-end`/`new-line-at-end-of-file`/`new-lines` are suppressed in
regions via `fix::suppressed_rules()`.

`--fix` writes back (`fix::fix_markdown_str`): re-applies each line's stripped prefix
(spaces, `> `, or a tab), preserves CRLF, and only rewrites a region when that reproduces
the original bytes exactly and parsing the replacement recovers the complete rewritten
region — ragged regions and rewrites that create closing fences are left untouched.
A Markdown file with a bare `\r` (CR not in CRLF) anywhere is skipped
loudly (`markdown_has_unsupported_cr` guards `lint_markdown_str`/`fix_markdown_str`/
`markdown_region_problems`: lint error + `--fix`/`--diff` notice): `pulldown-cmark` can't
find fences in a `\r` host and the `\n`-based remap can't place a region `\r`. LF/CRLF
embedded YAML is linted CR-aware. User docs: `docs/markdown.md`.

## `--fix`

`--fix` requires a complete relaxed parse, including resolved aliases. Lint and LSP
diagnostics use the parser's strict indentation mode; rewrite paths may repair relaxed-only
input when reindent preserves events and the final output parses strictly with the
original events. Repairs produce a notice; a disabled or unsafe repair is refused.
Lint repairs require the `ALL` fixable selector, honor indentation ignores and use
formatter indentation targets; per-rule LSP fixes suppress the repair pass.
Other parse errors return the input byte-for-byte unchanged and `rewrite_str`
returns it as a skip; the CLI prints a `<path>:L:C skipped by --fix:
<error>` notice. A later fixer can expose a diagnostic an earlier one fixes, so the
pipeline repeats until a pass changes nothing (capped at `FIX_PIPELINE_MAX_PASSES` = 100,
ruff's `MAX_ITERATIONS`): one `--fix` reaches the fixed point, and the "N fixed, M
remaining" summary and `--diff` see only the final text. N sums each file's drop per
safe-fix rule and "Found" is N + M, as in ruff, so a problem a fix introduces raises it
above the pre-fix count. If a pass past the cap would still change the text,
`apply_safe_fixes_capped` writes ruff's "Failed to converge" error (a ryl bug report
request naming the rules still changing) to stderr from the loop itself, so the CLI and
LSP both get it; `--no-warnings` does not suppress it, the text as of the cap is kept,
and the exit code is unaffected. After convergence,
`key_ordering::unfixed` names each mapping its fix declined; `FixOutcome::skipped` and
`DiffOutcome::skipped` carry those as `rule: Some("key-ordering")` problems, which the
CLI prints as `<path>:L:C key-ordering not fixed: <reason>` (Markdown regions mapped to
host lines like parse skips). Tests reach it through
`apply_safe_fixes_capped` with a small cap. Lint behavior is unchanged: an undefined
alias is still not a lint syntax error (the `anchors` rule reports it, matching
yamllint); only rewrites require resolved aliases, through the in-place and per-region
Markdown paths.

`fix::rewrite_files` and `fix::diff_files` read and rewrite files in parallel
(`read_in_parallel`), then write serially in input order. A sole pending input of at
most 64 KiB runs on the calling thread (`cli_support::small_file_batch`, also used by
the lint batches); larger inputs, batches and metadata failures stay parallel. Symlink
skips, captured stderr
(`cli_support::capture_file_stderr`) and skip notices enter one ordered `notices`
buffer. `format` adds read and write errors to that buffer; `check --fix` leaves errors
for the ordered lint report after the notice block. `diff_files` flushes notices before
returning a read error.
Lexical de-duplication
keeps hard links distinct, so `rewrite_files` splits inputs sharing a file identity into
separate batches: the later path reads the earlier path's write, as a serial run would.

## `--diff`

`--diff` (#269) previews `--fix` without writing: prints a unified diff (3 lines of
context) per changed file to **stdout** and exits `1` iff any file would change,
mirroring `ruff check --diff`. `conflicts_with` `--fix`, ignores `-f`, supports
stdin. Diff-only: remaining *unfixable* findings are neither printed nor counted (a file
tripping only an unfixable rule exits `0`). Reuses the fix pipeline
(`fix::diff_files` → `fix::diff_outcome`), inheriting the parse-error gate
and symlink skip (both → a `skipped by --diff` notice, no exit effect).

A non-UTF-8/BOM input runs through `fix::decoded_diff_outcome` for files and stdin:
decoded-text changes count toward exit `1`, but the patch is suppressed with a notice
because a text diff can't apply back to transcoded bytes; use `--fix` to re-encode.
A filename with control characters is skipped (no representable header). Markdown diffs
are at host-file level. The diff *body* is verbatim (hk re-applies it byte-for-byte);
the header path is sanitized and relativized to CWD (like ruff) so it applies via
`git apply -p0`. A bare
`\r` is rendered as diff *content* (`render_unified_diff` splits hunk lines on `\n`
only), so a mid-line/mixed `\r` round-trips; content that *ends* in a bare `\r` is
skipped (`fix::ends_in_bare_cr` — `similar` can't render it; use `--fix`).

## `ryl format`

`ryl format` and `ryl check --fix` both run `fix::rewrite` over a resolved
`fix::Passes` value. A file-wide disable skips the input. A strict-invalid input is
first re-indented (`fix::repair`) when that alone makes it strictly valid with the same
representation. `format::layout` then plans every rule in one candidate emission with
strict event verification, the whole-format semantic guard and a mandatory dry plan.
`truthy` and `key-ordering` run after each settled layout as guarded text fixes; a
change restarts the layout, bounded by `FIX_PIPELINE_MAX_PASSES`. An input with an
ambiguous blank block-scalar body line gets key ordering only. A layout decline rolls
back to the original bytes.
`ryl check --fix`
builds its `Passes` from the lint config (rule selection, per-rule `ignore`,
per-file-ignores, `fixable`/`unfixable`, per-line-ignores); `ryl format` builds its own
from the `[format]` table over `format::FORMAT_RULE_IDS` and reads none of those, so a
zero-config run formats. Inline directives are honoured by both. A YAML config gets the
`[format]` defaults.

Every changed formatter result must parse strictly and preserve the original significant
events; a failed comparison rolls the rewrite back to the original input.

Layout handles suppression and indentation refusals while retaining source bytes for
refused documents; a candidate whose re-indentation changes a document's events
freezes that document and replans. Collapses that cannot share a virtual line retry
after a separately verified quote epoch, and a folded-tail interaction retries with an
unindented first epoch. A comma before a closing bracket takes the bracket rule's
spacing; where the two lint options contradict, `check --fix` reports the
non-convergence error instead of looping. Lint colon and hyphen options plan from
their own configs; the cached spacing sites serve only the formatter targets.
Flow spacing shares cached punctuation sites; brackets retain their existing traversal
when template text occurs, because braces and commas skip template spans.

Formatter quote decisions retain the value-keyed plain-snippet cache across plans.
Column-zero document-marker checks run before the value-only cache lookup.
Unguarded ladder replacements settle locally before indentation measures
the resulting scalar lengths; a no-edit certificate avoids a whole-document quote
probe. Multiline collapses change virtual lines; multiline style changes compose
line-local edits. Quote and dirty dry-plan interactions settle through bounded,
verified layout epochs; partially suppressed multiline quotes preserve rollback.
Every quote replacement retains
the existing schema and snippet checks;
unchanged plain and block scalars need no quote decision.

Forced collection targets plan conversion before layout, retaining the existing
comment, key, width and reindent refusals. A strict parse compares significant
events with only collection style relaxed, alongside the semantic comparison.
All folding profiles settle quote edits in a separately verified epoch before
measuring fold endpoints. Conversions with comments align comments before
conversion, defer further comment alignment until after reindentation, then run
the full layout and dry plan. A dry collection plan must also have no remaining
edits. A remaining conversion plan becomes the next verified stage. Rejected
conversions can retry after a changed layout stage, retaining the existing
fixed-point bound. Dirty dry plans continue from the verified candidate;
consistency refusals replan from original bytes with the refused documents frozen.

The layout planner retains source provenance through dash joins, scalar collapses,
comment shifts, folding and boundary finishing. Set
`RYL_LAYOUT_STATS` to a JSONL destination to record per-input fallback reasons,
traversal/emission counts and elapsed time during development.

Both plans collect shared header, comment, sparse spacing and folding facts during
lexical scanning. The input buffers the unresolved line, projects tokens against
settled local gaps and quotes, and feeds the indentation analyzer directly.
The dry pass invokes the same producers and layout planner on fresh candidate facts;
remaining edits start another verified layout epoch, and consistency refusals
freeze the affected documents before replanning from original bytes.
Strict candidate verification compares expected events and semantic events in
the same parse. Refusal reporting stores epoch-bound evidence without text copies.
BOM dry inputs retain the
cached normalizer, whose marker columns distinguish prefix BOMs from scalar content.
Physical lines borrow slices from the byte-start table; scalar protection ranges are
cached per text epoch. Space replacements emit counts.

Serial formatter epochs observe settled scanner tokens during strict parsing and
seed compact lexical tapes. Formatter epochs of at least 1 MiB retain overlapping
strict parsing and lexical prefetch when CPU availability and the current Rayon
pool permit multiple workers. Candidate tapes are accepted only after verification;
each tape is released after emission or dry planning. Initial parse counts are
seeded when formatter recording starts, so input parsing stays unrecorded.
Counters stay on the caller so stolen tasks cannot charge another file's recording.
Check captures lexical evidence during strict parsing only when enabled rules need
scanner tokens; failed parses retain the standalone relaxed scanner and repair path.

For fallback and indentation-only callers, changed single-document output of at
least 1 MB uses one strict streaming parse to compare significant events against the
input tape through EOF and cache compact
output facts for the subsequent passes. A successful comparison certifies strict validity.
Strict parse errors retain the cached relaxed comparison and final strict-output guard.
Small files and multi-document streams retain their existing verification paths.
Consistency checks, per-document rollback and the bounded fixed-point loop still run.

- `--check`/`--diff` attach `format::problems` to `DiffOutcome`: the formatting rules'
  checkers on the original text, kept only for rules that actually edited and lines no
  directive disables, plus a `1:1 would reformat` line per editing rule that no checker
  explained. `--check` sends them through config `[output]`; `--diff` keeps stdout for
  the patch and uses the default target. Ragged Markdown regions remain unchanged;
  retained layout findings are reported and keep format previews failing.
- `format::conflicts` formats a fixed probe and lints it under the file's config; each
  enabled formatting rule that rejects the output is a stderr warning, deduplicated by
  text across configs, silenced by `--no-warnings`, never printed by `ryl check`.
- `format::unfixed` reports retained indentation, dash-layout, spacing and other
  formatter refusals. Every mode prints them as
  `<path>:L:C <rule> not fixed: …` skip notices; they never change the exit code.
- File and stdin output preserve the detected encoding and BOM. Encoded input still
  runs through the formatter for `--check`/`--diff`: decoded-text changes determine
  exit `1` and diagnostics, even when no applicable text patch can be emitted.
  Formatting changes also count when a trailing bare CR or an unrepresentable filename
  prevents a patch; `ryl check --diff` keeps those skips without an exit effect.
