---
icon: lucide/braces
---

# ryl

## A fast YAML linter, written in Rust

<div class="grid cards" markdown>

-   :zap:{ .lg .middle } **Built for speed**

    ---

    Written in Rust for fast lint runs across large YAML trees.

    [:octicons-arrow-right-24: Getting started](getting-started/installation.md)

-   :mag:{ .lg .middle } **yamllint-compatible**

    ---

    Drop-in compatible rule set with the rules and behaviour of `yamllint`,
    plus ryl-specific options.

    [:octicons-arrow-right-24: View rules](rules.md)

-   :wrench:{ .lg .middle } **Auto-fixes**

    ---

    Apply safe automatic fixes for spacing, line endings, quoting, and more
    with `ryl --fix`.

    [:octicons-arrow-right-24: Configuration](config-presets.md)

-   :package:{ .lg .middle } **Easy to install**

    ---

    Single binary, distributed via Cargo, pip, and npm. No runtime
    dependencies.

    [:octicons-arrow-right-24: Install](getting-started/installation.md#installation)

</div>

## Quick start

```bash
# Install with Cargo
cargo install ryl

# Or with pip
pip install ryl

# Or with npm
npm install --global @owenlamont/ryl

# Lint a file or directory
ryl path/to/file.yaml
ryl .
```

## YAML version compatibility

ryl targets **YAML 1.2** strictly: it parses with
[granit-parser](https://github.com/bourumir-wyngs/granit-parser) (a
`saphyr-parser` fork) and resolves scalars per the YAML 1.2 core schema.
yamllint defaults to YAML 1.1 semantics via PyYAML, so a handful of edge
cases &mdash;
notably bareword booleans like `yes` / `no` / `on` / `off` and
leading-zero integers like `0755` &mdash; behave differently. The same
1.2 semantics apply to `.yamllint` configuration files.

See [YAML version compatibility](yaml-version.md) for the practical
implications and how to adjust a configuration migrated from yamllint.

## Why ryl

ryl started as a fast Rust port of [yamllint][yamllint] focused on parity with
its rule set and message text. The aim is to keep yamllint compatibility as a
foundation while adding ergonomic features yamllint does not currently offer
&mdash; for example richer TOML configuration, auto-fix support for spacing
and quoting rules, and tighter integration with SchemaStore.

When a rule mirrors yamllint, the rule reference page links back to the
upstream documentation so you can confirm exact semantics. ryl-specific
options are called out explicitly so they cannot be mistaken for upstream
behaviour.

  [yamllint]: https://yamllint.readthedocs.io/

## Next steps

<div class="grid cards" markdown>

-   [:octicons-download-24: __Install ryl__](getting-started/installation.md)

    Cargo, pip, npm, or a prebuilt binary.

-   [:octicons-play-24: __Run your first lint__](getting-started/quickstart.md)

    Lint a project in under a minute.

-   [:octicons-book-24: __Rules reference__](rules.md)

    Every rule, with examples and fixable status.

-   [:octicons-gear-24: __Configuration presets__](config-presets.md)

    `default` and `relaxed` as TOML or YAML, plus the YAML-only `empty`.

</div>
