# Editor integration (language server)

`ryl server` runs ryl as a [Language Server Protocol](https://microsoft.github.io/language-server-protocol/)
(LSP) server over stdio, so any LSP-capable editor gets ryl's diagnostics and fixes inline
as you type. It is the same lint and fix engine as the CLI, exposed over the protocol.

A ryl language server is a **linter** provider. It is meant to run *alongside* Red Hat's
[`yaml-language-server`](https://github.com/redhat-developer/yaml-language-server) (schema
validation, completion, hover), the way Ruff runs alongside Pylance: editors attach both
and merge their diagnostics. ryl does not do schema validation, completion, or hover.

## What it provides

| Capability | LSP feature | Behaviour |
| --- | --- | --- |
| Diagnostics | `textDocument/publishDiagnostics` | Every enabled rule, re-linted on open and on each change |
| Fix all | `source.fixAll.ryl` code action | Applies every safe fix to the document (the `--fix` set) |
| Formatting | `textDocument/formatting` | Same as "fix all": formatting *is* applying safe fixes |

The fix-all action and formatting both apply ryl's whole-file safe fixes; ryl has no
per-occurrence "fix just this one" action, because its fix engine operates per file. A
document that does not parse is never modified (the same guarantee as `ryl --fix`).

## Running it

```console
$ ryl server
```

The server speaks LSP over stdin/stdout; you do not run it directly but point your editor's
LSP client at the command. (`server` is a subcommand, so to lint a path literally named
`server` use `ryl ./server` or `ryl server/`.) Configuration is discovered per document exactly as the CLI does
it (a `.ryl.toml` / `ryl.toml` / `[tool.ryl]` in `pyproject.toml`, or a yamllint config,
found by walking up from the file). As with the CLI, **ryl enables no rules by default**: a
file with no discovered config that enables at least one rule produces no diagnostics.

YAML and Markdown (embedded YAML) documents are both supported; the source kind is resolved
from your `[files]` globs just like the CLI. An untitled (unsaved) buffer is linted as YAML,
with config discovery anchored at the workspace root.

## Neovim

Neovim has a built-in LSP client. Until a `nvim-lspconfig` entry exists, register ryl
manually, for example:

```lua
vim.api.nvim_create_autocmd("FileType", {
  pattern = { "yaml", "markdown" },
  callback = function(args)
    vim.lsp.start({
      name = "ryl",
      cmd = { "ryl", "server" },
      root_dir = vim.fs.root(args.buf, { ".ryl.toml", "ryl.toml", ".git" }),
    })
  end,
})
```

Trigger the fix-all action with `vim.lsp.buf.code_action()`, or format with
`vim.lsp.buf.format()`.

## VS Code

A dedicated VS Code extension is tracked in
[issue #208](https://github.com/owenlamont/ryl/issues/208) and is not yet released. Once it
ships, it will bundle the binary and wire `ryl server` up automatically, including
format-on-save via `editor.defaultFormatter`.

## Notes

- **Position encoding** is negotiated at startup (UTF-8, UTF-16, or UTF-32); ryl supports
  all three and defaults to UTF-16 when the client states no preference, so columns line up
  correctly even for multi-byte and astral-plane characters.
- Diagnostics for an open document are recomputed when it is opened or edited. Editing a
  **config file** on disk (`.ryl.toml`, `pyproject.toml`, a yamllint config) does not yet
  re-lint already-open documents; re-open or edit a document to pick up the new config.
- A missing config or one that enables no rules produces **no diagnostics** — the editor
  stays quiet, matching ryl's "no rule is on unless you enable it" philosophy. A
  **malformed** config (which would make `ryl` exit non-zero on the CLI) is reported once
  via a `window/showMessage` so you know linting is off, rather than failing silently.
- The server is compiled in by default. A minimal build without it (and without its
  dependencies) is available via `cargo install ryl --no-default-features`.
