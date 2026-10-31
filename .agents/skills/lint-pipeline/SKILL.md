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
- Directories: recursively scanned, honoring git ignore and git exclude, skipping
  `.git`/`.hg`/`.svn`/`.jj` below the root (`discover::walk_dir`, shared with the
  LSP); does not follow symlinks. Each file's source kind is resolved from the
  `[files]` globs (TOML) or `yaml-files` (YAML); files matching no kind are skipped.
- Files named explicitly are linted as their resolved source kind; one that matches no
  `[files]` kind is rejected with an error (rather than silently treated as YAML).
- A YAML-kind file whose first line is a Salt Jinja renderer header
  (`directives::starts_with_jinja_header`: `#!jinja|yaml`, `#!yaml_jinja`) is a
  template. Named, or on stdin under either kind resolution, it exits 2; walked, the
  content-level `disables_file` skips it in every read path (lint, fix, format, LSP) at
  no extra I/O, and only `--list-files` peeks to omit it. Rewrite paths check
  `disables_file` before their parse gate, so an unparsable template prints no skip.
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
diagnostics report the error strict indentation stops at, which the one relaxed parse
records (`ParsedFacts::strict_error`); rewrite paths may repair relaxed-only input when
the reindent alone parses strictly with the original events. Later fixes are checked
against the repaired text, as for strictly valid input, so `key-ordering` may reorder it.
Repairs produce a notice; a disabled or unsafe repair is refused.
Lint repairs require the `ALL` fixable selector, honor indentation ignores and use
formatter indentation targets; per-rule LSP fixes suppress the repair pass.
Other parse errors return the input byte-for-byte unchanged and `rewrite_str`
returns it as a skip; the CLI prints a `<path>:L:C skipped by --fix:
<error>` notice. A later fixer can expose a diagnostic an earlier one fixes, so the
pipeline repeats until a pass changes nothing (capped at `FIX_PIPELINE_MAX_PASSES` = 100,
ruff's `MAX_ITERATIONS`): one `--fix` reaches the fixed point, and the "N fixed, M
remaining" summary and `--diff` see only the final text. N sums each file's drop per
safe-fix rule and "Found" is N + M, as in ruff, so a problem a fix introduces raises it
above the pre-fix count. After convergence,
`key_ordering::unfixed` names each mapping its fix declined; `FixOutcome::skipped` and
`DiffOutcome::skipped` carry those as `rule: Some("key-ordering")` problems, which the
CLI prints as `<path>:L:C key-ordering not fixed: <reason>` (Markdown regions mapped to
host lines like parse skips). Tests reach it through
`apply_safe_fixes_capped` with a small cap. Lint behavior is unchanged: an undefined
alias is still not a lint syntax error (the `anchors` rule reports it, matching
yamllint); only rewrites require resolved aliases, through the in-place and per-region
Markdown paths.

Every multi-file pass (`lint_files`, `lint_files_cached`, and `fix::rewrite_files` and
`fix::diff_files` through `read_in_order`) runs on `cli_support::try_for_each_ordered`.
The calling thread spawns chunks of files on the rayon pool, at most `IN_FLIGHT` chunks
past the oldest unemitted one, and hands each result to its `emit` callback in input
order, so memory stays flat in the file count. Tasks never block, so a nested
`rayon::join` cannot deadlock against the window, and a worker panic is re-raised on the
caller. `check` renders each record to its targets from that callback, `diff_files`
streams each diff and `format` finding, and lint-cache misses are stored every
`CACHE_BATCH` files. The helper captures each file's stderr for `emit` to replay, through
the console target when it owns stderr. An error that aborts the run leaves the files
before it already
written. `rewrite_files` writes each file from the task that rewrote it, so
no file's text outlives its task; hard-link batches keep two paths to one file in separate
passes. A sole pending input of at
most 64 KiB runs on the calling thread (`cli_support::small_file_batch`); larger inputs,
batches and metadata failures stay parallel. Symlink
skips, captured stderr
(`cli_support::capture_file_stderr`) and skip notices enter one ordered `notices`
buffer, printed once the run ends. `format` adds read and write errors to that buffer;
`check --fix` leaves errors for the ordered lint report after the notice block.
`diff_files` flushes notices before returning a read error.
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
representation. `format::layout` then plans every rule in one candidate emission and
writes it without parsing it again. `check --fix` lint passes instead replan
their output until no edit remains, since lint options can contradict each other.
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

Value preservation and idempotence hold by rule order: each target reads only preserved
facts and earlier targets. The property suites, not runtime checks, catch a rule that
breaks the order. Plan-time refusals stand in for the old rollbacks: a block
scalar's first trailing comment that would move further than its body freezes its
document, and a `---` line followed by non-ASCII whitespace, or a root keep scalar
whose final spaces-only line ryl's parser reads unlike the reference parser, falls back.

Layout handles suppression and indentation refusals while retaining source bytes for
refused documents. The formatter drops a comma before a closing bracket. In
`check --fix` the comma takes the bracket rule's spacing, and is dropped where the two
lint options contradict. Lint colon and
hyphen options plan from their own configs; the cached spacing sites serve only the
formatter targets.
Flow spacing shares cached punctuation sites; brackets retain their existing traversal
when template text occurs, because braces and commas skip template spans.

Formatter quote decisions retain the value-keyed plain-snippet cache across plans.
Column-zero document-marker checks run before the value-only cache lookup.
Unguarded ladder replacements settle locally before indentation measures
the resulting scalar lengths; a no-edit certificate avoids a whole-document quote
probe. Multiline collapses change virtual lines; multiline style changes compose
line-local edits. A partially suppressed multiline quote edit falls back at plan time.
Every quote replacement retains
the existing schema and snippet checks;
unchanged plain and block scalars need no quote decision.

Keep scalar and comment selection in their `target` modules, using the semantic views
in `text_version::model` and narrow observations. Quote eligibility precedes a selected
spelling recipe; source copying materializes the recipe without choosing a style.
Quote observations privately borrow that scalar's content and expose lazy predicates,
without a text getter.
Spacing resolves an observed `GapRequest` into a private `IndentationGap` before the
indentation analyzer consumes it. Folding settles a scalar candidate and permits a line
before choosing breaks. Line-ending targets resolve before emission. These decision
interfaces must not accept `Analysis`, `TextVersion`, parser events, whole-file text,
or source-backed callbacks. Private owner observations and source adapters handle
extraction, coordinates, copying and verification. Other owners supply retained or
selected facts with private fields; scalar text and parser tag spelling stay out of
the semantic model.

Collection conversion consumes retained scalar, spacing, comment and geometry facts
before quote selection. Keep this epoch order: selecting quotes globally first changes
flow eligibility. Collection policy chooses opaque copy, separator and break recipes;
its source adapter materializes them. Feasibility and standalone reindentation share
shape, indentation, refusal and consistency kernels.

Layout candidates settle refusals before publishing immutable indentation targets.
The consuming `LayoutTargets::finish_lines` and `LineTargets::fold` transitions produce
`EmitPlan`; candidates cannot finalize lines or emit. Line owners select scalar
protection, marker attachment, blank quotas and EOF endings. Folding consumes selected
styles and final line geometry, with characters outside the scalar window hidden.

Forced collection targets plan conversion before layout, retaining the existing
comment, key, width and reindent refusals. Block-to-flow width checks use target
indentation columns, computed lazily when a candidate needs them and cached for the
conversion stage.
Conversions with comments align comments before conversion, defer further comment
alignment until after reindentation, then run the full layout.

The layout planner retains source provenance through dash joins, scalar collapses,
comment shifts, folding and boundary finishing. Set
`RYL_LAYOUT_STATS` to a JSONL destination to record per-input fallback reasons,
traversal/emission counts and elapsed time during development.

Both plans collect shared header, comment, sparse spacing and folding facts during
lexical scanning. The input buffers the unresolved line, projects tokens against
settled local gaps and quotes, and feeds the indentation analyzer directly.
With `indent-width` unset, the plan checks a consistent width over its own target columns,
refusing each document that breaks it until none does.
Refusal reporting stores the emit plan's refusals, kept dashes and width violations,
mapped to output lines, as epoch-bound evidence, with whether a block scalar header
could make the output ambiguous and whether a flow collection stays where `block` is
configured (`collection_style::Plan::settles`); only then do notices parse the output.
A preview skips notices outright when that evidence is quiet (`FormatCache::quiet`).
Markdown notices come from each region's own rewrite in `fix_markdown_cached`.
BOM inputs retain the
cached normalizer, whose marker columns distinguish prefix BOMs from scalar content.
Physical lines borrow slices from the byte-start table; scalar protection ranges are
cached per text epoch. Space replacements emit counts.

Serial formatter epochs observe settled scanner tokens during strict parsing and
seed compact lexical tapes. Formatter epochs of at least 1 MiB retain overlapping
parsing and lexical prefetch when CPU availability and the current Rayon
pool permit multiple workers. Each tape is released after emission. Initial parse
counts are seeded when formatter recording starts, so input parsing stays unrecorded.
Counters stay on the caller so stolen tasks cannot charge another file's recording.
Check captures lexical evidence while parsing only when enabled rules need scanner
tokens. Input that only strict indentation rejects keeps the facts and tokens of that
same relaxed parse for the repair path.

Re-indentation for fallback, notice and repair callers shares the layout's refusals, so
it keeps every document's events. `fix::repair` reuses the input's relaxed parse and
parses its result once, strictly, to compare representations.

- `--check`/`--diff` attach one `unformatted` problem per changed file to `DiffOutcome`,
  at the first change in its diff (`fix::would_reformat`), as ruff does. `--check`
  sends it through config `[output]`, with the diff under it in `standard`/`colored`;
  `--diff` prints only the patch. Both end with a stderr summary
  (`format_summary`). Ragged Markdown regions remain unchanged;
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
