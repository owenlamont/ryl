# Migrating from yamllint

ryl is designed as a drop-in replacement for yamllint's existing rule set.
If you are coming from yamllint you have two paths:

- **Keep your existing YAML configuration.** ryl reads `.yamllint`,
  `.yamllint.yml`, and `.yamllint.yaml` with the same semantics as upstream.
  No changes needed to get started.
- **Migrate to TOML.** TOML is the recommended format for ryl-specific
  features that have no upstream equivalent &mdash; for example the
  [`[fix]` table](#optional-configure-auto-fixes) controlling auto-fix
  selection.

## Automatic migration

ryl ships with a built-in converter. From the root of your project:

```bash
# Preview the converted TOML (no files written)
ryl --migrate-configs

# Write .ryl.toml next to each discovered .yamllint file
ryl --migrate-configs --migrate-write

# Write and remove the original YAML configs
ryl --migrate-configs --migrate-write --migrate-delete-old

# Write and rename the original YAML configs (e.g. .yamllint.bak)
ryl --migrate-configs --migrate-write --migrate-rename-old .bak
```

Useful flags:

| Flag | Purpose |
| :--- | :--- |
| `--migrate-root <DIR>` | Search root (defaults to `.`) |
| `--migrate-stdout` | Print generated TOML to stdout instead of writing |
| `--migrate-rename-old <SUFFIX>` | Rename source YAML configs after migration |
| `--migrate-delete-old` | Delete source YAML configs after migration |

After migration, run `ryl .` to confirm diagnostics match what yamllint
produced.

## What is preserved

- Rule names, defaults, and option semantics match yamllint's existing rule
  set.
- Diagnostic message text is kept aligned with yamllint where practical, so
  existing log scrapers continue to work.
- Configuration discovery walks the same locations as yamllint, with TOML
  formats checked in addition: an explicit `--config-file`, then a
  project-local `.ryl.toml`, `ryl.toml`, `pyproject.toml`, `.yamllint`,
  `.yamllint.yaml`, or `.yamllint.yml`, then the user-level config
  directory.
- The three built-in presets &mdash; `default`, `relaxed`, and `empty` &mdash;
  match yamllint's behaviour. YAML configs can still use `extends:` to
  reference them; TOML configs must inline the `default`/`relaxed` content (see
  the example below). `empty` has no usable TOML form because ryl rejects a
  config that enables no rules. The full preset content is in
  [Configuration presets](../config-presets.md).
- Inline `# yamllint disable` / `disable-line` / `enable` comments are honoured
  with the same grammar and semantics, so existing in-file suppressions keep
  working. The equivalent `# ryl …` spelling is preferred for new files; see
  [Inline directives](../directives.md).

## How ryl differs from yamllint

ryl is a drop-in replacement, but it intentionally diverges from yamllint in a
small number of places. Every divergence is deliberate: ryl is **more correct
against the YAML 1.2.2 specification**, it **fails loudly instead of silently**, or
it **avoids redundant output**. The complete list:

### No implicit default configuration

ryl never enables a rule unless a configuration explicitly turns it on. yamllint,
run with no configuration, lints with its `default` preset; ryl instead exits `2`
with `no configuration found`. It also exits `2` (`no rules enabled`) when a
resolved config turns every rule off, where yamllint would silently lint nothing.

**Why ryl differs:** linting with rules the user never asked for &mdash; or
silently doing nothing &mdash; is surprising. ryl makes the rule set explicit and
reports loudly when there isn't one. The presets stay available as explicit
opt-ins, so the one-line equivalent of yamllint's out-of-the-box behaviour is a
YAML config containing `extends: default` (or the corresponding TOML preset from
[Configuration presets](../config-presets.md)). The migration converter flattens an
`extends: default` source into the generated TOML automatically, and warns when a
migrated config ends up enabling no rules.

### Anchor and alias names containing a colon

YAML 1.2.2 §6.9.2 defines an anchor/alias name as `ns-anchor-char+`, and
`ns-anchor-char` excludes only the flow indicators `, [ ] { }` &mdash; so a colon
is a **legal name character**. ryl's parser and the
[YAML reference parser](https://play.yaml.com) both read a `:` as part of the
name; yamllint (via PyYAML) stops at the first `:`, which the specification does
not permit. ryl follows the spec, so the `anchors` rule can diverge:

| Input (with `&x`/`&foo…` as noted) | ryl (spec-conformant) | yamllint (PyYAML) |
| :--- | :--- | :--- |
| `b: {*x: 2}` | `*x:` is an **undeclared** alias; `x` is **unused** | reads `{x: 2}` &mdash; no diagnostic |
| `&foo:bar` and `&foo:baz` | two **distinct** anchors | one anchor `foo`, reported **duplicated** |
| `*foo:baz` | a valid alias named `foo:baz` | a **syntax error** (PyYAML cannot scan it) |

**Why ryl differs:** the YAML specification and its reference parser are the
authority, and PyYAML's narrowing at `:` is non-conformant (see
[adrienverge/yamllint#686](https://github.com/adrienverge/yamllint/issues/686) and
[adrienverge/yamllint#780](https://github.com/adrienverge/yamllint/issues/780)).
The portable, unambiguous way to use an alias as a mapping key is a **space before
the colon** &mdash; `*foo : bar` &mdash; which every parser reads identically. The
ryl-only [`anchors: forbid-ambiguous-anchor-alias-names`](../rules/anchors.md)
option flags welded colons so you can forbid the construct outright.

### De-duplicated inputs

When a single run names the same file more than once &mdash; listed twice, or reached
by both a directory argument and an explicit path (`ryl . file.yaml`) &mdash; ryl
processes it **once**. yamllint handles each occurrence, so it would report that
file's diagnostics (or, under `ryl --diff`, emit its patch) twice.

**Why ryl differs:** duplicate output is never useful, and it is actively harmful for
`--diff`, whose output is meant to be applied as a patch &mdash; a repeated patch
block fails to apply on the second copy. ryl normalizes each input path (lexically,
without resolving symlinks, so a symlink and its target stay distinct) and skips a
file it has already selected.

## Side-by-side example

=== "yamllint (.yamllint)"

    ```yaml
    extends: default

    rules:
      line-length:
        max: 120
        allow-non-breakable-words: true
      quoted-strings:
        quote-type: double
        required: only-when-needed
      truthy: disable
    ```

=== "ryl (.ryl.toml)"

    ```toml
    [files]
    yaml = [
        "*.yaml",
        "*.yml",
        ".yamllint",
    ]

    [rules]
    anchors = "enable"
    braces = "enable"
    brackets = "enable"
    colons = "enable"
    commas = "enable"
    document-end = "disable"
    empty-lines = "enable"
    empty-values = "disable"
    float-values = "disable"
    hyphens = "enable"
    indentation = "enable"
    key-duplicates = "enable"
    key-ordering = "disable"
    new-line-at-end-of-file = "enable"
    new-lines = "enable"
    octal-values = "disable"
    quoted-strings = "disable"
    trailing-spaces = "enable"
    truthy = "disable"

    [rules.comments]
    level = "warning"

    [rules.comments-indentation]
    level = "warning"

    [rules.document-start]
    level = "warning"

    [rules.line-length]
    max = 120
    allow-non-breakable-words = true

    [rules.quoted-strings]
    quote-type = "double"
    required = "only-when-needed"
    ```

!!! note "TOML configuration is flat"

    TOML configuration does **not** support `extends`. Presets are expanded
    inline so the entire effective rule set is visible in one file. The
    `--migrate-configs` flag handles the expansion for you; if you write a
    TOML config by hand, start from the preset content in
    [Configuration presets](../config-presets.md).

## Optional: configure auto-fixes

TOML configurations can declare which rules are eligible for `ryl --fix`:

```toml
[fix]
fixable = ["ALL"]
unfixable = ["comments"]
```

This is a ryl-only feature. See the [Rules reference](../rules.md) for the
list of rules that support automatic fixing.

## Keeping both files

`.yamllint` and `.ryl.toml` can coexist. ryl prefers TOML when both are
present in the same directory, but a project that needs to remain
compatible with yamllint in CI can keep the YAML file authoritative and
treat the TOML file as supplemental for ryl-only features.
