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
  reference them; TOML configs must inline the preset content (see the
  example below). The full preset content is in
  [Configuration presets](../config-presets.md).
- Inline `# yamllint disable` / `disable-line` / `enable` comments are honoured
  with the same grammar and semantics, so existing in-file suppressions keep
  working. The equivalent `# ryl …` spelling is preferred for new files; see
  [Inline directives](../directives.md).

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
    yaml-files = ["*.yaml", "*.yml", ".yamllint"]

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
