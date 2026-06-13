---
name: codex-review-watch
description: Trigger and monitor a Codex CI code review on a GitHub PR and classify the verdict (clean / findings / rate-limited). Use after pushing changes to a PR you want Codex to (re-)review, or whenever waiting on a Codex GitHub review verdict — it encodes the polling gotchas (REST not GraphQL, three verdict channels, transient 👀) that make naive polling miss or misread the result.
---

# Codex review watch

Codex's GitHub code-review bot (`chatgpt-codex-connector[bot]`) is triggered by an
`@codex review` PR comment and answers a few minutes later. Detecting that answer
correctly is fiddly — this skill's `watch.py` does it so the logic isn't re-derived
(and re-bugged) each time.

## How to run

Run the watcher **as a background command** (it polls and blocks until a verdict or
timeout; backgrounding lets you keep working and get notified on exit):

```bash
uv run .agents/skills/codex-review-watch/watch.py <PR> [--repo owner/repo] [--no-trigger]
```

- Default: prints a **quota preflight**, posts `@codex review` (after capturing
  baselines), then polls. Pass `--no-trigger` to monitor a review you already triggered.
  Verdict detection compares against the baseline captured at startup, so in
  `--no-trigger` mode the verdict must land *after* you start watching; a review that
  already completed before startup is in the baseline and is not reported as new.
- `--repo` defaults to the current repo (`gh repo view`). Needs `gh` authenticated.
- Tunables: `--interval` (default 45s), `--max-polls` (default 40 ≈ 30 min).

For a **standalone quota check** (no PR, no trigger, consumes no quota):

```bash
uv run .agents/skills/codex-review-watch/watch.py --quota-only
```

When it exits, read its output file and relay the final `RESULT:` line. It prints one
of:

- `RESULT: CLEAN …` — Codex found no major issues (arrives as an issue comment or a
  👍 on the trigger).
- `RESULT: FINDINGS — N new inline comment(s):` followed by `[P1|P2|P3] <title> @
  path:line` for each.
- `RESULT: RATE-LIMITED …` — ChatGPT/Codex usage limit hit; retry after the rolling
  window.
- `RESULT: ISSUE-COMMENT (read it) …` — an unrecognized bot comment; read it.
- `TIMED OUT …` — no verdict in the window (slow or rate-limited; re-run to re-trigger).

## Why it works (gotchas it encodes)

- **REST, not GraphQL.** It polls `gh api repos/.../{pulls,issues}/...`. `gh pr view
  --json reviews/comments` renders the bot login differently, so a
  `select(.login=="chatgpt-codex-connector[bot]")` filter matches **nothing** and the
  poll runs blind.
- **Three verdict channels.** A verdict is a new **review** (findings, with inline
  P-badged comments), *or* a new **issue comment** ("no major issues" = clean, or a
  usage-limit notice), *or* a 👍 on the trigger comment. Watching only reviews misses
  the clean issue-comment verdict.
- **The 👀 ack is transient** — added within ~1 min, then removed when the verdict
  posts. So absence of 👀 *after* a verdict is normal (map trigger→verdict by
  **timestamp**, ~3 min apart), but absence *early on* (first ~2 min, no verdict) means
  Codex never picked the trigger up — the watcher warns on that.
- **Auto-review is unreliable** — only PR-open sometimes triggers; re-post `@codex
  review` after every push you want reviewed.

## Quota / rate limits

The cloud `@codex review` shares your **ChatGPT account quota** with the local Codex
CLI (rolling **5-hour** + **weekly** windows). When a window is exhausted, the trigger
**silently stalls** — no 👀, no review, often no message — which is the hardest case to
diagnose.

There is **no non-consuming quota command** (`/status` is TUI-only; `codex usage --json`
is only an open feature request, openai/codex#15281). The source of truth is the web
dashboard <https://chatgpt.com/codex/settings/usage>. As a proxy, the watcher reads the
rate-limit snapshot the Codex CLI **caches in its session rollouts**
(`~/.codex/sessions/**/rollout-*.jsonl`, the same data `/status` shows) and prints it as
a preflight (or via `--quota-only`):

- It reports each window's `used_percent` + reset time, and **warns when one is ≥90%**.
- It is **only as fresh as your last local Codex run** (the output notes its age), so
  treat a stale low reading with caution — but a stale *high* reading is a strong signal.
- If a future Codex CLI adds a live non-consuming usage command, prefer it over the
  cached snapshot.

## Acting on findings

Codex reviews the **code**, not the PR comment threads — it does **not** read replies
to its comments. So:

- To address a finding: change the code, then **re-run this skill** (re-trigger) and
  confirm a clean verdict.
- To refute a by-design finding: put the rationale **in the code/docs** (a clarifying
  comment is the only Codex-visible mitigation) and reply on the thread for the human
  record — but don't expect Codex to acknowledge it, and a re-review may re-raise it.
- Don't loop re-refuting an already-refuted by-design item; defer to the maintainer's
  merge call.
