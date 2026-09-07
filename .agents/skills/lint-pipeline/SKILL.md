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

Exit codes: `0` (ok/none), `1` (invalid YAML), `2` (usage error).

## Subcommand shape

`ryl check <inputs>` (the lint subcommand, #369) and bare `ryl <inputs>` lint
identically: `LintArgs` (`clap::Args`) is flattened both at the top level and under
`Commands::Check`, and the dispatch routes `check` through the subcommand's own
`ArgMatches` so the repeatable `--format`/`--output-file` `indices_of` recovery reads
the right scope. `check` is the recommended form; bare is being phased out
(warn-then-remove, later siblings of the #238 lint/format split). Meta-actions
(`--migrate-*`, `--print-*-config-schema`, `--generate-completions`) stay top-level.

## Inputs

- Accepts one or more inputs: files, directories, or `-` to read from stdin.
- Directories: recursively scanned, honoring git ignore and git exclude; does not
  follow symlinks. Each file's source kind is resolved from the `[files]` globs
  (TOML) or `yaml-files` (YAML); files matching no kind are skipped.
- Files named explicitly are linted as their resolved source kind; one that matches no
  `[files]` kind is rejected with an error (rather than silently treated as YAML).
- Inputs are de-duplicated: a file reached by two spellings (`ryl . f.yaml`, `f.yaml`
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
(`yamllint/linter.py`); a first-line `# ryl/yamllint disable-file`
(`directives::disables_file`) skips the whole file (no diagnostics, not even syntax
errors, no `--fix`). Handling is global: `lint_str` filters every diagnostic through
`Directives::is_disabled` before the syntax-error check, and `fix` reverts edits to
disabled lines via `Directives::reconcile`. Works region-locally in embedded Markdown.
Validate against yamllint with `tests/yamllint_compat_directives.rs`. User docs:
`docs/directives.md`.

## Markdown embedding

Off by default; enabled by `[files].markdown` globs, or per-run via `--markdown` which
injects default globs. ryl extracts front matter and fenced `yaml`/`yml` blocks (each
linted as its own document) and maps diagnostics back to the Markdown file. The
`[markdown]` `front-matter`/`fenced-blocks` booleans (default true) select sources.
Extractor in `src/markdown_embed/` (fenced blocks via `pulldown-cmark`, front matter via
a line scan); each `EmbeddedRegion` carries the `raw_span` and per-line column remap.
`document-start`/`document-end`/`new-line-at-end-of-file`/`new-lines` are suppressed in
regions via `fix::suppressed_rules(kind)`.

`--fix` writes back (`fix::fix_markdown_str`): re-applies each line's stripped prefix
(spaces, `> `, or a tab), preserves CRLF, and only rewrites a region when that reproduces
the original bytes exactly — a ragged region (no single shared prefix) is reported but
left untouched. A Markdown file with a bare `\r` (CR not in CRLF) anywhere is skipped
loudly (`markdown_has_unsupported_cr` guards `lint_markdown_str`/`fix_markdown_str`/
`markdown_parse_skips`: lint error + `--fix`/`--diff` notice): `pulldown-cmark` can't
find fences in a `\r` host and the `\n`-based remap can't place a region `\r`. LF/CRLF
embedded YAML is linted CR-aware. User docs: `docs/markdown.md`.

## `--fix`

`--fix` never mutates a file that does not fully parse: `fix::apply_safe_fixes_filtered`
gates the whole pipeline on `lint::parse_error` (stricter than lint's
`syntax_diagnostic` — it does *not* tolerate undefined aliases), so *any* granit parse
error ⇒ the input is returned byte-for-byte unchanged and `apply_safe_fixes_in_place`
returns `FixOutcome::Skipped(problem)`; the CLI prints a `<path>:L:C skipped by --fix:
<error>` notice. Lint behavior is unchanged: an undefined alias is still not a lint
syntax error (the `anchors` rule reports it, matching yamllint); only `--fix` applies the
stricter gate, through the in-place and per-region Markdown paths.

## `--diff`

`--diff` (#269) previews `--fix` without writing: prints a unified diff (3 lines of
context) per changed file to **stdout** and exits `1` iff any file would change,
mirroring `ruff check --diff`. `conflicts_with` `--fix`, ignores `--format`, supports
stdin. Diff-only: remaining *unfixable* findings are neither printed nor counted (a file
tripping only an unfixable rule exits `0`). Reuses the fix pipeline
(`fix::diff_safe_fixes_for_files` → `fix::diff_outcome`), inheriting the parse-error gate
and symlink skip (both → a `skipped by --diff` notice, no exit effect).

A non-UTF-8/BOM input is likewise skipped (`fix::non_utf8_diff_skip`; files via
`DecodedFile::is_plain_utf8`, stdin via decoded==raw bytes) — a text diff can't apply
back to transcoded bytes, so `--fix` (which re-encodes) is the path for those — as is a
filename with control characters (no representable header). Markdown diffs at host-file
level. The diff *body* is verbatim (hk re-applies it byte-for-byte); the header path is
sanitized and relativized to CWD (like ruff) so it applies via `git apply -p0`. A bare
`\r` is rendered as diff *content* (`render_unified_diff` splits hunk lines on `\n`
only), so a mid-line/mixed `\r` round-trips; content that *ends* in a bare `\r` is
skipped (`fix::ends_in_bare_cr` — `similar` can't render it; use `--fix`).
