---
name: retrospective
description: >-
  Use to run a development-friction retrospective over recent Claude Code
  sessions: a cheap deterministic extractor digests the transcripts (tool errors,
  wall-clock waits, retry clusters, corrections), then a small number of classifier
  agents tag the digests against a fixed taxonomy. Prefer this over fanning a
  classifier agent at each raw transcript (far cheaper, less noise).
---

# Retrospective Workflow

Goal: quantify development friction across recent sessions so process fixes (AGENTS.md,
dev skills, settings, tooling) target the biggest costs — not anecdotes.

**Cost lesson.** The first pass fanned one classifier agent at each of 30 raw multi-MB
transcripts: ~1.95M agent tokens, and the single largest finding category was
self-correcting noise. Do the measurable work *deterministically first*, then classify
small digests. That is ~10-20x cheaper and re-runnable every release.

## Workflow

1. **Extract (no LLM).** Run the helper over the sessions since the last release:

   ```bash
   uv run .agents/skills/retrospective/retro-extract.py --since 2026-06-01 \
       --out-dir <durable-dir>/retro-digests
   ```

   It streams each transcript once and emits per-session digests plus a summary:
   tool-error counts, **wall-clock waits** (the `tool_use`->`tool_result` gap; long waits
   that *ended in an error/cancellation* are surfaced first — a long block on a broken
   tool, the most frustrating friction and one pure text classification misses), retry
   clusters, and correction snippets. By default it covers the top-level (user-facing)
   sessions and reports the count of nested subagent/workflow transcripts it skipped;
   pass `--include-nested` to fold those in. Use a **durable** `--out-dir` (not `/tmp`,
   which is wiped between phases). Skim the summary first — it alone answers a lot.

2. **Iterate on one.** Before any fan-out, run a *single* classifier agent over *one*
   digest with the taxonomy below, eyeball its output, and tune the taxonomy/prompt
   (drop a noisy category, sharpen a definition). This catches a bad prompt for the cost
   of one agent, not N.

3. **Fan out small.** Then classify the digests with a *handful* of agents — group
   several digests per agent (not one agent per session, and never per raw transcript).
   Force a structured schema so the output is countable.

4. **Aggregate.** Tally by category, severity, and fix-target; rank by count x severity;
   separate already-closed gaps (verify they stayed) from still-open ones.

## Taxonomy

`failed_tool_call` (often self-correcting Edit-before-Read / post-`cargo fmt`
stale-string slips — **down-weight**, ~1 turn each), `long_wait_on_broken_tool` (from the
extractor's wall-clock data), `permission_or_sandbox_block`, `trial_and_error`,
`rediscovery`, `doc_gap`, `doc_stale`, `third_party_tool_friction`, `user_correction`,
`coverage_gate_struggle`, `proptest_struggle`, `schema_regen_churn`, `rework_after_review`,
`process_inefficiency`. Per finding capture severity, recurring?, root cause, and a
specific fix-target (which AGENTS.md section / skill / new skill / setting / tool).

The transcript JSONL: one event per line, each ISO-timestamped; assistant messages carry
`tool_use` blocks, tool results return as user-role messages whose content blocks have
`is_error`. The extractor handles the parsing — agents read the digests, not the raw files.
