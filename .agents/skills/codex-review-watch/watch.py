# /// script
# requires-python = ">=3.14"
# dependencies = ["typer"]
# ///
"""Trigger and/or monitor a Codex CI code review on a GitHub PR, then classify the
verdict. Run with: ``uv run watch.py <pr> [--repo owner/repo] [--no-trigger]``
(add ``--first-review`` for the first review on a freshly-opened PR), or
``uv run watch.py --quota-only`` to just print the Codex quota snapshot.

Each gotcha below was a real mistake this script exists to prevent:
  * Poll the REST API via ``gh api repos/...`` -- NOT ``gh pr view --json``, whose
    GraphQL renders the bot login differently so a login filter silently matches
    nothing and the poller runs blind.
  * A Codex verdict lands on ONE of three channels: a new PR REVIEW (findings, with
    inline P-badged comments), a new ISSUE COMMENT ("no major issues" = clean, or a
    usage-limit notice), or a thumbs-up on the trigger comment. Watch ALL THREE --
    watching only reviews misses the clean issue-comment verdict.
  * The eyes ack is transient (added, then removed when the verdict posts), so it is
    never the completion signal -- but its ABSENCE within a minute means Codex never
    picked the trigger up (rate-limited/stalled), which this fails fast on
    (``--ack-timeout``, default 60s) instead of polling the full window.
  * Quota: the cloud ``@codex review`` shares your ChatGPT account quota with the
    local Codex CLI. There is no non-consuming quota *command* (``/status`` is
    TUI-only), but the CLI caches the rate-limit snapshot from each response into its
    session rollout, which this reads as a preflight so a near-exhausted quota is
    flagged before a trigger silently stalls.
  * On a freshly-opened PR Codex usually AUTO-starts a review (no @codex review
    comment), acked by a transient eyes reaction on the PR BODY (the manual-prompt
    eyes land on the @codex review comment instead). ``--first-review`` watches for
    that auto-start -- a body eyes ack or a verdict landing within ``--auto-wait`` --
    and skips re-posting @codex review, falling back to posting only if no auto-start
    fires. Leave it OFF for subsequent (post-push) reviews: those have no auto-start
    and need an explicit @codex review.
  * The auto-review also fails the OTHER way: it acks (eyes) then silently STALLS
    without posting a verdict (Codex's "stuck eyes" flake). So under ``--first-review``
    the eyes ack is trusted, but not forever -- after ``--stuck-after``s of monitoring
    with no verdict the watchdog escalates once with an explicit @codex review.
"""

from __future__ import annotations

from datetime import datetime, UTC
import json
from pathlib import Path
import re
import subprocess
import sys
import time
from typing import Annotated

import typer


BOT = "chatgpt-codex-connector[bot]"
DASHBOARD = "https://chatgpt.com/codex/settings/usage"


def gh(*args: str) -> str:
    result = subprocess.run(
        ["gh", *args], capture_output=True, text=True, encoding="utf-8"
    )
    if result.returncode != 0:
        # A swallowed gh failure (auth, transient API error, rejected post) would let an
        # old verdict look new or reuse a stale trigger, so abort loudly instead.
        typer.echo(
            f"gh {' '.join(args)} failed (exit {result.returncode}): "
            f"{result.stderr.strip()}",
            err=True,
        )
        raise typer.Exit(code=1)
    return result.stdout


def gh_json(path: str) -> list:
    # --paginate reads every page (array endpoints default to 30 results/page) and
    # --slurp wraps the pages in one outer array; flatten back to a flat item list.
    out = gh("api", "--paginate", "--slurp", path)
    try:
        pages = json.loads(out) if out.strip() else []
    except json.JSONDecodeError:
        return []
    return [item for page in pages for item in page]


def by_bot(items: list) -> list:
    return [x for x in items if (x.get("user") or {}).get("login") == BOT]


def _find_rate_limits(obj: object) -> dict | None:
    """Recursively locate a rate-limit dict (has `primary`/`secondary`) in a parsed
    session-rollout line.
    """
    if isinstance(obj, dict):
        if isinstance(obj.get("primary"), dict) and "secondary" in obj:
            return obj
        if isinstance(obj.get("rate_limits"), dict):
            return obj["rate_limits"]
        for value in obj.values():
            found = _find_rate_limits(value)
            if found:
                return found
    elif isinstance(obj, list):
        for value in obj:
            found = _find_rate_limits(value)
            if found:
                return found
    return None


