# Using ryl with AI agents

ryl is built for programmatic use (stable exit codes, machine-readable output,
`--fix`/`--diff`, `ryl format`, stdin), so AI coding agents can drive it directly. This page points to
the agent-facing resources; the rest of these docs (start with
[Quick start](getting-started/quickstart.md)) cover the CLI itself.

## The ryl Agent Skill

Install the cross-tool [Agent Skill](https://github.com/owenlamont/ryl/blob/main/skills/ryl/SKILL.md)
so an agent learns the correct ryl invocation and config patterns:

```bash
gh skill install owenlamont/ryl ryl --agent claude-code --scope user
```

Swap `--agent` for `cursor`, `codex`, `antigravity`, `github-copilot`, and others (run
`gh skill install --help` for the full list); or run `gh skill search ryl` to discover
it. The skill is the canonical, tightened agent reference; this page only links to it.

## Prompting in AGENTS.md

If a repo has an `AGENTS.md` (or `CLAUDE.md`), a one-line nudge helps agents reach for
ryl and avoid its main gotcha:

> Lint YAML with `ryl check` and format it with `ryl format`. `check` has no default-on
> rules, so make sure `ryl.toml` enables rules first, or pass `--enable <RULES>`;
> otherwise it exits `2`.

`--disable <RULES>` subtracts from the lint selection; `--exclude <GLOBS>` replaces
configured exclusions. Both accept comma-separated values once per command.
Strict lint mode can also be configured with TOML `[lint] strict = true`;
`--no-strict` overrides configuration. `--no-warnings` suppresses lint warnings,
configuration warnings and deprecation notices; deprecation notices alone do not
fail strict mode.

Formatting defaults to two-space indentation; set top-level `indent-width` to keep
another width. For [lint conflict warnings](formatter.md#conflicting-lint-rules),
use `format --disable <RULES>` to silence selected rules or `--no-warnings` to
silence all warnings; formatting passes still run. For syntax repair and encoded
diff exit codes, use
the [formatter guarantees](formatter.md#guarantees) and
[diff guidance](getting-started/quickstart.md#preview-fixes-as-a-diff).

## Migrating existing tooling

Use `ryl --migrate-configs` to preview yamllint or static formatter imports.
`--migrate-from` selects Prettier, yamlfmt, dprint pretty_yaml or yamlfix;
`--migrate-directives` opts into suppression-comment conversion independently.
Add `--migrate-write` after inspecting the preview and warnings. See
[formatter migration](getting-started/migrating-from-formatters.md) for supported
settings and retained source contexts.

## Feeding the docs to an LLM

The docs site publishes [llmstxt.org](https://llmstxt.org) files for agents that fetch
documentation live (Cursor, Copilot, Claude Code, Windsurf):

```text
https://ryl-docs.pages.dev/llms.txt        # index: links + one-line summaries
https://ryl-docs.pages.dev/llms-full.txt   # the full docs concatenated in one file
```

## No MCP server

ryl is a CLI an agent can shell out to, so it intentionally ships no MCP server (as with
ruff, Biome, and Prettier). The skill plus `llms.txt` cover the agent use case.
