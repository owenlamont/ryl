# Design decisions

These are current, load-bearing choices from ryl's issues, pull requests, and repository history. Each link gives the ruling or implementation; a replaced choice is named where the change matters. [Scope](../scope.md) records declined requests, and the [migration catalog](../getting-started/migrating-from-yamllint.md#how-ryl-differs-from-yamllint) records compatibility differences.

## Configuration

- **Rules require explicit enablement.** A missing config or one enabling no rules exits 2, so an unconfigured run cannot silently apply a preset or lint nothing. [Decision](https://github.com/owenlamont/ryl/issues/248)
- **TOML is the native configuration model.** Shared settings live at the top level, lint under `[lint]`, and formatter targets under `[format]`. The nested model replaced flat top-level lint keys; migration warns and rewrites them. [Epic](https://github.com/owenlamont/ryl/issues/374), [implementation](https://github.com/owenlamont/ryl/pull/508)
- **TOML configuration stays local.** One selected config governs a file; TOML has no `extends` inheritance or implicit preset selection. [Maintainer decision](https://github.com/owenlamont/ryl/issues/291)
- **Legacy YAML configuration follows yamllint's surface.** Ryl-only settings and rules belong in TOML; YAML rejects unknown rule names and TOML-only keys rather than silently ignoring them. [Decision](https://github.com/owenlamont/ryl/issues/191), [validation](https://github.com/owenlamont/ryl/pull/441)
- **Legacy YAML configuration is deprecated.** Conversion remains available while warnings name a usable migration command; the removal release is not yet set. This replaced treating YAML as a permanent peer to native TOML. [Decision](https://github.com/owenlamont/ryl/issues/478), [warning](https://github.com/owenlamont/ryl/pull/562)
- **Deprecations need a usable target.** A warning starts only after its replacement exists, and stale configuration must never silently stop applying. [Formatter epic](https://github.com/owenlamont/ryl/issues/374)
- **Shared `indent-width` and `line-length` are defaults.** Formatter targets and lint defaults agree unless an explicit per-rule lint option overrides them; quote style remains formatter-owned. [Decision](https://github.com/owenlamont/ryl/issues/374), [implementation](https://github.com/owenlamont/ryl/pull/521)
- **Discovery prefers the selected project config.** Explicit CLI config wins first; project discovery precedes `YAMLLINT_CONFIG_FILE`, then native and yamllint-compatible user-global locations. [Discovery](https://github.com/owenlamont/ryl/pull/307)
- **Project discovery walks ancestors and `.config/`.** A run from a subdirectory finds the governing project config, with native TOML candidates preferred in the documented order. [Ancestor fix](https://github.com/owenlamont/ryl/pull/473), [`.config/` support](https://github.com/owenlamont/ryl/pull/331)
- **`YAMLLINT_CONFIG_FILE` accepts yamllint YAML only.** Native TOML uses `-c` or project discovery, keeping the compatibility variable's meaning unambiguous. [Decision](https://github.com/owenlamont/ryl/pull/333)
- **Ryl has a native user-global TOML path.** The native path takes precedence over the yamllint-compatible user-global YAML file when no project config governs the input. [Decision](https://github.com/owenlamont/ryl/pull/307)
- **Invalid configuration fails visibly.** Unknown TOML keys or rule names are errors with exit 2, and the generated schema rejects them too. [Rule validation](https://github.com/owenlamont/ryl/pull/249), [TOML validation](https://github.com/owenlamont/ryl/pull/308)
- **`ALL` and `--enable` are explicit shortcuts.** They select all rules or a command-line set without an implicit default preset. [Decision](https://github.com/owenlamont/ryl/pull/490)
- **Per-file and per-line ignores are TOML-backed suppression.** Path patterns and source-line regexes select where findings are ignored; the same suppression also governs safe fixes. [Per-file](https://github.com/owenlamont/ryl/pull/200), [per-line](https://github.com/owenlamont/ryl/pull/288)
- **Markdown linting is opt-in.** YAML and Markdown are separate source kinds; each embedded YAML region is checked independently. [Decision](https://github.com/owenlamont/ryl/pull/237)
- **YAML config aliases have a bound.** The loader rejects expansion that would amplify a hostile config into an unbounded graph. [Hardening](https://github.com/owenlamont/ryl/pull/249)
- **Migration does not replace an active config.** A TOML config shadowing legacy YAML leaves that inactive YAML source untouched. [Decision](https://github.com/owenlamont/ryl/pull/632)
- **Migration flattens relative `extends`.** It checks the config directory before the current directory; a duplicate filename can therefore select different content from lint time. [Review decision](https://github.com/owenlamont/ryl/pull/307)
- **User-global migration refuses relative `ignore-from-file`.** Its meaning depends on each linted file's directory, so one relocated TOML config cannot preserve it. [Decision](https://github.com/owenlamont/ryl/pull/630), [guide](../getting-started/migrating-from-yamllint.md#what-is-preserved)
- **Migration preserves disabled formatting intent.** A legacy rule that did not enforce a layout choice becomes an appropriate preserve target, avoiding a new formatting change. [Decision](https://github.com/owenlamont/ryl/pull/556)
- **Top-level TOML file exclusion uses `exclude`.** `ignore` and `ignore-from-file` are deprecated there, while yamllint YAML and per-rule ignore spellings retain their meanings. [Decision](https://github.com/owenlamont/ryl/pull/608)
- **Path matching follows the platform.** Configured paths compare ASCII case-insensitively on Windows and macOS and case-sensitively on Linux. [Decision](https://github.com/owenlamont/ryl/pull/653)
- **Documentation examples use the real config loader.** CI validates examples through finalized configuration so misspelled rules and options fail. [Decision](https://github.com/owenlamont/ryl/pull/357)

## Rules

- **Built-in rules cover general YAML with evidence.** Application-specific schemas, unsubstantiated best practices, and contested style remain outside the rule set. [Scope](../scope.md), [rules epic](https://github.com/owenlamont/ryl/issues/261)
- **Ryl-only rules are TOML-only and opt-in.** Merge keys, tags, Unicode line breaks, and block-scalar chomping address portability or clarity without changing yamllint YAML config semantics. [Merge keys](https://github.com/owenlamont/ryl/pull/276), [tags](https://github.com/owenlamont/ryl/pull/272), [line breaks](https://github.com/owenlamont/ryl/pull/270), [chomping](https://github.com/owenlamont/ryl/pull/286)
- **Inline disable directives are shared across rules.** `# ryl` is preferred; `# yamllint` remains accepted for migration. [Decision](https://github.com/owenlamont/ryl/pull/243)
- **Canonical duplicate-key checks are opt-in.** YAML 1.2 core-schema identity can flag keys yamllint treats as different spellings, so TOML enables that semantic mode explicitly. [Decision](https://github.com/owenlamont/ryl/pull/267)
- **Tag allowlists use source spelling.** A user's non-core tag pattern matches what the author wrote, while unsafe-tag checks use the resolved identity. [Decision](https://github.com/owenlamont/ryl/pull/272)
- **Quoted-string checks separate keys from values.** Mapping values are checked by default; rewriting keys requires `check-keys = true`. [Decision](https://github.com/owenlamont/ryl/issues/290)
- **A single-quote preference can permit necessary escapes.** The double-quote exception is an opt-in TOML setting because some strings need escapes a single quote cannot express. [Decision](https://github.com/owenlamont/ryl/pull/201)
- **Truthy fixes do not guess intent.** A safe fix changes only case variants of the same YAML boolean, never `yes`, `no`, `on`, or `off`. [Decision](https://github.com/owenlamont/ryl/pull/493)
- **Key ordering remains a lint rule.** Its safe fix carries attached comments with a block mapping entry and leaves ambiguous cases unchanged. [Decision](https://github.com/owenlamont/ryl/pull/492)
- **Key orders may depend on file and mapping.** TOML can place listed keys first and choose whether unlisted keys sort or stay in place. [Decision](https://github.com/owenlamont/ryl/pull/494)
- **Layout diagnostics stay in `ryl check`.** The formatter also enforces layout and warns if an enabled lint tolerance rejects its target. This replaced the original plan to move layout diagnostics solely into formatting. [Maintainer re-review](https://github.com/owenlamont/ryl/issues/374)

## Formatter

- **Formatting uses local span edits, never whole-document reprinting.** Resolved values, comments, anchors, and untouched bytes are protected; unsafe changes are reported and skipped. [Formatter epic](https://github.com/owenlamont/ryl/issues/374), [scope](../scope.md)
- **Safe fixing promises soundness, not completeness.** A fix may leave a diagnostic behind; an ambiguous or unparsable region stays unchanged with a reason. [Decision](https://github.com/owenlamont/ryl/pull/270)
- **Meaning-changing repairs need explicit assumptions.** Default `--fix` preserves resolved values under a document's declared YAML version; an unsafe uplift would require opt-in configuration. [Decision](https://github.com/owenlamont/ryl/issues/365)
- **`ryl format` has its own pass list.** Formatter targets do not inherit lint rule selection, fixability settings, or lint ignores. [Decision](https://github.com/owenlamont/ryl/pull/520)
- **The formatter has built-in, overridable targets.** The linter remains explicit-rule-only; formatting works without a config and normalizes each option to a single target rather than a lint tolerance. [Epic](https://github.com/owenlamont/ryl/issues/374), [profile](https://github.com/owenlamont/ryl/issues/385)
- **Document-start markers default to preserve.** Existing marker style remains unless the user requests a new target, replacing the initial add-`---` default. [Decision](https://github.com/owenlamont/ryl/pull/584)
- **Unset indent width uses whole-file evidence.** Among widths 2–8, the formatter chooses the one changing the fewest lines; ties or no evidence use 2. [Maintainer decision](https://github.com/owenlamont/ryl/issues/594)
- **Line-ending auto uses the whole file.** The majority ending wins, a tie falls back to LF, and explicit LF remains the default. [Maintainer decision](https://github.com/owenlamont/ryl/issues/594)
- **Collection style has flat targets.** `[format]` can preserve, block-format, or flow-format sequences and mappings, converting only cases safe for their comments and values. [Decision](https://github.com/owenlamont/ryl/issues/446)
- **Quoting preserves cross-version values.** Quotes remain when removing them would change resolution, including version-sensitive `y` and `n` spellings. [Decision](https://github.com/owenlamont/ryl/pull/560), [correction](https://github.com/owenlamont/ryl/pull/604)
- **Markdown boundaries can veto one region.** Formatting refuses an edit that would close a YAML fence or front matter early, while continuing with safe neighboring regions. [Decision](https://github.com/owenlamont/ryl/pull/633)
- **Formatter guarantees have two gates.** Property tests check preservation and idempotence, and a pinned real-repository corpus blocks release on value change, panic, or comment loss. [Property suite](https://github.com/owenlamont/ryl/issues/379), [corpus](https://github.com/owenlamont/ryl/issues/386)

## CLI and exit codes

- **`ryl check` names linting.** Bare `ryl <paths>` still lints with a deprecation warning, giving existing callers time to migrate. [Command](https://github.com/owenlamont/ryl/pull/370), [warning](https://github.com/owenlamont/ryl/pull/461)
- **`ryl format` writes by default.** `--check` and `--diff` inspect without writing and exit 1 when formatting is pending; stdin formatting writes to stdout. [Decision](https://github.com/owenlamont/ryl/pull/519)
- **`check --diff` is a patch preview.** It leaves files alone, prints an applicable unified diff where possible, and exits 1 when a safe fix would change input. [Decision](https://github.com/owenlamont/ryl/pull/282)
- **`-` reads stdin with an optional filename.** `--stdin-filename` supplies the diagnostic label, config discovery context, and path-filter context. [Decision](https://github.com/owenlamont/ryl/issues/209)
- **Markdown fixes splice back into the host.** Each embedded YAML region is fixed separately, retaining surrounding Markdown and rejecting boundary-breaking replacements. [Decision](https://github.com/owenlamont/ryl/pull/241)
- **Symlinks are readable inputs but not fix destinations.** Ryl lints a symlinked file and refuses to write a safe fix through its symlink. [Decision](https://github.com/owenlamont/ryl/pull/249)
- **Input de-duplication is lexical.** Absolute paths are normalized without resolving symlinks, so distinct symlink paths remain distinct inputs. [Decision](https://github.com/owenlamont/ryl/pull/282)
- **Output destinations are repeatable pairs.** CLI format and file targets override `[output]`, which overrides the console default; conflicting destinations fail with exit 2. [Decision](https://github.com/owenlamont/ryl/pull/303)
- **GitLab fingerprints exclude location.** They hash path, rule, and message so a finding keeps its identity when line numbers move. [Decision](https://github.com/owenlamont/ryl/pull/301)
- **Safe fixes converge in one invocation.** Ryl repeats the fix pipeline to a fixed point, with a bounded pass count and a visible non-convergence error. [Decision](https://github.com/owenlamont/ryl/pull/467)
- **Encoding does not make a pending change invisible.** BOM and UTF-16/32 inputs preserve their encoding through formatting; `--check` and `check --diff` still report changes when a byte patch cannot be shown. [Format](https://github.com/owenlamont/ryl/pull/617), [check](https://github.com/owenlamont/ryl/pull/620)
- **A write failure does not stop later files.** Formatting reports failed destinations, continues with the rest, and exits 2. [Decision](https://github.com/owenlamont/ryl/pull/714)
- **Hook order gives formatting the final layout pass.** Run `check --fix` before `format` when both pre-commit hooks are enabled. [Decision](https://github.com/owenlamont/ryl/pull/705)

## Parser authority

- **One parser stack drives ryl.** Granit supplies YAML events; a small local DOM supports configuration without maintaining a second parser implementation. [Decision](https://github.com/owenlamont/ryl/pull/228)
- **YAML 1.2.2 grammar and the reference parser settle disagreements.** PyYAML and ruamel cannot outvote the reference event stream; eemeli's `yaml` is a cross-check. [Decision](https://github.com/owenlamont/ryl/pull/701)
- **Resolution uses the YAML 1.2 core schema.** Plain `011` and `11` are the same integer key, replacing a proposed JSON-schema string reading. Canonical checks now compare integers beyond `i64` by value, replacing the earlier limit. [Core decision](https://github.com/owenlamont/ryl/pull/267), [wide integers](https://github.com/owenlamont/ryl/pull/517)
- **Each document's `%YAML` directive matters.** Unversioned input uses 1.2; explicit 1.1 keeps version-sensitive values, a higher minor warns, and a higher major errors. [Decision](https://github.com/owenlamont/ryl/issues/351)
- **Core tags resolve across spellings.** Verbatim and shorthand tags carry the same core-schema meaning in duplicate-key checks, quotes, and configuration. [Decision](https://github.com/owenlamont/ryl/pull/278)
- **Bare CR is a YAML line break.** Rules and safe fixes share YAML 1.2's treatment instead of letting byte scanners disagree. [Decision](https://github.com/owenlamont/ryl/pull/287)
- **Documented divergences stay deliberate.** Ryl checks comments after block-scalar headers even where yamllint does not, because the comment is still part of the author's YAML source. [Decision](https://github.com/owenlamont/ryl/pull/496)

## Packaging and release

- **A public GitHub release is the final publication step.** Registry publishing must succeed first, replacing the earlier order that exposed the release before package feeds were ready. [Current decision](https://github.com/owenlamont/ryl/pull/133), [replaced order](https://github.com/owenlamont/ryl/pull/80)
- **Release archives carry verifiable provenance.** Publish checksums and GitHub attestations with binaries so users can check an artifact against the release. [Attestations](https://github.com/owenlamont/ryl/pull/129), [checksums](https://github.com/owenlamont/ryl/pull/325)
- **npm uses platform packages.** The `@owenlamont/ryl` package resolves a platform-specific binary instead of downloading one at installation time. [Decision](https://github.com/owenlamont/ryl/pull/168)
- **Conda-forge builds from source.** Its feedstock follows staged-recipes and receives subsequent version bumps from the conda-forge bot. [Decision](https://github.com/owenlamont/ryl/issues/298)
- **Winget follows a finalized release.** The release workflow submits x64 and arm64 archive URLs through `wingetcreate`. [Decision](https://github.com/owenlamont/ryl/pull/312)
- **Winget fork sync failures fail the release job.** A dedicated fork-scoped token replaced the earlier warning-only treatment after a release submission failed. [Current decision](https://github.com/owenlamont/ryl/pull/469), [replaced warning](https://github.com/owenlamont/ryl/pull/430)
- **Downstream editor pins update after release finalization.** The release dispatch notifies `ryl-vscode` once the public release exists. [Decision](https://github.com/owenlamont/ryl/pull/423)
- **The changelog supplies release notes.** Published notes come from the matching version section rather than ad hoc text, including on draft reruns. [Decision](https://github.com/owenlamont/ryl/pull/677)
- **Coverage is earned by named examples.** CI omits random property cases from coverage and replays saved seeds separately; local property runs retain their default counts. [Decision](https://github.com/owenlamont/ryl/pull/668)

## LSP and editor

- **`ryl server` reuses the check engine.** The default-on LSP feature is a thin protocol adapter; it complements the YAML schema server in an editor. [Decision](https://github.com/owenlamont/ryl/pull/316)
- **A client receives one diagnostics channel.** Pull-capable clients get pull diagnostics; push is used only when the client lacks pull support, avoiding duplicates. [Decision](https://github.com/owenlamont/ryl/pull/324)
- **Unchanged workspace pulls wait for a change.** Result IDs report unchanged files, and a fully idle workspace request waits for an edit, watched-file event, or config change. [Decision](https://github.com/owenlamont/ryl/pull/424)
- **Format Document uses `ryl format`.** Editor formatting follows the formatter even without enabled lint rules; Fix All stays a lint-fix action. [Decision](https://github.com/owenlamont/ryl/pull/583)
- **Conflict warnings appear only for explicit changes.** Formatting and Fix All may warn about clashing formatter and lint settings; diagnostics and quick fixes remain quiet. [Decision](https://github.com/owenlamont/ryl/pull/615)