def read_quota() -> dict | None:
    """Return the most recent Codex rate-limit snapshot the CLI cached in its session
    rollouts (the same data the TUI `/status` shows), with an added `_mtime`. The cloud
    `@codex review` shares this account quota. May be stale (only as fresh as the last
    local Codex run); returns None when no snapshot is found.
    """
    sessions = Path.home() / ".codex" / "sessions"
    if not sessions.is_dir():
        return None
    files = sorted(
        sessions.rglob("rollout-*.jsonl"), key=lambda p: p.stat().st_mtime, reverse=True
    )
    for path in files[:10]:
        found = None
        try:
            text = path.read_text(encoding="utf-8")
        except OSError:
            continue
        for line in text.splitlines():
            if '"primary"' not in line and '"rate_limits"' not in line:
                continue
            try:
                snapshot = _find_rate_limits(json.loads(line))
            except json.JSONDecodeError:
                continue
            if snapshot and isinstance(snapshot.get("primary"), dict):
                found = snapshot
        if found:
            found["_mtime"] = path.stat().st_mtime
            return found
    return None


def _window_line(window: object) -> str | None:
    if not isinstance(window, dict):
        return None
    name = {300: "5-hour", 10080: "weekly"}.get(
        window.get("window_minutes"), f"{window.get('window_minutes')}min"
    )
    resets = window.get("resets_at")
    when = (
        datetime.fromtimestamp(resets, tz=UTC).strftime("%Y-%m-%d %H:%M UTC")
        if isinstance(resets, int | float)
        else "?"
    )
    return f"{name} window: {window.get('used_percent')}% used (resets {when})"


def report_quota(quota: dict | None) -> bool:
    """Print the quota snapshot; return True when a window looks near/at its limit."""
    if quota is None:
        typer.echo("quota: unknown (no local Codex session snapshot found)")
        return False
    age_min = (time.time() - quota.get("_mtime", time.time())) / 60
    typer.echo(
        f"quota snapshot (plan={quota.get('plan_type')}, ~{age_min:.0f} min old; "
        "shared with cloud @codex review):"
    )
    for key in ("primary", "secondary"):
        line = _window_line(quota.get(key))
        if line:
            typer.echo(f"  {line}")
    reached = quota.get("rate_limit_reached_type")
    near = any(
        isinstance(quota.get(k), dict) and (quota[k].get("used_percent") or 0) >= 90
        for k in ("primary", "secondary")
    )
    if reached or near:
        typer.echo(
            f"  WARNING: a Codex window is near/at its limit -- @codex review may "
            f"silently stall (no eyes ack, no verdict). Dashboard: {DASHBOARD}"
        )
        return True
    return False


def summarize_finding(body: str) -> str:
    """Pull the P-level + title out of an inline review comment body, which looks like
    ``**<sub><sub>![P2 Badge](...)</sub></sub>  Title**`` on its first line.
    """
    level_match = re.search(r"P([0-9])\b", body)
    level = f"P{level_match.group(1)}" if level_match else "?"
    first = body.splitlines()[0] if body else ""
    title = re.sub(r"!\[[^\]]*\]\([^)]*\)", "", first)  # drop the badge image
    title = re.sub(r"</?sub>|\*\*", "", title)  # drop <sub>/</sub>/**
    title = re.sub(r"\s+", " ", title).strip()
    return f"[{level}] {title[:200]}"


