# Linting YAML embedded in Markdown

`ryl` can lint YAML that lives **inside** Markdown documents, in addition to
standalone `.yaml`/`.yml` files. Two kinds of embedded YAML are recognised:

- **Front matter** — the leading block delimited by `---` … `---` (or a closing
  `...`) at the very top of the file.
- **Fenced code blocks** tagged `yaml` or `yml` (including the `{.yaml}`
  attribute form and `~~~` tilde fences).

Each region is linted as its **own independent YAML document**, and every
diagnostic's line and column point back into the original Markdown file.

This is a ryl-only capability, so it is configured exclusively in **TOML**
(`ryl.toml`, `.ryl.toml`, or `[tool.ryl]` in `pyproject.toml`). The YAML
(`yamllint`-compatible) configuration uses the legacy `yaml-files` key and has no
markdown support.

## Source kinds and the `[files]` table

In TOML, ryl assigns every file a **source kind** via the `[files]` table, which
maps each kind to a list of gitignore-style glob patterns:

```toml
[files]
yaml     = ["*.yaml", "*.yml", ".yamllint"]   # default if [files] is omitted
markdown = ["*.md", "docs/**/*.md"]           # opt-in: enables markdown linting
```

- `yaml` defaults to `["*.yaml", "*.yml", ".yamllint"]`; setting it replaces the
  default.
- `markdown` is empty by default — listing patterns is what **enables** markdown
  linting (and scopes it, so only matching files are touched).
- A file that matches **more than one** kind is a hard error (a file has exactly
  one kind).
- A file passed **explicitly** that matches no kind is rejected with an error
  telling you to add a glob; a file found while scanning a directory that matches
  no kind is simply skipped.

> The legacy `yaml-files` key is **not** valid in TOML — use `[files].yaml`. It
> remains valid in the yamllint-compatible YAML config.

Markdown behaviour is tuned in a separate `[markdown]` table (both default `true`):

```toml
[markdown]
front-matter  = true   # lint the --- ... --- block
fenced-blocks = true   # lint yaml / yml fenced blocks
```

Set either flag to `false` to lint only the other source.

## How rules apply

The same rule set and configuration that applies to standalone YAML applies to
each embedded region. Four file-shape rules are **suppressed** inside embedded
regions, because a region is not a standalone file:

- `document-start` and `document-end` — the front matter delimiters are not part
  of the linted content, and code-block fragments rarely carry markers.
- `new-line-at-end-of-file` and `new-lines` — these are governed by the host
  Markdown file, not the embedded snippet.

All other rules (indentation, `key-duplicates`, `colons`, `truthy`,
`line-length`, `trailing-spaces`, …) run normally.

## Example

A document with both a front matter block and a fenced `yaml` block:

````markdown
---
title:  Example
---

# Notes

```yaml
build:
  steps:
    -  run: make
```
````

With `colons` enabled, `ryl docs.md` reports the extra space after `title:` on
line 2 and any spacing problems inside the fenced block on its actual line —
columns include the block's indentation.

## `--fix`

`--fix` does **not** modify Markdown files; embedded YAML is checked only. When a
run includes Markdown files, `ryl` prints a one-line note and leaves those files
untouched while still reporting their diagnostics.

## Use with pre-commit

When `ryl` runs as a pre-commit hook, the hook only sees the file paths
pre-commit passes to it. The `ryl` hook targets YAML files by default, so to lint
Markdown you must both (a) enable `[markdown]` in your ryl config and (b) widen
the hook to pass Markdown files, for example:

```yaml
- repo: https://github.com/owenlamont/ryl-pre-commit
  rev: <version>
  hooks:
    - id: ryl
      types_or: [yaml, markdown]
```

pre-commit decides *which* files to pass; `[files]` decides *how* ryl treats each.
So if the hook passes a `.md` that no `[files]` glob matches, ryl reports an error
(it was named explicitly) — add a `markdown` glob to `[files]` to lint it, or narrow
the hook's file filter.
