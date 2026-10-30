# ryl

ryl is a fast YAML linter and formatter written in Rust. It checks YAML style,
applies safe lint fixes, and formats files with configurable layout. It is usable
today, but edge-case behaviour is still maturing.

Full documentation lives at <https://ryl-docs.pages.dev/>.

ryl targets YAML 1.2. See [YAML version compatibility](https://ryl-docs.pages.dev/yaml-version/)
for how version directives and implicit types affect linting and formatting.

Coming from yamllint? See the [migration guide](https://ryl-docs.pages.dev/getting-started/migrating-from-yamllint/).

## Quick start

ryl enables no rules by default, so it needs a configuration or `--enable` that
turns rules on. `--enable ALL` runs every rule for a one-off run, and
`--enable <RULES>` runs just the named rules; for a project,
drop a `.ryl.toml` at the root (see
[Configuration](https://ryl-docs.pages.dev/getting-started/quickstart/)).

`ryl check` is the lint subcommand and the recommended form; `ryl format` is the
formatter. Bare `ryl <paths>` still lints identically but is deprecated: it
prints a warning to stderr (silenced by `--no-warnings`) and a later release will
remove it, so prefer `ryl check`.

```bash
# Using uv (Python)
uvx ryl check --enable ALL .

# Omit selected rules and files without writing a config
uvx ryl check --enable ALL --disable line-length,truthy,document-start --exclude pnpm-lock.yaml .

# Using npx (Node.js)
npx @owenlamont/ryl check --enable ALL .

# Format in place, or check formatting in CI
uvx ryl format .
uvx ryl format --check .
```

For `prek` / `pre-commit` integration, see
[ryl-pre-commit](https://github.com/owenlamont/ryl-pre-commit).

## Installation

```bash
uv tool install ryl                 # uv
npm install -g @owenlamont/ryl      # npm
pip install ryl                     # pip
cargo install ryl                   # cargo
pixi global install ryl             # conda-forge
winget install owenlamont.ryl       # winget (Windows)
```

Building from source (`cargo install`, or a `pip` sdist) needs Rust 1.99.1 or
newer. The prebuilt wheels, npm, conda-forge and winget packages do not.

In GitHub Actions, use [ryl-action](https://github.com/marketplace/actions/ryl-action):

```yaml
- uses: owenlamont/ryl-action@v0.27.0
```

## Status and scope

- `ryl format` formats YAML in place without enabling lint rules. Its `--check`
  and `--diff` modes preview changes without writing; `[format]` in TOML controls
  the style. It also formats embedded YAML in Markdown with `--markdown`.
- ryl includes 27 rules, including `tags`, `merge-keys`,
  `block-scalar-chomping`, and `unicode-line-breaks`. The rule reference
  and per-rule pages are at <https://ryl-docs.pages.dev/rules/>.
- Auto-fixing (`--fix`) is supported for `braces`, `brackets`, `colons`,
  `commas`, `comments`, `comments-indentation`, `document-end`,
  `document-start`, `empty-lines`, `hyphens`, `key-ordering`,
  `new-line-at-end-of-file`, `new-lines`, `quoted-strings`, `trailing-spaces`,
  and `truthy` (case only, e.g. `True` to `true`). The set of rules that may
  apply fixes is configurable via the TOML `lint.fixable`/`lint.unfixable`
  keys.
- `--diff` previews those safe fixes as a unified diff on stdout instead
  of writing them (modelled on `ruff check --diff`); it is mutually
  exclusive with `--fix`, works with stdin, and exits `1` iff some file
  would change.
- TOML is the recommended configuration format and supports
  the `lint.fixable`/`lint.unfixable`
  fix policy, `[lint.per-file-ignores]`, and rule options such as
  `allow-double-quotes-for-escaping`.
- TOML assigns each file a source kind via the `[files]` table
  (`yaml = [...]`, `markdown = [...]`). The legacy `yaml-files` key is YAML-only;
  in TOML use `[files].yaml`.
- YAML embedded in Markdown (front matter and fenced `yaml`/`yml` blocks)
  can be linted by listing globs under `[files].markdown`; diagnostics map back
  to the Markdown file. It is off by default; `--fix` writes safe fixes back into
  the embedded blocks and `--diff` previews them at the host-file level. See
  <https://ryl-docs.pages.dev/markdown/>.
- YAML configuration (`.yamllint`, `.yamllint.yml`,
  `.yamllint.yaml`, including the `default`, `relaxed`, and `empty` presets via
  `extends`) still works but is deprecated and warns; convert it with
  `ryl --migrate-configs --migrate-write`.
- `--list-files` prints the files ryl would lint (after ignores and
  config discovery) and exits, without running rules. `--no-warnings`
  suppresses warning-level diagnostics in the output. `--strict` turns a
  warning-only run into exit code `2`; TOML `[lint] strict = true` enables the
  same behavior for a project, and `--no-strict` overrides the config.
  `--enable <RULES>` runs exactly the
  listed rules (comma-separated IDs, or `ALL`) in place of the config's
  selection, and needs no config file. `--disable <RULES>` subtracts from the
  selected lint rules. `--exclude <GLOBS>` replaces config file exclusions;
  globs use the current directory and also exclude explicitly named files.
  Both new flags accept comma-separated values and may appear once per command.
- `ryl check` and `ryl format` cache results by content and configuration.
  Use `--no-cache` to bypass the cache or `--cache-dir PATH` to place it elsewhere.
  See [caching](docs/getting-started/quickstart.md#caching).
- Pass `-` as the input to read YAML from stdin (ruff convention). Add
  `--stdin-filename <PATH>` so diagnostics, project-config discovery,
  and path-based filtering (`yaml-files`, per-file-ignores, per-rule
  `ignore`) use that filename. Without it, diagnostics are labelled
  `<stdin>`, config is anchored at the current working directory, and
  all path-based filtering is skipped so every enabled rule runs.
  `-` cannot be combined with other inputs; `--fix` cannot read from
  stdin, but `--diff` can.
- `ryl server` runs ryl as a Language Server (LSP) over stdio, giving
  editors inline diagnostics, a `source.fixAll.ryl` code action, and
  formatting via `ryl format`. It complements Red Hat's
  `yaml-language-server` rather than replacing it, and is built in by
  default (a minimal binary without it builds with
  `cargo install ryl --no-default-features`). See
  <https://ryl-docs.pages.dev/editor-integration/>.
- Run `ryl --help` for the authoritative CLI reference.

For installation walkthroughs, configuration presets, and per-rule
documentation, see the docs site.

## Performance

The chart times ryl against yamllint and three formatters on pinned real-world
YAML: single files from 1 KiB to 9.4 MiB, and 1 to 1,000 files of under 16 KiB
(98% of the 76,000 YAML files in the 40 pinned repos are under 16 KiB). Both axes
are logarithmic.

![Lint and max-common-work format: wall time and peak memory](https://raw.githubusercontent.com/owenlamont/ryl/main/img/benchmark.svg)

The format panel uses **max common work**: every formatter configured for output
as close as its options allow, plus the two rewrites all four offer, single quotes
and folding long lines at 80 columns. It is the most work each tool can be asked to
do in common. Median of 10 runs; peak RSS is the median of 3 more:

| Input | ryl | yamlfmt | dprint + pretty_yaml | Prettier |
| --- | ---: | ---: | ---: | ---: |
| 1 KiB file | 2.5 ms, 8.3 MiB | 2.7 ms, 5.5 MiB | 6.6 ms, 16 MiB | 107 ms, 64 MiB |
| 16 KiB file | 3.8 ms, 8.7 MiB | 4.0 ms, 6.3 MiB | 9.4 ms, 17 MiB | 152 ms, 79 MiB |
| 0.9 MiB file | 75 ms, 25 MiB | 138 ms, 70 MiB | 559 ms, 38 MiB | 1.33 s, 369 MiB |
| 9.4 MiB file | 0.91 s, 191 MiB | 1.30 s, 674 MiB | 5.08 s, 241 MiB | 10.1 s, 1.8 GiB |
| 1,000 files | 30 ms, 16 MiB | 247 ms, 18 MiB | 147 ms, 23 MiB | 3.78 s, 246 MiB |

| Lint, `extends: default` | 1 KiB | 9.4 MiB | 1,000 files |
| --- | ---: | ---: | ---: |
| ryl | 2.8 ms | 0.63 s | 27 ms |
| yamllint | 75 ms | 31.2 s | 5.67 s |

ryl formats faster than the other three at every size and file count, under all
three profiles. yamlfmt uses less memory up to 16 KiB (5.5–6.3 MiB against ryl's
8.3–8.7 MiB).

Profiles and what they cannot equalise:

- **Max common work** (charted): aligned configs plus single quotes and folding at
  80 columns; 91–96% identical lines, as each tool folds differently.
- **Aligned configs**: each formatter set up to produce output as close
  to the others' as its options allow. ryl `quote-style = "double"`,
  `max-blank-lines = 1`, `comment-spacing = 1`, `comment-starting-space =
  "preserve"`, `brace-spacing = true`; yamlfmt `retain_line_breaks_single`, double quotes,
  `pad_line_comments: 1`, `trim_trailing_whitespace`; dprint and Prettier defaults.
  Pairwise identical lines are 94–98%. The rest: ryl's double quote style also
  removes quotes no value needs, yamlfmt drops `---`, dprint keeps inline comment
  alignment, and dprint and Prettier wrap long strings at 80 columns.
- **Defaults**: each tool with no configuration, so each does different work.
- **Lint**: yamllint reads `extends: default` and ryl reads the TOML that
  `ryl --migrate-configs` writes from the same file. On the benchmark inputs they
  share 43,569 diagnostics; yamllint alone reports 36 (20 `braces` inside Jinja
  `{{ }}`, 10 `trailing-spaces` inside block scalars, 6 `comments`,
  `comments-indentation` and `document-start`) and ryl alone 2 (`document-start`).

Every run gets a fresh copy of its inputs and an empty cache directory, with
ryl's and Prettier's caches off and dprint's incremental mode off; dprint keeps
its compiled plugin, an install artifact. Tools alternate order run to run.
yamlfmt is the native release binary, not a launcher; ryl, dprint and yamlfmt may
use several threads, Prettier and yamllint one. Inputs are files every tool
accepts under every profile, which excludes Helm templates and other non-YAML.
Each size point is a different file, so content varies along that axis.

| Tool | Version |
| --- | --- |
| ryl | 0.27.0 |
| yamllint | 1.38.0 |
| yamlfmt | 0.21.0 |
| Prettier | 3.9.10 |
| dprint + pretty_yaml | 0.64.3 + 0.6.0 |

The [benchmark script](scripts/benchmark_yaml_tools.py) reruns everything and draws
the chart; `--tool-versions latest` uses the newest release of each tool instead of
the pins in `scripts/benchmark_tool_versions.toml`, and records the versions used.

## Acknowledgements

This project exists thanks to the tooling and ecosystems around YAML
linting and developer automation, especially:

- [yamllint](https://github.com/adrienverge/yamllint) - for giving me the
  shoulders to stand on and the source of many of the automated tests
  that ryl uses now to check for behaviour parity. Copying the behaviour
  of an existing tool is infinitely easier than building one from
  scratch - there'd be no ryl without yamllint.
- [ruff](https://github.com/astral-sh/ruff) - for showing the power of
  Rust tooling for Python development and inspiring the config and API
  for ryl.
- [rumdl](https://github.com/rvben/rumdl) - for giving me another
  template to follow for Rust tooling.
- [saphyr](https://github.com/saphyr-rs/saphyr) - ryl's vendored YAML DOM
  is derived from saphyr, and saphyr's developers were very patient in
  showing some of the nuance and complexity of parsing YAML which I was
  embarrassingly ignorant of when starting ryl.
- [granit-parser](https://github.com/bourumir-wyngs/granit-parser) - a
  `saphyr-parser` fork with comment and style metadata; ryl's own parser,
  `ryl-yaml-parser`, is forked from granit-parser 2.0.0.
- [esbuild](https://github.com/evanw/esbuild) and
  [biome](https://github.com/biomejs/biome) - for providing the "binary
  wrapper" blueprint for distributing high-performance native tools via
  NPM.
