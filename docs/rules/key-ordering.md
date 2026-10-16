# key-ordering

## What this rule does

Requires that the keys within each mapping appear in lexicographic
(locale-aware) order.

## Why this matters

- **Predictable diffs.** When new keys are inserted in sorted order,
  diffs are localised to the area of change.
- **Reviewability.** A consistent key order makes it easy to spot when a
  key is missing or misnamed.

## Configuration

```toml
[lint.rules.key-ordering]
level = "error"
ignored-keys = []
```

| Option | Default | Description |
| :--- | :--- | :--- |
| `ignored-keys` | `[]` | Regular expressions; keys matching any pattern may appear in any order. |
| `orders` | `[]` | TOML only. Configured key orders for selected mappings; see [Custom orders](#custom-orders). |

## Examples

### :white_check_mark: Allowed

```yaml
---
alpha: 1
beta: 2
gamma: 3
```

### :x: Reported

```yaml
---
gamma: 3
alpha: 1
beta: 2
```

### :white_check_mark: Allowed (with `ignored-keys: ["^x-"]`)

```yaml
---
x-trailing-extension: value
alpha: 1
beta: 2
```

## Custom orders

Each `[[lint.rules.key-ordering.orders]]` entry gives a mapping a key order other
than alphabetical. An entry applies where both its `files` and its `path`
match, and the first matching entry wins. Mappings that no entry selects keep
the alphabetical order, and that includes mappings nested inside a selected one.

```toml
[lint.rules.key-ordering]
level = "error"

[[lint.rules.key-ordering.orders]]
files = [".pre-commit-config.yaml"]
path = "$.repos[*].hooks[*]"
keys = ["alias", "name", "description", "args", "env"]
```

| Field | Description |
| :--- | :--- |
| `files` | Globs matched like `per-file-ignores` keys: a basename or a path relative to the config. A pattern starting with `!` excludes. |
| `path` | A JSONPath subset: `$` (each document's root), `.name`, `['name']`, and `[*]` or `.*` (any key or sequence item). |
| `keys` | The order, first to last. |
| `unlisted` | `"sort"` (default) puts unlisted keys after the listed ones, alphabetically. `"keep"` leaves them in their original positions. |

Keys matched by `ignored-keys` keep their positions either way. With the
configuration above, `ryl check --fix` turns

```yaml
repos:
  - repo: local
    hooks:
      - id: example
        args: [--check]
        # Display name for this hook.
        name: Example hook
        alias: example-check
```

into

```yaml
repos:
  - hooks:
      - alias: example-check
        # Display name for this hook.
        name: Example hook
        args: [--check]
        id: example
    repo: local
```

and with `unlisted = "keep"`, `id` stays first:

```yaml
repos:
  - hooks:
      - id: example
        alias: example-check
        # Display name for this hook.
        name: Example hook
        args: [--check]
    repo: local
```

## Automatic fixing

`ryl check --fix` sorts each block mapping, nested ones included, and leaves
sequence order alone. Each entry moves with its comments:

- a comment block at the key's column, directly above the key, moves with it;
- an inline comment moves with its line;
- comments indented deeper than the key, directly after its value, move with it.

Blank lines stay where they are, as do comments before the first entry's own
comment block. Keys matched by `ignored-keys` keep their positions, and keys
that compare equal keep their relative order.

```yaml
---
# Gamma setting.
gamma: 3  # Gamma-specific note.
# Alpha setting.
alpha: 1  # Alpha-specific note.
beta: 2
```

becomes

```yaml
---
# Alpha setting.
alpha: 1  # Alpha-specific note.
beta: 2
# Gamma setting.
gamma: 3  # Gamma-specific note.
```

`--fix` leaves a mapping unsorted, prints
`path:line:col key-ordering not fixed: <reason>`, and keeps the finding when:

- it is a flow mapping (`{b: 1, a: 2}`), has a tag, or is part of a key;
- a key is complex, explicit (`? key`), or carries an anchor or tag;
- an own-line comment between two entries attaches to neither, such as one
  followed by a blank line or one at another column;
- an entry would carry its leading comment onto a `- ` line;
- sorting would move an alias before its anchor, or reorder two definitions of
  one anchor name;
- two keys spelled differently load as the same key (`true` and `True`), since
  the later one wins;
- an entry ends in a keep-chomping (`|+`, `>+`) block scalar, whose value
  absorbs the blank lines after it;
- a `# ryl disable`/`enable` directive sits inside it, or `key-ordering` is
  disabled on any of its lines.

To keep `--fix` from reordering keys at all, add `unfixable = ["key-ordering"]`
to the `[lint]` table.

## Related rules

- [`key-duplicates`](key-duplicates.md) &mdash; ordering and uniqueness
  are commonly enforced together.
