---
name: adding-a-rule
description: >-
  Use when adding a new ryl lint rule or changing an existing rule's wiring.
  Walks the multi-site checklist (rule module, registration, dispatch, TOML
  config, tests, docs) where a missed site usually trips a guard test rather
  than shipping silently.
---

# Adding a New Rule

A rule touches several disconnected sites; a missing one usually fails a guard test
rather than shipping silently. Work this checklist (the `property-tests` and
`coverage` dev skills expand steps 5–6):

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
   TOML config; see the YAML-vs-TOML note in `AGENTS.md`).
3. **Dispatch**: one `lint_rule!(...)` call in `src/lint.rs`, in the right
   reported-order slot of the matching batch fn (`collect_layout` / `collect_value` /
   `collect_block_diagnostics`). Pick the arm matching the rule's shape (config or not,
   `Vec`/`Option`, per-violation or fixed `MESSAGE`).
4. **TOML config wiring** (`src/config_schema.rs` + `config_schema/serialization.rs`):
   a `RuleName` variant + `as_str` arm, a `RulesTable` field with its `…Options` type,
   and the `insert_serialized` line in `rules_table_to_value`. These four parallel lists
   have no compile-time cross-check; the `every_rule_round_trips_through_toml_serialization`
   guard test catches a forgotten serialization line. Regenerate the committed
   `ryl.{toml,yaml}.schema.json` (see *Testing Tips* in `AGENTS.md`) and run `prek`.
5. **Tests**: add the rule to `property_check`'s `collect_spans` + a `RULE_TRIGGERS`
   row; if it has a safe `--fix`, also `SAFE_FIX_RULES` and the safe-fix generator. Add
   a CLI test `tests/cli_<rule>_rule.rs` (use the shared `common::cli` harness) and an
   embedded-markdown regression test in `tests/cli_markdown_embed.rs`.
6. **Docs**: a `docs/rules/<rule>.md` page + the index, and a "How ryl differs from
   yamllint" entry for any deliberate divergence. When a doc page shows example CLI
   output, produce the `line:col`/message by **running ryl** on the shown input — not by
   hand (hand-written examples have shipped with wrong columns).

## granit event/span gotchas (granit-parser 0.0.5)

Facts the codebase relies on; re-verify on a granit bump:

- Derive a token's position from `marker.byte_offset()` via
  `crate::rules::support::span_utils` (the pattern every span-using rule follows), rather
  than reading offsets off the raw `Span` by hand.
- Any rule reading granit token/event line numbers must index lines split the
  granit-aligned way (`\r\n|\r|\n`, via `line_syntax`), never `\n`-only: a bare `\r`
  is a line break to granit, so a `\n`-only split desyncs and can panic out-of-bounds.
- For an indentation/column-sensitive rule, derive structure from granit events from the
  start; a line-based `classify_mapping` is YAML-unsound on colons-in-scalars, quoted
  escapes, and multiline plain scalars (the comments-indentation rewrite learned this over
  ~5 review rounds).
- Matching a core-schema tag (`!!int`, …): use `crate::yaml_dom::core_schema_suffix` /
  `is_core_schema`, **never** granit's handle-only `Tag::is_yaml_core_schema` (a verbatim
  `!<tag:yaml.org,2002:int>` slips past it); compare the full resolved URI when a `%TAG`
  can split it, as `support::merge_key` does.
