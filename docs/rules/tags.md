# tags

## What this rule does

Inspects the YAML tags attached to nodes (`!!python/object/apply`, `!!omap`,
`!env`, …) for safety and portability. The rule bundles three independent
checks, all **off by default**; enable only the ones your project needs.

## Why this matters

- **Unsafe construction tags.** Language-specific tags such as
  `!!python/object/apply`, `!ruby/object:`, or `!!java/…` drive
  arbitrary-object construction in some loaders. PyYAML's documentation warns
  that `yaml.load` "is as powerful as `pickle.load`" and recommends
  `safe_load`; authoring YAML that depends on such tags is a recognised
  anti-pattern.
- **Removed YAML 1.1 types.** `!!omap`, `!!pairs`, `!!set`, `!!timestamp`, and
  `!!binary` were removed in YAML 1.2. ryl targets YAML 1.2, so flagging them
  keeps documents portable.
- **Local / non-core tags.** Local tags such as `!env` or `!include` are
  application-specific and "may even have different semantics in different
  documents" (YAML 1.2.2 spec). An allowlist lets a team permit only the
  handles it actually uses.

Sources: YAML 1.2.2 spec (tags); YAML 1.2.2 changes page; PyYAML docs; The
YAML Company.

## Configuration

`tags` is a ryl-only rule (yamllint has no equivalent), so it is configured
**only in TOML** &mdash; `[rules.tags]` in `.ryl.toml`/`ryl.toml` or
`[tool.ryl.rules.tags]` in `pyproject.toml`. It is rejected in
yamllint-compatible YAML config (including `-d` data) so the YAML `tags`
namespace stays reserved for any future yamllint rule.

```toml
[rules.tags]
level = "error"
forbid-unsafe-tags = false
forbid-removed-types = false
allowed-tags = []
```

| Option | Default | Description |
| :--- | :--- | :--- |
| `forbid-unsafe-tags` | `false` | Forbid language-specific construction tags whose suffix begins with a known namespace: `python/`, `ruby/`, `perl/`, `php/`, `java/`, `java.`, or `javax.`. This list is a curated best-effort, not an exhaustive denylist. |
| `forbid-removed-types` | `false` | Forbid the YAML 1.1 types removed in 1.2: `!!omap`, `!!pairs`, `!!set`, `!!timestamp`, `!!binary`. |
| `allowed-tags` | `[]` | When non-empty, report any other local / non-core tag (e.g. `!env`) that is not listed. Core-schema tags (`!!str`, `!!omap`, …) are governed by the other two options, not by this allowlist. |

List `allowed-tags` entries in shorthand form (e.g. `!env`). Tag spelling is
normalised before matching, so shorthand, local, and verbatim (`!<…>`) forms
are treated alike and core-schema tags always compare as their `!!type` form.
A tag written with a custom `%TAG` handle is matched by its resolved URI (e.g.
`tag:example.com,2002:env`), not the `!handle!suffix` shorthand. The
non-specific `!` tag is never reported.

When more than one check matches the same node, a single diagnostic is
reported in the order: unsafe, removed type, not allowed.

A tag heading a block collection is reported at the collection, which may begin
on the next line, so a `# ryl disable-line` for it must sit on that line, not the
tag's line.

## Examples

### :x: Reported (with `forbid-unsafe-tags: true`)

```yaml
payload: !!python/object/apply:os.system ["id"]
record: !ruby/object:Account {}
```

### :x: Reported (with `forbid-removed-types: true`)

```yaml
ordered: !!omap [{a: 1}]
unique: !!set {a, b}
```

### :x: Reported (with `allowed-tags: ["!keep"]`)

```yaml
database: !env DATABASE_URL
```

### :white_check_mark: Allowed (with `allowed-tags: ["!keep"]`)

```yaml
secret: !keep VALUE
name: !!str 42
```

## Automatic fixing

This rule does not auto-fix. Rewriting or removing a tag would change the
node's resolved type (or require guessing the intended value), so no single
rewrite is universally safe.

## Related rules

- [`anchors`](anchors.md) &mdash; the analogous rule for the other piece of
  node metadata, anchors and aliases.
- [`quoted-strings`](quoted-strings.md) &mdash; useful when you want to pin a
  value to a string instead of relying on a tag.
