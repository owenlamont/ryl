# Using ryl with AI agents

ryl is built for programmatic use (stable exit codes, machine-readable output,
`--fix`/`--diff`, stdin), so AI coding agents can drive it directly. This page points to
the agent-facing resources; the rest of these docs (start with
[Quick start](getting-started/quickstart.md)) cover the CLI itself.

## The ryl Agent Skill

Install the cross-tool [Agent Skill](https://github.com/owenlamont/ryl/blob/main/skills/ryl/SKILL.md)
so an agent learns the correct ryl invocation and config patterns:

```bash
gh skill install owenlamont/ryl ryl --agent claude-code --scope user
```

Swap `--agent` for `cursor`, `codex`, `gemini-cli`, `copilot`, and others; or run
`gh skill search ryl` to discover it. The skill is the canonical, tightened agent
reference; this page only links to it.

## Prompting in AGENTS.md

If a repo has an `AGENTS.md` (or `CLAUDE.md`), a one-line nudge helps agents reach for
ryl and avoid its main gotcha:

> Lint YAML with `ryl`. It has no default-on rules, so make sure a config (`ryl.toml`
> or a yamllint-style `.yamllint`) enables rules first, or it exits `2`.

## Feeding the docs to an LLM

The docs site publishes an [llmstxt.org](https://llmstxt.org) index for agents that fetch
documentation live (Cursor, Copilot, Claude Code, Windsurf):

```text
https://ryl-docs.pages.dev/llms.txt
```

## No MCP server

ryl is a CLI an agent can shell out to, so it intentionally ships no MCP server (as with
ruff, Biome, Prettier, and yamllint). The skill plus `llms.txt` cover the agent use case.
