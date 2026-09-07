---
name: payload-hardening
description: >-
  Use when changing `--fix`/`--diff` write paths, the YAML config loader, or any
  output format — the invariants that keep a hostile YAML payload from writing
  through a symlink, exhausting memory, or injecting into a report. Lists the
  guard tests that pin each one.
---

# Malicious-Payload Hardening

ryl's threat model (#246) is realistic payloads, not a concurrent filesystem racer.
Invariants to preserve:

## Writes

`--fix`/`--diff` never write/read through a symlink (`fix::refuse_symlink`) and the write
target is always the input path, never derived from YAML content.

## Config loading

The YAML config loader (`yaml_dom::loader`; `lint_str` builds no DOM) bounds alias
expansion at `MAX_EXPANDED_NODES` and `extends` depth at `MAX_EXTENDS_DEPTH`, so
billion-laughs and cyclic-`extends` configs error instead of exhausting memory/stack. An
empty YAML/TOML config errors ("not a mapping" / "configuration is empty") rather than
silently linting nothing. granit caps nesting recursion (~256), and config regexes
(`key-ordering`/`quoted-strings`) are validated at parse time with the linear-time
`regex` crate (no ReDoS).

## Output

The GitHub format escapes user text (`github_escape_data`/`_property`) so a crafted
key/anchor/filename can't inject a `::command::` (it is a line-oriented command
protocol); the streaming console formats run user text through `sanitize_control`.

The `junit`/`gitlab` report formats are structured data, not command protocols, so the
analogous risk is breaking out of an XML attribute / JSON string: `sanitize_control`
first strips control chars, then `quick-xml` (XML) and `serde_json` (JSON) apply
structural escaping, and fixed fields (`severity`, `check_name`, the testcase `name`) are
derived from the rule/level, not the message. `tests/property_report.rs` fuzzes this
(every output must stay well-formed XML / schema-valid JSON under hostile input).

## Guards

`tests/cli_alias_bomb.rs`, `cli_fix_symlink.rs`, `cli_config_data_error.rs`,
`cli_toml_config.rs`, `config_extends_inline.rs`, `cli_format_options.rs`,
`cli_markdown_embed.rs`, `property_config.rs`, `report_formats.rs`, `property_report.rs`.

## What not to chase

Codex's adversarial review escalates indefinitely on file I/O (TOCTOU, partial or
interrupted writes, cross-file non-atomicity). Converge on real bugs, document the rest
as known limitations, and ship; do not re-introduce atomic temp+rename via a runtime
`tempfile` dep (tried on #285, reverted: 0600-perms regression + `clippy::cargo
multiple_crate_versions`).
