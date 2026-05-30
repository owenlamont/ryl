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
  default. Add patterns for YAML stored under less-common names or extensions — for
  example Citation File Format (`*.cff`), clang's `.clang-format` / `.clang-tidy`,
  or Common Workflow Language (`*.cwl`):
  `yaml = ["*.yaml", "*.yml", "*.cff", ".clang-format", "*.cwl"]`. Globs match exact
  filenames and extensionless dotfiles too. (Avoid pointing it at templated
  pseudo-YAML such as SaltStack `*.sls` or `*.yaml.j2`, which embed Jinja and are not
  valid standalone YAML.)
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

## Other Markdown-family formats (Quarto, RMarkdown, MDX, …)

The `markdown` kind is not tied to the `.md` extension. Front matter is found with
a format-agnostic line scan and fenced blocks are located with a CommonMark parser,
so any Markdown-superset format works — map its extension(s) to the `markdown` kind:

```toml
[files]
markdown = ["*.md", "*.markdown", "*.qmd", "*.Rmd", "*.mdx"]
```

Or, for a one-off run without editing config, pass `--markdown`, which enables the
`markdown` kind with those default globs:

```sh
ryl --markdown docs/        # scan a tree for *.md/*.markdown/*.qmd/*.Rmd/*.mdx
ryl --markdown report.qmd   # a single Quarto document
cat SKILL.md | ryl --markdown -   # from stdin (e.g. an editor / pre-commit)
```

This lints the YAML front matter and fenced `yaml`/`yml` blocks in Quarto (`.qmd`),
RMarkdown (`.Rmd`), MDX (`.mdx`), and similar documents. Format-specific constructs
that are not CommonMark (e.g. MDX/JSX, Quarto/RMarkdown executable `{r}`/`{python}`
chunks) are ignored — only YAML front matter and `yaml`/`yml` fenced blocks are
extracted.

In practice Quarto and RMarkdown keep their YAML almost entirely in **front matter**
(their code chunks are ` ```{r} `/` ```{python} `, not ` ```yaml `), so linting them
mostly exercises the front-matter path. For example, `ryl --markdown report.qmd`
checks the leading block of:

````qmd
---
title: "Quarterly Report"
format:
  html:
    toc:  true
---

## Section
````

and reports the extra space after `toc:` at its real line and column inside the
`.qmd` file. The same applies to **agent skill files**: a `SKILL.md` is YAML front
matter (`name`, `description`) plus prose, so `ryl --markdown SKILL.md` (or a
`markdown = ["**/SKILL.md"]` glob) lints that block.

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

`--fix` applies the same safe fixes to each embedded region and writes the result
back into the Markdown document, re-indenting fixed YAML to its fence column and
preserving the document's line endings (CRLF stays CRLF). The four file-shape rules
suppressed in check mode are also excluded from fixing, so a fragment never gains a
`---`/`...` marker or a trailing newline.

Write-back is **conservative by construction**: ryl only rewrites a region when its
fixed YAML can be re-indented to reproduce the region's original bytes exactly. A
region it cannot reproduce — ragged indentation (content lines indented less than
the fence), tab indentation, or other non-uniform layouts — is left **byte-for-byte
untouched** while still being reported. This guarantees `--fix` can never corrupt a
Markdown document: the worst case is that an unusual region is reported but not
auto-fixed.

## Linting Markdown from stdin and the CLI

Markdown linting is normally enabled by listing `[files].markdown` globs, but it can
also be turned on for a single run from the command line:

- `--markdown` enables Markdown linting using default globs (`*.md`, `*.markdown`,
  `*.mdx`, `*.qmd`, `*.Rmd`) without editing config. It is a no-op when
  `[files].markdown` is already set, and its injected globs **win** over the `yaml`
  globs for an overlapping file (so the flag never aborts a run whose `yaml` globs
  happen to match a Markdown extension). When linting stdin, `--markdown` forces the
  input to be treated as Markdown regardless of `--stdin-filename`.
- Reading from stdin otherwise honours the source kind: `ryl - --stdin-filename
  doc.md` lints the piped bytes as Markdown when `doc.md` matches the `markdown`
  globs (front matter and fenced blocks are extracted exactly as for a file on
  disk). Without `--stdin-filename` and without `--markdown`, stdin is linted as
  plain YAML. As with files, `--fix` is not supported when reading from stdin.

## Use with pre-commit

When `ryl` runs as a pre-commit hook, the hook only sees the file paths
pre-commit passes to it. The `ryl` hook targets YAML files by default, so to lint
Markdown you must both (a) add a `markdown` glob under `[files]` in your ryl config
and (b) widen the hook to pass Markdown files, for example:

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

`ryl` also applies its `ignore` patterns to **explicitly passed** files, not just
to files found by scanning a directory. So a file pre-commit hands to `ryl` that
matches `ignore` is skipped — the equivalent of ruff's `force-exclude`, always on,
with no separate flag to set.
