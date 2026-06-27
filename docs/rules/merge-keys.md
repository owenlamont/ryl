# merge-keys

## What this rule does

Reports the `<<` merge key. A merge key splices the keys of another mapping into
the current one:

```yaml
defaults: &defaults
  retries: 3
  timeout: 30
prod:
  <<: *defaults       # prod gains retries: 3 and timeout: 30
  host: prod.example.com
```

When enabled the rule flags every `<<` key, whether its value is an alias
(`<<: *defaults`), an inline mapping (`<<: {a: 1}`), or a list of either
(`<<: [*a, *b]`). A key explicitly tagged `!!merge` is also flagged regardless of
its text (`!!merge foo:` merges in the same loaders that honour `<<`). It is
**off by default**.

## Why this matters

The merge key is a YAML **1.1** type. YAML **1.2**
removed it: "The merge `<<` and value `=` special mapping keys have been removed"
(YAML 1.2.2 changes page). ryl resolves scalars under the YAML 1.2 core schema,
where `<<` is an ordinary string key with no special meaning.

That makes `<<` a portability trap. Whether `<<: *defaults` actually merges
anything depends entirely on the parsing library: some still honour the 1.1 merge,
others treat `<<` as a literal key, so the same document can produce a different
mapping depending on the tool. A portability-sensitive repository can enable this
rule to forbid the construct outright.

A quoted `"<<"` is **not** flagged: quoting turns it into a plain string key that
never merges in any parser (verified against PyYAML and ruamel.yaml), so it is not
a portability hazard. Note that quoting is therefore not a way to *keep* a merge —
it removes the merge behaviour entirely.

Sources: YAML 1.2.2 changes page; YAML merge type.

## Configuration

`merge-keys` is a ryl-only rule (yamllint has no equivalent), so it is configured
**only in TOML** &mdash; `[rules.merge-keys]` in `.ryl.toml`/`ryl.toml` or
`[tool.ryl.rules.merge-keys]` in `pyproject.toml`. It is rejected in
yamllint-compatible YAML config (including `-d` data) so the YAML namespace stays
reserved for any future yamllint rule.

```toml
[rules.merge-keys]
level = "error"
```

The rule has no options: when enabled it flags every `<<` merge key.

## Examples

### :x: Reported

```yaml
defaults: &defaults
  retries: 3
prod:
  <<: *defaults
  host: prod.example.com
```

```text
4:3  error  forbidden merge key "<<"  (merge-keys)
```

### :white_check_mark: Allowed

Write the keys explicitly instead of merging:

```yaml
defaults: &defaults
  retries: 3
prod:
  retries: 3
  host: prod.example.com
```

## Automatic fixing

This rule does not auto-fix. Removing a merge requires inlining the merged
mapping's resolved keys and values (which the source text alone does not carry),
and quoting the `<<` would silently drop the merge; no single rewrite is
universally safe.

## Related rules

- [`key-duplicates`](key-duplicates.md) &mdash; its `forbid-duplicated-merge-keys`
  option targets a *repeated* `<<` in one mapping, and its `check-canonical` /
  `forbid-merge-key-shadowing` options flag merges that silently change a key's
  value. `merge-keys` instead forbids *any* use of `<<`.
- [`anchors`](anchors.md) &mdash; governs the anchors and aliases (`&`/`*`) a
  merge typically relies on.
