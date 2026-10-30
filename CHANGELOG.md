# Changelog

Release dates below use the GitHub release publication date in UTC. Historical entries
summarize user-visible changes from the tagged commits and published release records.

## Unreleased

## v0.27.0

### Added

- [ryl-action](https://github.com/marketplace/actions/ryl-action) runs ryl in GitHub
  Actions, pinned per release as `owenlamont/ryl-action@v0.27.0`.

### Changed

- `ryl server` reports open files only by default, as ruff, rumdl and biome do. The new
  `diagnosticMode` setting, named after ty's, restores the workspace-wide
  `workspace/diagnostic` scan with `"workspace"`. A mistyped server setting is now
  reported through `window/showMessage` instead of silently ignored.
  ([#407](https://github.com/owenlamont/ryl/issues/407))
- `ryl format --check` reports each file that would change as one `unformatted` finding
  at its first change, with the diff under it, as ruff does, instead of one finding per
  rule and line. `--check` and `--diff` end with a summary such as
  `1 file would be reformatted`.
  ([#843](https://github.com/owenlamont/ryl/issues/843))
- `ryl format` parses each file, or each YAML block in Markdown, once, including under
  `--check` and `--diff`, unless it reports a repair or a `not fixed` notice. Debug
  builds no longer re-check formatter output.
  ([#843](https://github.com/owenlamont/ryl/issues/843))
- A file whose first line is a Salt Jinja renderer header (`#!jinja|yaml`,
  `#!yaml_jinja`) is a template that ryl neither lints nor formats: a directory scan
  skips it, and naming it or piping it to stdin exits `2`.
  ([#781](https://github.com/owenlamont/ryl/issues/781))
- Building ryl from source, as `cargo install ryl` does, needs Rust 1.99.1.
  ([#876](https://github.com/owenlamont/ryl/pull/876))

### Fixed

- A file or Markdown block opening with `# ryl disable-file` no longer prints a syntax
  or encoding skip notice under `check --fix`/`--diff` or `ryl format`.
- `ryl format` no longer panics on a document whose re-indentation is refused while a
  hyphen-spacing edit applies.
- `sequence-style = 'flow'` converts a sequence whose items end in trailing whitespace,
  where the whole file was previously left unformatted.
- Collection conversion handles sequences flush with their mapping key, so a spacing
  change no longer prevents another collection in the document from being converted.
- Flow conversion measures width at the target indentation, so changing source
  indentation no longer changes which collections fit.
- Re-indentation preserves a block scalar's value when the final line contains spaces
  without a line break.
  ([#838](https://github.com/owenlamont/ryl/issues/838))
- `ryl format` joins a one-column mapping onto its sequence dash in one run when the
  file contains a byte order mark.
- Formatting preserves a compact collection's opening gap when the collection's
  indentation cannot be changed safely.
- Restyling a collection no longer dedents a quoted scalar's final continuation
  line that starts with `#` when the file has no final line break.
- Formatting and lint fixes preserve ytt `#@` directives and their inline spacing
  instead of converting them into ordinary comments, and the `comments` rule no
  longer reports them.
  Block collection style leaves a flow collection with a trailing directive as
  written, and the `key-ordering` fix skips a mapping holding an own-line one.
  Contributed by [@samirhvbr](https://github.com/samirhvbr) in
  [#877](https://github.com/owenlamont/ryl/pull/877), fixing
  [#780](https://github.com/owenlamont/ryl/issues/780).

## v0.26.1

### Added

- ryl builds on its own YAML parser, published as the `ryl-yaml-parser` crate.
  ([#816](https://github.com/owenlamont/ryl/pull/816))

### Changed

- `ryl format` is faster than yamlfmt, dprint and Prettier at every benchmarked size
  and file count from 4 KiB, tying yamlfmt on a single 1 KiB file. ([#787](https://github.com/owenlamont/ryl/issues/787))
- `ryl check` streams its output and writes formatted files as it goes, so memory no
  longer grows with the number of files (218 MB to 22 MB at 20,000 files).
  ([#831](https://github.com/owenlamont/ryl/pull/831))
- `ryl format` parses each file once: it no longer parses its output again to compare
  values or plans it again to confirm convergence, which rule order now guarantees.
  ([#842](https://github.com/owenlamont/ryl/issues/842))

### Fixed

- `ryl format` moves a blank-only block scalar body with its collection, so its value
  no longer changes when the collection shifts, and formats such scalars with an
  explicit indentation indicator instead of leaving the file unchanged.
  ([#849](https://github.com/owenlamont/ryl/pull/849))
- The `comments-indentation` rule, and the formatter, no longer read a trailing comment
  on a multi-line scalar's last line, or an empty block scalar's header line, as other
  than content, matching yamllint; a folded scalar no longer moves the comments after it
  on a second format.
- `ryl format` settles in one run, where it took two, for `'---' : x` (now `---: x`), the
  spaces-only lines of a blank keep-chomped body under `- key:`, and a dash it moves onto
  its own line after an explicit `:` or in a file with a byte order mark.
- `ryl format` no longer aligns a comment with a `---` or `...` line, which it may add,
  so a comment after a block scalar keeps its column when `document-end = 'add'` adds a
  marker below it.
- `ryl format` re-indents a document whose only tab follows an indicator that it respaces,
  and no longer reports one it did re-indent where folding starts a line with `-` and a
  tab.
- `ryl check --fix` keeps an indentation repair when `key-ordering` also reorders the
  file, instead of leaving the file unchanged and reporting the indentation as a syntax
  error.
- `ryl format` re-indents a block sequence holding a bare `-` item, such as a sequence
  flush with its key whose first item is empty, instead of refusing the document.
  ([#836](https://github.com/owenlamont/ryl/issues/836))
- The `indentation` rule no longer reports a sequence item after a nested sequence
  ending in a bare `-` as wrongly indented, under every `indent-sequences` setting.
  ([#836](https://github.com/owenlamont/ryl/issues/836))
- `ryl format` keeps a keep-chomped block scalar's final spaces-only line when
  re-indenting empties that line, as at indent width 1, so the scalar keeps its final
  empty line.
- Root strings resembling document markers and scalars containing a BOM keep their
  quotes in formatting and lint fixes.
  ([#806](https://github.com/owenlamont/ryl/issues/806))
- Migration warnings name the required ryl version, quote config paths, and remove
  legacy project YAML configs; `--migrate-configs` also converts TOML `yaml-files` to
  `[files].yaml`. ([#789](https://github.com/owenlamont/ryl/issues/789))

## v0.26.0

### Breaking changes

- `ryl format` and editor formatting default to indent width 2; set top-level
  `indent-width` to keep another width. Lint indentation still defaults to `consistent`.
  ([#758](https://github.com/owenlamont/ryl/issues/758))
- `ryl check` and LSP diagnostics reject spec-invalid indentation; formatting and lint
  fixes can repair indentation only when strict parsing and event preservation succeed,
  with a repair notice. ([#635](https://github.com/owenlamont/ryl/issues/635))

### Added

- `--disable <RULES>` subtracts lint rules after selection, and `--exclude <GLOBS>`
  overrides file exclusions; requested by [@entorb](https://github.com/entorb) in
  [#766](https://github.com/owenlamont/ryl/issues/766).
- TOML `[lint] strict = true` enables strict lint exit codes, overridable with
  `--no-strict`; requested by [@Winipedia](https://github.com/Winipedia) in
  [#522](https://github.com/owenlamont/ryl/issues/522).
- TOML-only `anchors.forbid-anchors-and-aliases` reports every anchor and alias without
  automatic fixes; requested by [@pygarap](https://github.com/pygarap) in
  [#515](https://github.com/owenlamont/ryl/issues/515).
- Content-hash caching speeds repeated `check` and `format` runs, with CLI, environment
  and TOML controls. ([#750](https://github.com/owenlamont/ryl/issues/750))

### Changed

- Linting, formatting and safe fixes reuse parse and scalar analysis, skip unchanged
  work and process file contents in parallel while keeping writes and diffs in input
  order. ([#748](https://github.com/owenlamont/ryl/issues/748),
  [#750](https://github.com/owenlamont/ryl/issues/750),
  [#758](https://github.com/owenlamont/ryl/issues/758))
- Parsing accepts explicit `?` keys at EOF, quoted continuations one column past block
  indentation and tabs separating explicit keys, and corrects block-scalar EOF values
  and span positions.

### Fixed

- Formatter and lint fix/diff notices follow input order, including symlink skips, parse
  skips and read errors. ([#754](https://github.com/owenlamont/ryl/issues/754))
- Rule references and agent guidance now describe strict overrides, rule/file selection,
  formatting defaults, guarded repair and encoded diff exit codes accurately.

## v0.25.1 - 2026-10-16

### Changed

- The PyPI package supports Python 3.11 to 3.15: it drops end-of-life 3.10 and
  declares 3.15, and releases smoke-test 3.15 and free-threaded 3.15t wheels.
  ([#726](https://github.com/owenlamont/ryl/issues/726))
- A new pre-commit and prek docs page covers the `ryl-check` and `ryl-format` hooks
  and running a hook with `--verbose` to see config deprecation warnings.
  ([#730](https://github.com/owenlamont/ryl/issues/730),
  [#729](https://github.com/owenlamont/ryl/issues/729))
- The ryl agent skill names the `ryl-check` and `ryl-format` pre-commit hooks.
- `ryl --help` describes ryl as a YAML linter and formatter.

### Fixed

- The SchemaStore copy of the TOML config schema drops schemars' integer `format`
  values, which SchemaStore's validator rejects.
  ([#725](https://github.com/owenlamont/ryl/pull/725))
- The quick start leads with `--enable`, inline TOML via `-d` and copying a preset
  into `.ryl.toml` instead of the deprecated YAML `extends: default`, and marks YAML
  config deprecated where it remains.
  ([#731](https://github.com/owenlamont/ryl/issues/731))
- Config deprecation warnings name a command that rewrites the file from any
  directory, `ryl --migrate-configs --migrate-write --migrate-root <config>`, instead
  of the preview-only command. Where migration cannot rewrite it (`pyproject.toml`,
  a symlink, inline `-d`, a custom-named file), the warning says what to do instead.
  ([#727](https://github.com/owenlamont/ryl/issues/727))
- A `--migrate-configs` or `--migrate-user-config` preview that lists configs now
  ends with a hint naming `--migrate-stdout` to see the migrated content and
  `--migrate-write` to apply it.
  ([#728](https://github.com/owenlamont/ryl/issues/728))

## v0.25.0 - 2026-10-16

### Added

- `ryl format` formats YAML in place, checks or previews changes, and formats YAML
  embedded in Markdown with `--markdown`.
  ([#519](https://github.com/owenlamont/ryl/pull/519),
  [#520](https://github.com/owenlamont/ryl/pull/520),
  [#538](https://github.com/owenlamont/ryl/pull/538),
  [#605](https://github.com/owenlamont/ryl/pull/605))
- `[format]` controls indentation, line endings, quotes, collections, comments,
  blank lines, folding and document markers. Unset indentation detects the file's
  existing width; `line-ending = "auto"` keeps its prevailing line ending.
  ([#550](https://github.com/owenlamont/ryl/pull/550),
  [#606](https://github.com/owenlamont/ryl/pull/606))
- `--output-format` selects output for `check` and `format --check`; only `check`
  also accepts `-f`/`--format`. ([#605](https://github.com/owenlamont/ryl/pull/605))
- Editor Format Document uses `ryl format`, including for embedded YAML in Markdown.
  ([#583](https://github.com/owenlamont/ryl/pull/583))
- Top-level TOML `line-length` and `indent-width` provide shared defaults for lint
  and format. ([#521](https://github.com/owenlamont/ryl/pull/521))
- `check --fix` can repair `colons` and `hyphens` spacing.
  ([#524](https://github.com/owenlamont/ryl/pull/524))
- `fold-long-lines` can wrap plain, quoted and folded block scalars.
  ([#523](https://github.com/owenlamont/ryl/pull/523),
  [#541](https://github.com/owenlamont/ryl/pull/541),
  [#536](https://github.com/owenlamont/ryl/pull/536))
- `sequence-style` and `mapping-style` can convert collections between flow and
  block forms; `indent-width` reindents block collections.
  ([#544](https://github.com/owenlamont/ryl/pull/544),
  [#551](https://github.com/owenlamont/ryl/pull/551),
  [#545](https://github.com/owenlamont/ryl/pull/545))
- Formatting can choose quotes that avoid escapes and configure brace spacing,
  comment spacing and blank-line limits.
  ([#539](https://github.com/owenlamont/ryl/pull/539),
  [#540](https://github.com/owenlamont/ryl/pull/540),
  [#546](https://github.com/owenlamont/ryl/pull/546))

### Changed

- Ambiguous block scalars leave the whole YAML file, or the Markdown region holding
  them, unchanged by formatting and safe fixes apart from key ordering; both commands
  name the file, line and reason on stderr.
- When formatting would change a Markdown region with mixed line endings, `format`
  names the refusal and `--check`/`--diff` fail; a fence-boundary refusal is also named.
  ([#713](https://github.com/owenlamont/ryl/pull/713))
- Indentation conflict warnings identify `indent-width` as a TOML key and point
  YAML-configured users to config migration.
  ([#713](https://github.com/owenlamont/ryl/pull/713))
- `format` and `check --fix` continue after a file write fails, report the file,
  and exit `2` after processing the remaining inputs.
  ([#714](https://github.com/owenlamont/ryl/pull/714))
- `check --fix` and `ryl format` finish unterminated block scalars with a newline
  and, when configured, a document end marker, following the YAML 1.2 reference parser.
  Adding a marker removes value-neutral indentation spaces on a final blank
  block-scalar line, but leaves a root explicit-indent marker unfixed when its
  final spaces carry value and no preserving marker append is available.
  ([#712](https://github.com/owenlamont/ryl/pull/712),
  [#715](https://github.com/owenlamont/ryl/pull/715))
- ryl TOML now separates `[lint]` and `[format]`. Run `ryl --migrate-configs`
  to preview existing configs, then add `--migrate-write` to update them.
  ([#508](https://github.com/owenlamont/ryl/pull/508))
- Top-level TOML `ignore` and `ignore-from-file` are deprecated. Replace them with
  `exclude` and `exclude-from-file`, or use `--migrate-configs --migrate-write`.
  ([#608](https://github.com/owenlamont/ryl/pull/608))
- yamllint YAML configuration is deprecated. Convert project configs with
  `ryl --migrate-configs --migrate-write` and user-global config with
  `ryl --migrate-user-config --migrate-write`; a later release will remove YAML
  config loading. ([#562](https://github.com/owenlamont/ryl/pull/562))
- `-d` accepts inline TOML; legacy YAML preset names still work with a warning.
  ([#562](https://github.com/owenlamont/ryl/pull/562))
- `format` preserves document-start markers by default, including files migrated
  from yamllint's `default` preset.
  ([#584](https://github.com/owenlamont/ryl/pull/584))
- Editor formatting and Fix All warn when lint settings conflict with formatting.
  ([#615](https://github.com/owenlamont/ryl/pull/615))
- `quoted-strings` and `format` keep quotes needed by YAML 1.1 readers, while a
  declared `%YAML 1.1` follows the lint policy for redundant quotes.
  ([#535](https://github.com/owenlamont/ryl/pull/535),
  [#560](https://github.com/owenlamont/ryl/pull/560),
  [#604](https://github.com/owenlamont/ryl/pull/604))
- Formatter previews reuse formatting and indentation detection, reducing work
  on repeated `--check` and `--diff` passes.
  ([#561](https://github.com/owenlamont/ryl/pull/561),
  [#616](https://github.com/owenlamont/ryl/pull/616))

### Fixed

- `comments-indentation` now reports, and `--fix` moves, a comment indented to the
  continuation of a multi-line quoted scalar, matching yamllint.
- A leading shebang or `#cloud-config` stays before an inserted document marker.
  ([#531](https://github.com/owenlamont/ryl/pull/531))
- `check --fix` and `format` add requested start and end markers to each document
  in a stream. ([#566](https://github.com/owenlamont/ryl/pull/566),
  [#587](https://github.com/owenlamont/ryl/pull/587))
- Editor diagnostics and fix actions stay positioned after a leading BOM.
  ([#588](https://github.com/owenlamont/ryl/pull/588))
- `quoted-strings --fix` keeps quotes on flow keys whose colon follows whitespace
  or a comment, preserving the key's meaning.
  ([#555](https://github.com/owenlamont/ryl/pull/555))
- Fixes preserve blank block-scalar content and scalar values when headers have
  comments; editor disable-line actions work inside blank-only scalars.
  ([#542](https://github.com/owenlamont/ryl/pull/542),
  [#543](https://github.com/owenlamont/ryl/pull/543),
  [#563](https://github.com/owenlamont/ryl/pull/563),
  [#573](https://github.com/owenlamont/ryl/pull/573))
- Formatting leaves lines after empty folded scalars intact and retains comments
  on dash lines. ([#559](https://github.com/owenlamont/ryl/pull/559),
  [#569](https://github.com/owenlamont/ryl/pull/569))
- Block-scalar diagnostics no longer misread an apostrophe in a plain key as a
  quote. ([#571](https://github.com/owenlamont/ryl/pull/571))
- Formatting aligns block-scalar bodies and detects indentation around document
  BOMs and explicit keys. ([#591](https://github.com/owenlamont/ryl/pull/591),
  [#626](https://github.com/owenlamont/ryl/pull/626),
  [#664](https://github.com/owenlamont/ryl/pull/664))
- Multiline double-quoted scalars can use single quotes when no required escapes
  prevent conversion. ([#662](https://github.com/owenlamont/ryl/pull/662))
- Formatting preserves encoded stdin and BOMs; encoded `check --diff` input counts
  pending fixes. ([#617](https://github.com/owenlamont/ryl/pull/617),
  [#620](https://github.com/owenlamont/ryl/pull/620))
- `format --check` writes empty reports when no files or changes are selected.
  ([#614](https://github.com/owenlamont/ryl/pull/614))
- Formatter previews reject conflicting output targets before reading inputs.
  ([#631](https://github.com/owenlamont/ryl/pull/631))
- Config migration leaves YAML sources shadowed by active TOML in place.
  ([#632](https://github.com/owenlamont/ryl/pull/632))
- Config migration preserves disabled formatting choices and handles custom YAML
  paths without guessing an ambiguous user-global ignore file.
  ([#556](https://github.com/owenlamont/ryl/pull/556),
  [#557](https://github.com/owenlamont/ryl/pull/557))
- Path matching respects Windows and macOS ASCII casing, and Windows diff paths apply
  with `git apply -p0`. ([#653](https://github.com/owenlamont/ryl/pull/653),
  [#655](https://github.com/owenlamont/ryl/pull/655))
- Formatting preserves Markdown fence boundaries and block scalar values while
  normalizing safe whitespace. ([#633](https://github.com/owenlamont/ryl/pull/633),
  [#654](https://github.com/owenlamont/ryl/pull/654),
  [#661](https://github.com/owenlamont/ryl/pull/661),
  [#669](https://github.com/owenlamont/ryl/pull/669))

### Migrating from yamllint

The new formatter has no yamllint counterpart. For `check`, the existing deliberate
differences are listed below so a migration can account for each one; see the
[examples and rationale](https://ryl-docs.pages.dev/getting-started/migrating-from-yamllint/#how-ryl-differs-from-yamllint).

| Difference | Migration step |
| :--- | :--- |
| No implicit default rules | Enable `--enable ALL` or a TOML preset. |
| Inline directives | Use `# ryl`; keep `# yamllint` standalone. |
| Colon in anchor names | Review `:` in names; ryl follows YAML grammar. |
| Duplicate input paths | Expect one check per file, even when inputs overlap. |
| Bare `\r` line breaks | Normalize to LF or CRLF if relying on yamllint's line count. |
| Document-prefix BOMs | Remove embedded BOMs if another parser rejects them. |
| CRLF scalar blanks | Review block-scalar indentation. |
| `%YAML` directives | Set the version or quote sensitive values. |
| Block scalar header comments | Review `comments` diagnostics. |
| `per-line-ignores` | Configure the ryl-only option in TOML, not yamllint YAML. |
| Open-block comments | Keep default or set `allow-any-open-indent` in TOML. |
| Own-line dash | Keep default or set `dash-on-own-line` in TOML. |
| Inline-comment gap | Keep default or set `max-spaces-from-content` in TOML. |
| JUnit and GitLab reports | Select ryl's output format for CI. |
| ryl-native user-global config | Run `ryl --migrate-user-config --migrate-write`. |
| `YAMLLINT_CONFIG_FILE` rejects TOML | Use YAML, or pass TOML with `-c`. |

## v0.24.0 - 2026-10-05

### Added

- `--enable ALL` enables every rule; `key-ordering` accepts custom key orders and
  sorts attached comments during `--fix`.
  ([#490](https://github.com/owenlamont/ryl/pull/490),
  [#494](https://github.com/owenlamont/ryl/pull/494),
  [#492](https://github.com/owenlamont/ryl/pull/492))
- `comments` accepts `max-spaces-from-content`; `truthy --fix` can recase boolean
  spellings. ([#489](https://github.com/owenlamont/ryl/pull/489),
  [#493](https://github.com/owenlamont/ryl/pull/493))
- `# ryl` directives work beside other inline comments.
  ([#488](https://github.com/owenlamont/ryl/pull/488))

### Fixed

- Corrected indentation diagnostics and ancestor config discovery when run from a
  subdirectory. ([#483](https://github.com/owenlamont/ryl/pull/483),
  [#473](https://github.com/owenlamont/ryl/pull/473))
- Counted `--fix` changes per rule and printed TOML parse errors on one line.
  ([#498](https://github.com/owenlamont/ryl/pull/498),
  [#497](https://github.com/owenlamont/ryl/pull/497))

## v0.23.0 - 2026-10-04

### Changed

- Bare `ryl <paths>` now warns; use `ryl check <paths>`.
  ([#461](https://github.com/owenlamont/ryl/pull/461))

### Fixed

- `--fix` repeats until stable and reports a pipeline that cannot converge.
  ([#465](https://github.com/owenlamont/ryl/pull/465),
  [#467](https://github.com/owenlamont/ryl/pull/467))
- Fixed `ignore` matching for absolute paths and `per-file-ignores` matching for
  resolved paths. ([#459](https://github.com/owenlamont/ryl/pull/459),
  [#460](https://github.com/owenlamont/ryl/pull/460))
- Rejected TOML-only keys in yamllint-compatible YAML configs.
  ([#441](https://github.com/owenlamont/ryl/pull/441))

## v0.22.0 - 2026-09-06

### Fixed

- The language server keeps idle `workspace/diagnostic` requests open for changes.
  ([#424](https://github.com/owenlamont/ryl/pull/424))

## v0.21.0 - 2026-06-22

### Added

- `ryl check` explicitly selects linting; bare `ryl <paths>` still works.
  ([#370](https://github.com/owenlamont/ryl/pull/370))

## v0.20.0 - 2026-06-21

### Added

- Rules respect a document's `%YAML` version directive.
  ([#363](https://github.com/owenlamont/ryl/pull/363))

### Fixed

- Closed a `%TAG`-split tag bypass in the `tags` rule.
  ([#356](https://github.com/owenlamont/ryl/pull/356))

## v0.19.1 - 2026-06-18

- Re-enabled winget publishing for Windows installs.
  ([#338](https://github.com/owenlamont/ryl/pull/338))

## v0.19.0 - 2026-06-16

### Added

- Discover ryl configs under a repository's `.config/` directory.
  ([#218](https://github.com/owenlamont/ryl/issues/218))

### Changed

- `YAMLLINT_CONFIG_FILE` accepts only yamllint YAML configs; use `-c` for TOML.
  ([#333](https://github.com/owenlamont/ryl/pull/333))

## v0.18.1 - 2026-06-15

### Fixed

- The language server sends one diagnostics channel per client.
  ([#323](https://github.com/owenlamont/ryl/pull/323))

## v0.18.0 - 2026-06-15

### Added

- `ryl server` provides language-server diagnostics and editor actions.
  ([#316](https://github.com/owenlamont/ryl/pull/316),
  [#318](https://github.com/owenlamont/ryl/pull/318))

## v0.17.0 - 2026-06-14

### Added

- Added shell completions via `--generate-completions`.
  ([#300](https://github.com/owenlamont/ryl/pull/300))
- Added JUnit XML and GitLab code-quality output, plus repeatable output targets.
  ([#301](https://github.com/owenlamont/ryl/pull/301),
  [#303](https://github.com/owenlamont/ryl/pull/303))
- Added ryl-native user-global config and `--migrate-user-config`.
  ([#307](https://github.com/owenlamont/ryl/pull/307))

## v0.16.0 - 2026-06-12

### Added

- Added `per-line-ignores`, `comments-indentation: allow-any-open-indent`, and
  `hyphens: dash-on-own-line`.
  ([#288](https://github.com/owenlamont/ryl/pull/288),
  [#293](https://github.com/owenlamont/ryl/pull/293))

### Fixed

- Treat a bare carriage return as a YAML 1.2 line break.
  ([#287](https://github.com/owenlamont/ryl/pull/287))

## v0.15.0 - 2026-06-09

### Added

- `--diff` previews safe fixes as a unified diff; `block-scalar-chomping` checks
  scalar endings. ([#282](https://github.com/owenlamont/ryl/pull/282),
  [#286](https://github.com/owenlamont/ryl/pull/286))

## v0.14.0 - 2026-06-08

### Added

- Added `merge-keys` and `unicode-line-breaks` rules.
  ([#276](https://github.com/owenlamont/ryl/pull/276),
  [#270](https://github.com/owenlamont/ryl/pull/270))
- `anchors` can flag ambiguous names; `key-duplicates` detects canonical keys
  and merge collisions.
  ([#275](https://github.com/owenlamont/ryl/pull/275),
  [#267](https://github.com/owenlamont/ryl/pull/267))

### Fixed

- Corrected a `colons` false positive on alias mapping keys.
  ([#273](https://github.com/owenlamont/ryl/pull/273))

## v0.13.0 - 2026-06-05

### Added

- Added the opt-in `tags` rule for unsafe and non-portable YAML tags.
  ([#262](https://github.com/owenlamont/ryl/pull/262))

## v0.12.0 - 2026-06-03

### Changed

- A config must explicitly enable at least one rule; ryl no longer applies the
  default preset automatically.
  ([#250](https://github.com/owenlamont/ryl/pull/250))

### Fixed

- Limited the effect of malicious YAML payloads.
  ([#249](https://github.com/owenlamont/ryl/pull/249))

## v0.11.0 - 2026-05-31

### Added

- Lint YAML embedded in Markdown, including stdin and `--fix` write-back; use
  `--markdown` to select Markdown input.
  ([#237](https://github.com/owenlamont/ryl/pull/237),
  [#241](https://github.com/owenlamont/ryl/pull/241))
- Inline directives can disable selected rules or files.
  ([#243](https://github.com/owenlamont/ryl/pull/243))

## v0.10.2 - 2026-05-29

### Fixed

- Corrected character-column and byte-offset handling across rules.
  ([#234](https://github.com/owenlamont/ryl/pull/234))

## v0.10.1 - 2026-05-27

No user-facing changes; the YAML parser implementation changed.

## v0.10.0 - 2026-05-24

### Added

- Read YAML from stdin with `-`.
  ([#225](https://github.com/owenlamont/ryl/pull/225))
- Added partial safe fixes for trailing spaces, document markers, and empty lines.
  ([#223](https://github.com/owenlamont/ryl/pull/223))

## v0.9.2 - 2026-05-18

### Fixed

- Restricted published package contents to runtime files.
  ([#216](https://github.com/owenlamont/ryl/pull/216))

## v0.9.1 - 2026-05-04

### Fixed

- Corrected `quoted-strings` autofix edge cases.
  ([#207](https://github.com/owenlamont/ryl/pull/207))

## v0.9.0 - 2026-05-03

### Added

- `quoted-strings` supports safe `--fix` rewrites.
  ([#202](https://github.com/owenlamont/ryl/pull/202))

## v0.8.0 - 2026-04-29

### Added

- Added `per-file-ignores` configuration.
  ([#200](https://github.com/owenlamont/ryl/pull/200))

### Fixed

- `quoted-strings` with single quotes allows double-quoted escape sequences.
  ([#201](https://github.com/owenlamont/ryl/pull/201))

## v0.7.0 - 2026-04-27

### Added

- Published a JSON Schema for `.ryl.toml` configuration and connected it to
  SchemaStore.
  ([#195](https://github.com/owenlamont/ryl/pull/195),
  [#196](https://github.com/owenlamont/ryl/pull/196))

## v0.6.0 - 2026-04-12

### Added

- Added more safe `--fix` rules and support for root sequences.
  ([#179](https://github.com/owenlamont/ryl/pull/179),
  [#175](https://github.com/owenlamont/ryl/pull/175))

## v0.5.0 - 2026-04-07

### Added

- `--fix` applies the first set of safe lint fixes.
  ([#171](https://github.com/owenlamont/ryl/pull/171))

## v0.4.4 - 2026-04-06

### Fixed

- Corrected the npm package executable bit.
  ([#169](https://github.com/owenlamont/ryl/pull/169))

## v0.4.3 - 2026-04-06

### Fixed

- Corrected the npm trusted-publishing runtime.
  ([#168](https://github.com/owenlamont/ryl/pull/168))

## v0.4.2 - 2026-04-06

### Added

- Added npm package publishing.
  ([#167](https://github.com/owenlamont/ryl/pull/167))

## v0.4.1 - 2026-03-04

### Fixed

- `indentation` detects unindented sequence entries with mapping values.
  ([v0.4.1 release](https://github.com/owenlamont/ryl/releases/tag/v0.4.1))

## v0.4.0 - 2026-03-02

### Added

- Added TOML configuration support.
  ([#148](https://github.com/owenlamont/ryl/pull/148))

### Fixed

- `ignore-from-file` re-inclusion matches yamllint.
  ([#146](https://github.com/owenlamont/ryl/pull/146))

## v0.3.6 - 2026-03-02

No user-facing changes; release automation was updated.

## v0.3.5 - 2026-03-01

### Fixed

- Corrected lint parity regressions found against yamllint.
  ([#143](https://github.com/owenlamont/ryl/pull/143))

## v0.3.4 - 2026-02-28

### Added

- Published a performance benchmark comparison with yamllint in the README.
  ([#138](https://github.com/owenlamont/ryl/pull/138))

## v0.3.3 - 2026-02-28

No user-facing changes; release publishing was repaired.

## v0.3.2 - 2026-02-28

### Fixed

- Improved yamllint parity for comments, indentation, document-end and truthy
  diagnostics. ([#87](https://github.com/owenlamont/ryl/pull/87))

## v0.3.1 - 2025-10-12

### Fixed

- Corrected a false undeclared-alias diagnostic.
  ([#83](https://github.com/owenlamont/ryl/pull/83))

## v0.3.0 - 2025-10-12

### Added

- Added configuration discovery and CLI lint commands.
  ([#17](https://github.com/owenlamont/ryl/pull/17))
- Added yamllint-compatible rules for YAML structure, spacing, comments, scalar
  values, ordering and document markers.
  ([v0.3.0 tag](https://github.com/owenlamont/ryl/releases/tag/v0.3.0))
- Added file-encoding support and the `anchors` rule.
  ([#76](https://github.com/owenlamont/ryl/pull/76),
  [#74](https://github.com/owenlamont/ryl/pull/74))

## v0.2.0 - 2025-09-10

No user-facing changes; publishing and coverage automation were updated.

## v0.1.0 - 2025-09-07

### Added

- First release: search for YAML files and parse them.
  ([#7](https://github.com/owenlamont/ryl/pull/7))
