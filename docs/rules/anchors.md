# anchors

## What this rule does

Reports problems with YAML anchors and aliases &mdash; duplicated anchor
names, aliases that reference undeclared anchors, anchors that are declared
but never used, and (optionally) anchor/alias names that are ambiguous because
a `:` is welded to them.

## Why this matters

- **Forward references fail at runtime.** An alias that points at an
  anchor declared later in the document is rejected by most YAML parsers.
- **Duplicates are silently ambiguous.** When two `&name` declarations
  share a name, downstream consumers may bind to either one depending on
  parser order.
- **Unused anchors clutter intent.** Removing dead anchors keeps the
  document's data-sharing structure explicit.
- **A `:` in a name means different things to different parsers.** YAML
  1.2.2 §6.9.2 lets an anchor/alias name contain any non-space character
  except the flow indicators `,[]{}`, so `:` is a *legal* name character.
  But parsers disagree where the name ends: ryl's parser, the YAML reference
  parser, and `ruamel.yaml` read `&foo:`/`*foo:` as the name `foo:` (colon
  included), while PyYAML/libyaml stop at the `:` (name `foo`, colon treated
  as a mapping separator). So a document of `a: &foo: 42` then `m:` / `- *foo:`
  resolves to `{a: 42, m: [42]}` under `ruamel.yaml` but raises a parse error
  under PyYAML — the same bytes, two different meanings. A single space
  disambiguates (`*foo : bar` is read the same way everywhere), so that is
  the form to use when an alias is genuinely a mapping key. Because ryl
  follows the spec here, the undeclared/duplicated/unused checks can differ
  from yamllint on colon-containing names; see
  [How ryl differs from yamllint](../getting-started/migrating-from-yamllint.md#how-ryl-differs-from-yamllint).

## Configuration

```toml
[rules.anchors]
level = "error"
forbid-undeclared-aliases = true
forbid-duplicated-anchors = false
forbid-unused-anchors = false
forbid-ambiguous-anchor-alias-names = false   # ryl-only
```

| Option | Default | Description |
| :--- | :--- | :--- |
| `forbid-undeclared-aliases` | `true` | Report aliases (`*name`) whose anchor (`&name`) has not been declared earlier. |
| `forbid-duplicated-anchors` | `false` | Report when the same anchor name is declared more than once. |
| `forbid-unused-anchors` | `false` | Report anchors that are never referenced by an alias. |
| `forbid-ambiguous-anchor-alias-names` | `false` | **ryl-only.** Report an anchor (`&name`) or alias (`*name`) whose name has a `:` welded to it &mdash; trailing (`&foo:`), internal (`&foo:bar`), or leading (`&:foo`) &mdash; the construct parsers disagree on. A space before the `:` (`*foo : bar`) is the unambiguous alias-key form and is never flagged. |

`forbid-ambiguous-anchor-alias-names` is ryl-only and configurable only in
TOML; a yamllint-compatible YAML config rejects it so the YAML `anchors`
namespace stays reserved for any future yamllint option.

## Examples

### :white_check_mark: Allowed

```yaml
---
- &anchor
  foo: bar
- *anchor
```

### :x: Reported (with `forbid-undeclared-aliases: true`)

```yaml
---
- &anchor
  foo: bar
- *unknown
```

### :x: Reported (with `forbid-duplicated-anchors: true`)

```yaml
---
- &anchor Foo Bar
- &anchor [item 1, item 2]
```

### :x: Reported (with `forbid-ambiguous-anchor-alias-names: true`)

```yaml
---
a: &foo: 42
m:
  - *foo:
```

## Automatic fixing

This rule does not auto-fix; anchor and alias graphs require human
judgement to rewrite safely.

## Related rules

- [`key-duplicates`](key-duplicates.md) &mdash; covers a related class of
  ambiguity in mappings.
