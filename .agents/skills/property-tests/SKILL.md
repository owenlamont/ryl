---
name: property-tests
description: >-
  Use when adding or changing a rule's detection or safe-fix behaviour, or
  editing any property-test suite (safe-fix / rule-checker / markdown-fix /
  config). Covers what each generator must be extended with, the ~1000x
  pre-commit run, and which rules intentionally have no safe `--fix`.
---

# Property Tests

When implementing a new rule or changing an existing one, extend the relevant
property-test generator(s) so the new/updated syntax is actually exercised (each suite
below lists exactly what to extend and the deterministic guard to add), then do a
one-off **~1000× thorough run** before committing: e.g.
`PROPTEST_CASES=512000 cargo test --release --test property_check` (the suites'
in-CI default is 512 cases — tuned for speed, not exhaustiveness). Build `--release`
and run it in the background; it routinely flushes rare interleavings the small count
misses. Commit only once it is green, and keep any newly-persisted seeds in
`tests/proptest-regressions/`.

## Property Tests For Safe Fixes

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

## Property Tests For Rule Checkers

`tests/property_check.rs` property-tests the **detection** path: it runs every rule's
`check()` over generated YAML and asserts oracle-free invariants — `check()` never
panics, every span is in-bounds and **character-aligned** (`1 <= line <= line_count`,
`1 <= column <= chars_on_line + 1`), a leading `# ryl disable` mutes every rule (only a
syntax error survives), and block-disabling a firing rule removes its diagnostics. It
targets ryl's fragile byte↔char offset arithmetic rather than semantic correctness (the
fast complement to the slow `yamllint_compat_*` differential suite).
`property_check/strategy.rs` generates documents biased to trigger every rule (truthy
words, octal/float scalars, duplicate/unordered keys, flow spacing, anchors, long lines,
odd indentation, trailing spaces, `%YAML` version directives) interleaved with multibyte
chars, raw NEL/LS/PS, and mixed LF/CRLF/bare-CR (a bare `\r` is a YAML 1.2 line break
everywhere, so the oracle `line_char_lengths` is CR-aware too). `harness.rs` holds the
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

## Property Tests For Markdown `--fix`

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

## Property Tests For Config Parsing

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

## Rules Without A Safe `--fix`

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
