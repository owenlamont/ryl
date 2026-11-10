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

Rewrite/diff passes refuse symlinks (`fix::symlink_notice`) and never write through
them; `check --fix` still lints the readable target. The write target is always the
input path, never derived from YAML content.

## Config loading

The YAML config loader (`yaml_dom::loader`; `lint_str` builds no DOM) bounds alias
expansion at `MAX_EXPANDED_NODES` and `extends` depth at `MAX_EXTENDS_DEPTH`, so
billion-laughs and cyclic-`extends` configs error instead of exhausting memory/stack. An
empty YAML/TOML config errors ("not a mapping" / "configuration is empty") rather than
silently linting nothing. The parser caps nesting recursion (~256), and config regexes
(`key-ordering`/`quoted-strings`) are validated at parse time with the linear-time
`regex` crate (no ReDoS).

## Output

The GitHub format escapes user text (`github_escape_data`/`_property`) so a crafted
key/anchor/filename can't inject a `::command::` (it is a line-oriented command
protocol); the streaming console formats run user text through `sanitize_control`.

The `junit`/`gitlab` report formats are structured data, not command protocols, so the
analogous risk is breaking out of an XML attribute / JSON string: `sanitize_control`
first escapes control chars, then `quick-xml` (XML) and `serde_json` (JSON) apply
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

## Local JSON Schema validation

`rules::schema` prepares validators during config finalization, before cache lookup.
Root and referenced files use the same canonical-path confinement and bounded reader:
16 MiB total, 1,024 resources, no UNC paths. Distinct URI aliases
count separately toward both limits. Regex uses the
bounded fancy-regex engine. Before compilation, expanded schema weight is capped at
1,000,000, counting declarations and resolved references with stack back-edges worth one.
Recursive components must descend through the instance and cannot have overlapping
recursive branches; distinct literal properties and disjoint object/array descent pass,
while repeated recursive tuple items are refused. Refusals use the config-error path.
Projection charges source nodes, alias and merge expansion,
scalar bytes and pointer storage before growing unbounded state. Limits are 1,000,000 nodes,
16 MiB and depth 128; output caps at 100 findings plus a truncation note.
Schema content digests live in resolved-config Debug only when associations exist;
keep the schema-free Debug unchanged. `cli_schema_rule` guards the boundaries and cache
invalidation after an unmatched file; `property_schema` exercises projection and spans.

## Remote JSON Schemas

`rules::schema::net` owns HTTP and the user schema cache. Project URLs request
resources; only CLI flags, launcher environment and the user allowlist grant origins.
Every physical retrieval needs an exact HTTPS origin grant, including offline and
after `$id` rebasing. Remote roots have no file capability. Retain recorded retriever
failures: referencing can swallow failed `$schema` fetches.

The bounded resolver worker checks every OS answer before ureq truncates to 16;
connections use only checked addresses and retain the URL TLS name. Deny private,
reserved and transition addresses, disable proxies/redirects/pooling, use bundled
webpki roots, and cap requests, deadlines, headers and identity-encoded bodies.

Cache keys hash canonical full URLs; verify content hashes and pins on every read.
Offline never resolves DNS or refreshes timestamps. Refuse symlinks, write with
create-new temporary files and rename, and evict oldest entries above 256 MiB.
Resource budgets count cache reads and repeated associations. Network output redacts
query values; `sanitize_control` also escapes bidi controls. The server grants origins
only from the launcher, user allowlist or explicit session approval. Project settings
cannot approve access. Prompts and approval actions use canonical origins and redacted,
sanitised URLs. Denial/dismissal/timeout never fetch. Offline never prompts.

The LSP preparation worker is lazy, bounded and shared with workspace scans. Cancellation
and enqueue deadlines apply before retrieval; config revisions supersede queued work.
User allowlist updates create the directory, refuse symlink components and atomically
rename a create-new temporary file.
Concurrent allowlist updates serialize through a sibling advisory lock held across the
read-modify-rename; lock acquisition has a 250 ms deadline and failure retains session access.
A wedged DNS worker can be replaced once while stuck; at most two resolver threads exist,
and further saturation fails visibly.
Transport, resolver, clock and cache filesystem injections are trusted Rust test
seams; never expose policy bypasses through project configuration or launcher flags.
