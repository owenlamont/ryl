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

Building from source (`cargo install`, or a `pip` sdist) needs Rust 1.99.0 or
newer. The prebuilt wheels, npm, conda-forge and winget packages do not.

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

The chart compares lint and format times on real-world YAML sampled from pinned
repositories: 20–100 files × 1–9 KiB, with five runs per point.

![Lint and format benchmark on real-world YAML](https://raw.githubusercontent.com/owenlamont/ryl/main/img/benchmark-cold-5x5-5runs.svg)

The [benchmark script](scripts/benchmark_yaml_tools.py) also offers a generated stress dataset with `--datasets stress`.

| Tool | Mode | Real-world mean, 100 × 9 KiB (s) |
| --- | --- | ---: |
| ryl | Lint | 0.050 |
| yamllint | Lint | 2.304 |
| ryl | Format | 0.056 |
| yamlfmt | Format | 0.113 |
| Prettier | Format | 1.140 |
| dprint | Format | 0.105 |

| Tool | Mode | Version |
| --- | --- | --- |
| ryl | Lint, format | 0.26.0 |
| yamllint | Lint | 1.38.0 |
| yamlfmt | Format | 0.21.0 |
| Prettier | Format | 3.9.10 |
| dprint + pretty_yaml plugin | Format | 0.63.0 + 0.6.0 |

Result caches are cold; install artifacts are warm. The
[benchmark script](scripts/benchmark_yaml_tools.py) records the commands and
cache checks. On the real-world dataset, ryl format is faster than dprint and yamlfmt
with value preservation and idempotence guaranteed by rule order and tests rather
than a second parse ([formatter guarantees](docs/formatter.md#guarantees)).

Run the benchmark with `--warm` to also record second-run timings with each
tool's result cache enabled. Its warm view is separate from the cold chart above.

![Warm second-run benchmark on real-world YAML](https://raw.githubusercontent.com/owenlamont/ryl/main/img/benchmark-warm-5x5-5runs.svg)

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
