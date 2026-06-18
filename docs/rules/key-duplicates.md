# key-duplicates

## What this rule does

Reports duplicate keys in a single mapping. Optionally also reports
duplicated `<<` merge keys, keys that are *semantically* equal after YAML 1.2
core-schema resolution, and keys that collide once `<<` merges are expanded.

## Why this matters

- **Silent overwrites.** When two entries share a key, most YAML parsers
  keep only one of them; the other's value is silently discarded.
- **Schema validation.** Some downstream consumers reject documents with
  duplicate keys outright, so catching the duplication early avoids
  surprises in production.
- **Equal-but-differently-spelled keys.** The YAML 1.2.2 spec requires mapping
  keys to be unique and defines equality by each tag's canonical form, so
  `0xB` and `11` are the *same* integer key even though the text differs.
- **Invisible merge precedence.** When a single `<<` merges two mappings that
  define the same key, YAML silently keeps the first and discards the second.

## Configuration

```toml
[rules.key-duplicates]
level = "error"
forbid-duplicated-merge-keys = false
check-canonical = false              # ryl-only
forbid-merge-key-shadowing = false   # ryl-only
```

| Option | Default | Description |
| :--- | :--- | :--- |
| `forbid-duplicated-merge-keys` | `false` | Also report duplicate `<<` merge keys in the same mapping. |
| `check-canonical` | `false` | **ryl-only.** Compare keys by their YAML 1.2 core-schema canonical form, so `0xB`/`011`/`11` (integer 11) or `Null`/`~` collide, while a quoted `"11"` (a string) stays distinct from the integer `11`. A key carrying a local / non-core tag falls back to literal-text comparison. Also reports *merge-vs-merge* collisions: a `<<` whose merged mappings assign **different values** to the same key (sources that agree, or the same anchor merged twice, are not flagged). |
| `forbid-merge-key-shadowing` | `false` | **ryl-only.** Also report a key that is set both by a merge and explicitly when the two **values differ** &mdash; the value-changing `<<: *defaults` + override pattern. A redundant override to the merged value is not flagged. (Merge-vs-merge collisions are reported whenever either ryl-only knob is on.) |

`check-canonical` and `forbid-merge-key-shadowing` are ryl-only and configurable
only in TOML; a yamllint-compatible YAML config rejects them.

## Examples

### :white_check_mark: Allowed

```yaml
---
first: 1
second: 2
```

```yaml
# check-canonical: an intentional merge override is not a duplicate
defaults: &d {timeout: 30}
prod:
  <<: *d
  timeout: 60
```

### :x: Reported

```yaml
---
key: 1
key: 2
```

### :x: Reported (with `forbid-duplicated-merge-keys: true`)

```yaml
base: &base {host: localhost}
extra: &extra {port: 8080}
server:
  <<: *base
  <<: *extra
```

### :x: Reported (with `check-canonical: true`)

```yaml
# 0xB and 11 are both the integer 11
0xB: a
11: b
```

```yaml
# both merged mappings define `x`; the second is silently dropped
a: &a {x: 1}
b: &b {x: 2}
c:
  <<: [*a, *b]
```

### :x: Reported (with `forbid-merge-key-shadowing: true`)

```yaml
# the explicit `timeout` shadows the merged one
defaults: &d {timeout: 30}
prod:
  <<: *d
  timeout: 60
```

## Automatic fixing

This rule does not auto-fix; resolving a duplicate requires deciding
which value is canonical.

## Related rules

- [`key-ordering`](key-ordering.md) &mdash; enforces alphabetical key
  order, which makes duplicates harder to introduce accidentally.
- [`anchors`](anchors.md) &mdash; covers a related class of ambiguity
  for anchor/alias declarations.
- [`merge-keys`](merge-keys.md) &mdash; forbids the `<<` merge key outright;
  this rule's merge options instead detect *duplicate* or value-changing merges.
