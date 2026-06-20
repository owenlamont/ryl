---
name: codex-review
description: Drive a Codex CI code review on a GitHub PR end to end, from triggering it through monitoring and classifying the verdict (clean / findings / rate-limited) to handling the resulting comments (react, resolve, reply). Use after pushing changes you want Codex to (re-)review, while waiting on a verdict, or when triaging Codex's review comments. It encodes the polling gotchas (REST not GraphQL, three verdict channels, transient 👀) and the comment-handling conventions (when to thumbs-up/down, resolve, or reply).
---

# Codex review

Codex's GitHub code-review bot (`chatgpt-codex-connector[bot]`) is triggered by an
`@codex review` PR comment and answers a few minutes later. Detecting that answer
correctly is fiddly — this skill's `watch.py` does it so the logic isn't re-derived
(and re-bugged) each time.

## How to run

Run the watcher **as a background command** (it polls and blocks until a verdict or
timeout; backgrounding lets you keep working and get notified on exit):

```bash
uv run .agents/skills/codex-review/watch.py <PR> [--repo owner/repo] \
  [--first-review] [--no-trigger]
```

- Default: prints a **quota preflight**, posts `@codex review` (after capturing
  baselines), then polls. Pass `--no-trigger` to monitor a review you already triggered.
  Verdict detection compares against the baseline captured at startup, so in
  `--no-trigger` mode the verdict must land *after* you start watching; a review that
  already completed before startup is in the baseline and is not reported as new.
- **First review on a freshly-opened PR:** pass `--first-review`. Codex almost always
  *auto-starts* a review on PR open (acked by a transient 👀 on the **PR body**, with no
  `@codex review` comment), so blindly posting `@codex review` double-triggers it.
  `--first-review` baselines all bot channels at zero (the fresh PR's true pre-state),
  then watches for that auto-start — a body 👀 ack, or a verdict landing within
  `--auto-wait` (default 90s) — and **skips** posting. It falls back to posting `@codex
  review` only if no auto-start fires in that window (so the worst case is exactly the
  default behavior, no regression). Use it **only** for the first review; **subsequent**
  (post-push) reviews have no auto-start and need the explicit prompt, so leave it off.
  - The auto-review also stalls the *other* way — it acks with 👀 then silently posts
    nothing ("stuck eyes," a real Codex flake). So `--first-review` trusts the 👀 ack
    but not forever: after `--stuck-after` (default 600s ≈ 10 min) of monitoring with no
    verdict it **escalates once** with an explicit `@codex review`. The default is
    deliberately generous — a healthy Codex review can genuinely take several minutes,
    and escalating too eagerly re-introduces the double-trigger this mode exists to
    avoid. A merely-slow review that lands first means the watchdog never fires; set
    `--stuck-after 0` to disable it. Keep `--stuck-after` comfortably below the overall
    `--max-polls × --interval` budget (default 30 min) so the escalated review still has
    time to land.
- `--repo` defaults to the current repo (`gh repo view`). Needs `gh` authenticated.
- Tunables: `--interval` (default 45s), `--max-polls` (default 40 ≈ 30 min),
  `--auto-wait` (default 90s) + `--stuck-after` (default 600s ≈ 10 min), both
  `--first-review` only.

For a **standalone quota check** (no PR, no trigger, consumes no quota):

```bash
uv run .agents/skills/codex-review/watch.py --quota-only
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
  Codex never picked the trigger up — the watcher warns on that. The ack attaches to the
  **trigger comment** for a manual `@codex review`, but to the **PR body** for the
  auto-started first review (which has no comment) — `reactions()` reads whichever
  applies.
- **The first review auto-starts; subsequent ones don't.** On PR open Codex usually
  posts a review on its own (the 👀-on-body auto-start), so for the first review pass
  `--first-review` to detect it instead of double-triggering. The auto-start is *only*
  on PR open — after every push, re-post `@codex review` (the default mode, no
  `--first-review`) to get the next review.
- **Writing `@codex` is itself a trigger — even inside backticks.** Per OpenAI's docs,
  `@codex review` requests a review, but `@codex` followed by *any other text* starts a
  cloud **task** (the autonomous agent) using the PR as context. It fires from a PR/issue
  **body or comment**, and (observed on this repo) backtick-quoting does **not** shield
  it: a PR description full of backticked `@codex review` mentions got routed to the task
  agent, which ran ~20 min then posted a phantom "committed / opened a follow-up PR"
  summary that never actually touched the repo. So when writing *about* the trigger (a PR
  body, an upstream issue), neutralize the mention — write `&#64;codex` (renders as
  `@codex`, no literal `@` token) — and only post a bare `@codex review` when you truly
  want a review.

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

**Never sleep until a "reset time."** The snapshot's reset timestamp is just where the
rolling window currently lands, not a fixed clock alarm — it moves. Do not schedule a
wait against a derived reset time (this cost ~23 idle minutes once); poll for the
verdict, or hand back to the user. And a **dropped trigger** (no 👀 in the first ~2 min,
no verdict) is **not** the same as rate-limited — re-trigger once immediately rather than
waiting out the full poll window, and confirm a real wall with `--quota-only` before
assuming quota.

## Handling review comments

When a verdict lands, drive each inline comment to a resolved state so the PR's open-thread
list shows only genuinely open items. Codex reviews the **code**, not the threads (it does
**not** read replies), so replies are for the human record and re-reviewing means re-running
this skill against new commits, never answering inline.

Pick one of three dispositions per comment, then **resolve the thread**:

- **Valid: action it.** Fix it in the code or docs, react **👍** on the comment (Codex
  treats the reaction as its feedback signal; its comments literally ask "Useful? React
  with 👍/👎"), then resolve the thread and re-run this skill on the new commit to confirm
  a clean verdict.
- **Factually wrong: 👎.** If the comment is provably untrue (not merely debatable),
  react **👎**, reply with the correction for the human record, then resolve the thread.
- **A trade-off or opinion we decline: no reaction.** Do not react either way; reply on
  the thread with the reasons we are not actioning it, then resolve. Put any durable
  rationale in the code or docs too (the only Codex-visible mitigation), since a re-review
  may re-raise it.

Resolving a thread needs **GraphQL** (`resolveReviewThread`); the REST API cannot. React
via REST, resolve via GraphQL:

```bash
# react 👍 (use content=-1 for 👎) to an inline comment by its REST id
gh api -X POST repos/<owner>/<repo>/pulls/comments/<comment-id>/reactions -f content=+1
# list unresolved threads (node id + first comment author), then resolve one by node id
gh api graphql -f query='query{repository(owner:"<owner>",name:"<repo>"){pullRequest(number:<PR>){reviewThreads(first:50){nodes{id isResolved comments(first:1){nodes{databaseId author{login}}}}}}}}'
gh api graphql -f query='mutation($id:ID!){resolveReviewThread(input:{threadId:$id}){thread{isResolved}}}' -F id=<thread-node-id>
```

Do not loop re-refuting an already-declined by-design item; defer to the maintainer's
merge call. When Codex re-raises the **same theme** with progressively narrower edges
(common on prose/docs, not only file-I/O), state the full precise behaviour **once**
comprehensively (or scope the bullet down) rather than patching per-edge, round by round.
Hitting the daily review quota is a legitimate convergence stop.