def main(
    pr: Annotated[str, typer.Argument(help="pull request number")] = "",
    repo: Annotated[str, typer.Option(help="owner/repo (default: current repo)")] = "",
    trigger: Annotated[
        bool,
        typer.Option(
            help="post '@codex review' first (use --no-trigger to only monitor)"
        ),
    ] = True,
    first_review: Annotated[
        bool,
        typer.Option(
            help="first review on a freshly-opened PR: detect Codex's auto-started "
            "review (transient eyes on the PR body) before posting '@codex review', "
            "to avoid double-triggering. Leave OFF for subsequent (post-push) reviews."
        ),
    ] = False,
    quota_only: Annotated[
        bool, typer.Option(help="print the Codex quota snapshot and exit")
    ] = False,
    max_polls: Annotated[int, typer.Option(help="maximum poll attempts")] = 40,
    interval: Annotated[int, typer.Option(help="seconds between polls")] = 45,
    auto_wait: Annotated[
        int,
        typer.Option(
            help="--first-review: seconds to wait for the auto-started review's ack "
            "(eyes on the PR body) before falling back to posting '@codex review'"
        ),
    ] = 90,
    stuck_after: Annotated[
        int,
        typer.Option(
            help="--first-review: if a detected auto-review posts no verdict within "
            "this many seconds of monitoring, escalate once with '@codex review' "
            "(handles Codex's stuck-eyes flake). Generous by default since a healthy "
            "review can genuinely run several minutes. 0 disables the escalation."
        ),
    ] = 600,
    ack_timeout: Annotated[
        int,
        typer.Option(
            help="seconds to wait for Codex's eyes/thumbs ack before giving up "
            "(no ack within this window = rate-limited / not picked up)"
        ),
    ] = 60,
    ack_interval: Annotated[
        int, typer.Option(help="seconds between ack-wait polls")
    ] = 15,
) -> None:
    """Watch a Codex CI review on a GitHub PR and classify the verdict."""
    # Windows stdout/stderr default to cp1252; the Codex verdict and quota text
    # carry non-ASCII (emoji, P-badges), so force UTF-8 to avoid an encode crash.
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")
    quota = read_quota()
    if quota_only:
        report_quota(quota)
        return
    if not pr:
        typer.echo("a PR number is required (or pass --quota-only)", err=True)
        raise typer.Exit(code=2)
    report_quota(quota)

    repo = (
        repo
        or gh("repo", "view", "--json", "nameWithOwner", "-q", ".nameWithOwner").strip()
    )
    if not repo:
        typer.echo("could not determine repo; pass --repo owner/repo", err=True)
        raise typer.Exit(code=2)
    typer.echo(f"repo={repo} pr={pr} trigger={trigger}")

    def reviews() -> list:
        return by_bot(gh_json(f"repos/{repo}/pulls/{pr}/reviews"))

    def issues() -> list:
        return by_bot(gh_json(f"repos/{repo}/issues/{pr}/comments"))

    def inlines() -> list:
        return by_bot(gh_json(f"repos/{repo}/pulls/{pr}/comments"))

    def latest_trigger_id() -> int | None:
        """Id of the most recent '@codex review' comment (ours or a human's), or None."""
        triggers = [
            c
            for c in gh_json(f"repos/{repo}/issues/{pr}/comments")
            if c.get("body") == "@codex review"
        ]
        return triggers[-1]["id"] if triggers else None

    def post_trigger() -> int | None:
        gh("pr", "comment", pr, "--repo", repo, "--body", "@codex review")
        return latest_trigger_id()

    auto_started = False
    # Set once a '@codex review' comment exists (posted by us or found in --no-trigger).
    trig_id: int | None = None

    if first_review:
        # A freshly-opened PR: Codex has produced nothing yet, so baseline every bot
        # channel at zero -- any bot review/comment/reaction below IS the auto-review.
        base_rev = base_iss = base_inl = 0
    else:
        base_rev, base_iss, base_inl = len(reviews()), len(issues()), len(inlines())
    typer.echo(
        f"baseline: reviews={base_rev} issue_comments={base_iss} inline={base_inl}"
    )

    def reactions() -> tuple[int, int]:
        """(thumbs, eyes) from the bot on the verdict's ack target: the '@codex review'
        trigger comment when one exists, else the PR body (Codex's auto-started review
        on PR open has no comment, so it acks on the body).
        """
        path = (
            f"repos/{repo}/issues/comments/{trig_id}/reactions"
            if trig_id
            else f"repos/{repo}/issues/{pr}/reactions"
        )
        reacts = by_bot(gh_json(path))
        return (
            sum(1 for r in reacts if r.get("content") == "+1"),
            sum(1 for r in reacts if r.get("content") == "eyes"),
        )

    def report_verdict(
        rev: list, iss: list, inl: list, thumbs: int, base_thumbs: int
    ) -> bool:
        """Print the verdict if one has landed on any of the three channels; return
        True when it has (caller should stop).
        """
        if len(iss) > base_iss:  # new bot issue comment = clean all-clear or a notice
            body = iss[-1].get("body", "")
            low = body.lower()
            first = body.splitlines()[0] if body else ""
            if "usage limit" in low:
                typer.echo(f"RESULT: RATE-LIMITED -- {first}")
            elif "no major issues" in low:
                typer.echo(f"RESULT: CLEAN -- {first}")
            else:
                typer.echo(f"RESULT: ISSUE-COMMENT (read it) -- {body[:300]}")
            return True
        if thumbs > base_thumbs:
            typer.echo("RESULT: CLEAN -- thumbs-up ack")
            return True
        if len(rev) > base_rev:  # new review = findings (inline comments)
            new = inl[base_inl:]
            typer.echo(f"RESULT: FINDINGS -- {len(new)} new inline comment(s):")
            for c in new:
                typer.echo(
                    f"  {summarize_finding(c.get('body', ''))}"
                    f"  @ {c.get('path')}:{c.get('line')}"
                )
            return True
        return False

    # First review on a freshly-opened PR: Codex usually AUTO-starts a review, acked by
    # a transient eyes reaction on the PR body (no '@codex review' comment). Detect that
    # before posting so we don't double-trigger it; only fall back to posting if no
    # auto-start fires within --auto-wait.
    if first_review:
        # Zero the body-thumbs baseline like every other channel: a fresh PR carries no
        # prior Codex output, so any bot thumbs-up on the body IS the auto-review's clean
        # verdict -- including one that already landed before this watcher started (eyes
        # already gone). Baselining it at the current count would miss that clean verdict
        # and fall back to a redundant @codex review.
        auto_base_thumbs = 0
        waited = 0
        while True:
            rev, iss, inl = reviews(), issues(), inlines()
            thumbs, eyes = reactions()
            if report_verdict(rev, iss, inl, thumbs, auto_base_thumbs):
                return  # the auto-review already landed (or finished before we looked)
            if eyes > 0:
                auto_started = True
                typer.echo(
                    f"auto-review underway (eyes on PR body, ~{waited}s); "
                    "not re-triggering"
                )
                break
            if waited >= auto_wait:
                typer.echo(f"auto-detect: no auto-started review within {auto_wait}s")
                break
            step = min(ack_interval, auto_wait - waited)
            time.sleep(step)
            waited += step
            typer.echo(f"auto-detect {waited}/{auto_wait}s: no auto-review ack yet")
        if auto_started:
            trigger = False  # the auto-review is running; monitor it, don't re-trigger

    # Post (or, in --no-trigger, just locate) the @codex review comment so reactions()
    # can see its verdict. auto-monitor mode (auto_started) posts nothing here -- it
    # relies on the running auto-review until the watchdog escalates.
    trig_id = post_trigger() if trigger else latest_trigger_id()
    if trigger:
        typer.echo(f"posted @codex review (trigger id={trig_id})")
    elif auto_started:
        typer.echo("monitoring Codex's auto-started review (no trigger posted)")
    elif trig_id:
        typer.echo(f"monitoring existing trigger id={trig_id}")
    else:
        typer.echo("monitoring without a trigger (no '@codex review' comment found)")

    # Baseline the ack target's reactions like the other channels. In --first-review the
    # baseline is zero (a fresh PR's body has no prior bot thumbs-up, and a just-posted
    # fallback trigger has none either), so an already-present clean thumbs-up still
    # counts. Otherwise an existing trigger may already carry one, so only a NEW
    # thumbs-up is a clean verdict.
    base_thumbs = 0 if first_review else reactions()[0]

    # Fail fast on a missing ack: Codex normally adds an eyes/thumbs reaction within a
    # minute of the trigger, so its ABSENCE within `ack_timeout`s means the trigger was
    # never picked up (rate-limited/stalled) -- bail instead of burning the full poll
    # window. A verdict that lands during this short phase short-circuits too. Skipped in
    # auto-review monitor mode (trig_id is None): we already saw the auto-start's eyes,
    # and that ack is transient, so its absence now is not a dropped trigger.
    if trig_id:
        waited = 0
        acked = False
        while waited < ack_timeout:
            step = min(ack_interval, ack_timeout - waited)
            time.sleep(step)
            waited += step
            rev, iss, inl = reviews(), issues(), inlines()
            thumbs, eyes = reactions()
            if report_verdict(rev, iss, inl, thumbs, base_thumbs):
                return
            if eyes > 0 or thumbs > 0:
                acked = True
                typer.echo(f"acked: Codex picked up the trigger (~{waited}s)")
                break
            typer.echo(f"ack wait {waited}/{ack_timeout}s: no eyes/thumbs yet")
        if not acked:
            typer.echo(
                f"RESULT: NOT ACKED -- no eyes/thumbs from Codex within {ack_timeout}s; "
                f"almost certainly rate-limited or not picked up (check {DASHBOARD})."
            )
            raise typer.Exit(code=1)

    # Acked (or --no-trigger / auto-review monitor): poll the three channels for verdict.
    # Watchdog: a detected auto-review that acked (eyes) then silently stalled (Codex's
    # stuck-eyes flake) would otherwise be monitored to timeout. After `stuck_after`s of
    # monitoring with no verdict, escalate ONCE with an explicit @codex review. Gated on
    # auto_started, so the normal/--no-trigger paths (which already have a trigger or
    # chose not to) never escalate.
    monitored = 0
    escalated = False
    for i in range(1, max_polls + 1):
        time.sleep(interval)
        monitored += interval
        rev, iss, inl = reviews(), issues(), inlines()
        thumbs, _eyes = reactions()
        if report_verdict(rev, iss, inl, thumbs, base_thumbs):
            return
        if auto_started and stuck_after and not escalated and monitored >= stuck_after:
            typer.echo(
                f"auto-review stalled (no verdict after {monitored}s); "
                "escalating with @codex review"
            )
            trig_id = post_trigger()
            base_thumbs, _ = reactions()
            escalated = True
            typer.echo(f"posted @codex review (trigger id={trig_id})")
            continue
        typer.echo(
            f"poll {i}/{max_polls}: reviews={len(rev)} issue={len(iss)} "
            f"inline={len(inl)} thumbs={thumbs} (no verdict)"
        )

    typer.echo(
        f"TIMED OUT -- Codex acked but slow to post a verdict (check {DASHBOARD})."
    )
    raise typer.Exit(code=1)


if __name__ == "__main__":
    typer.run(main)
