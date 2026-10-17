---
icon: lucide/braces
---

# ryl

## A fast YAML linter and formatter, written in Rust

<div class="grid cards" markdown>

-   :zap:{ .lg .middle } **Built for speed**

    ---

    Written in Rust for fast lint runs across large YAML trees.

    [:octicons-arrow-right-24: Getting started](getting-started/installation.md)

-   :mag:{ .lg .middle } **Configurable rules**

    ---

    Choose the YAML style checks your project needs and configure their options.

    [:octicons-arrow-right-24: View rules](rules.md)

-   :wrench:{ .lg .middle } **Auto-fixes**

    ---

    Apply safe automatic fixes for spacing, line endings, quoting, and more
    with `ryl check --fix`, or format whole files with `ryl format`.

    [:octicons-arrow-right-24: Formatter](formatter.md)

-   :package:{ .lg .middle } **Easy to install**

    ---

    Single binary, distributed via Cargo, pip, npm, conda-forge, and
    winget. No runtime dependencies.

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

# Or with conda (conda-forge)
pixi global install ryl

# Or with winget (Windows)
winget install owenlamont.ryl

# Lint with every rule, before adding a config (see Quick start)
ryl check --enable ALL .

# Format in place
ryl format .
```

## YAML version compatibility

ryl targets **YAML 1.2** by default: it parses with
[granit-parser](https://github.com/bourumir-wyngs/granit-parser) (a
`saphyr-parser` fork) and resolves a directive-less document per the YAML 1.2
core schema, while honouring an explicit `%YAML` version directive.
See [YAML version compatibility](yaml-version.md) for how version directives
and implicit types affect linting and formatting.

## Why ryl

[ruff](https://github.com/astral-sh/ruff), [ty](https://github.com/astral-sh/ty),
[prek](https://github.com/j178/prek) and [rumdl](https://github.com/rvben/rumdl)
replaced the Python linters and formatters in my own toolchain with fast Rust
tools, leaving yamllint as the last one. ryl is its Rust replacement, modelled
on those tools.

ryl combines configurable lint rules, safe fixes, and whole-file formatting.
TOML configuration supports per-file rule selection, and SchemaStore helps
editors validate that configuration.

Coming from yamllint? See the [migration guide](getting-started/migrating-from-yamllint.md).

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
