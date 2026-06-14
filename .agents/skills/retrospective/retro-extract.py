#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.14"
# dependencies = ["typer"]
#
# [tool.uv]
# # One-week dependency cooldown (rolling): ignore releases newer than one week
# # before each run as a supply-chain-safety buffer.
# exclude-newer = "1 week ago"
# ///

"""Cheap, deterministic friction extraction from Claude Code session transcripts.

The brute-force retrospective (one classifier agent per raw multi-MB transcript) is
expensive and noisy. This script does the *measurable* work with no LLM: it streams each
JSONL transcript once and emits a compact per-session digest of only the candidate
friction — hard counts plus the few lines worth classifying — so a small number of
classifier agents can run over the digests instead of the raw transcripts (~10-20x
cheaper).

Signals (all derived from the transcript, no model needed):

- tool errors: ``tool_result`` blocks with ``is_error: true``.
- **wall-clock waits**: every event is ISO-timestamped, so the gap between a ``tool_use``
  and its ``tool_result`` is the tool's real duration. A long wait that ends in an
  error/cancellation is the worst friction (a long block on a broken tool) and is
  reported first — a signal pure text classification cannot see.
- retry clusters: the same Bash command re-issued back-to-back, with no intervening
  tool or human turn (a blind retry — trial-and-error, not normal edit-then-rerun).
- user corrections: human turns carrying redirection markers ("no", "actually", "wrong",
  "instead", "revert", ...).

With no ``--project-dir`` it derives the transcript directory from the current working
directory the way Claude Code names it (``~/.claude/projects/<cwd with / and . replaced
by ->``). By default it analyzes the top-level (user-facing) session transcripts and
reports how many nested subagent/workflow transcripts it skipped; ``--include-nested``
adds those (their autonomous tool-churn would otherwise dominate the user-friction
signal). Prints a cross-session summary; with ``--out-dir`` it also writes one
``<session>.digest.json`` per session for the classifier-agent fan-out.
"""

from __future__ import annotations

from collections.abc import Iterator
from dataclasses import dataclass, field
from datetime import datetime
import json
from operator import itemgetter
from pathlib import Path
import sys
from typing import Annotated, Final

import typer


# Lower-cased substrings that mark a human turn as a course-correction.
CORRECTION_MARKERS: Final = (
    "no,",
    "don't",
    "do not",
    "actually",
    "that's not",
    "thats not",
    "wrong",
    "instead",
    "revert",
    "i meant",
    "should be",
    "not quite",
    "stop",
)

# Default wall-clock gap (seconds) at or above which a tool_use->result wait is flagged.
DEFAULT_WAIT_THRESHOLD: Final = 120.0

# Tools whose wait is the human responding, not the tool running. Their gaps measure
# how long you were away — not friction — so they are excluded from the wait analysis.
INTERACTIVE_TOOLS: Final = frozenset({"AskUserQuestion", "ExitPlanMode"})

# Manifest this tool writes under --out-dir listing the digests it produced, so a later
# run removes only its own files and never an unrelated *.digest.json kept there.
MANIFEST_NAME: Final = ".retro-manifest.json"


def parse_ts(value: str | None) -> datetime | None:
    """Parse an ISO-8601 transcript timestamp (``...Z``) to a datetime.

    Returns:
        The parsed ``datetime``, or ``None`` when absent or unparsable.
    """
    if not value:
        return None
    try:
        return datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None


@dataclass
class SessionDigest:
    """The compact, classifiable summary of one transcript."""

    session: str
    first_ts: str | None = None
    last_ts: str | None = None
    events: int = 0
    tool_uses: int = 0
    tool_errors: int = 0
    long_waits: list[dict] = field(default_factory=list)
    retries: list[dict] = field(default_factory=list)
    error_samples: list[str] = field(default_factory=list)
    correction_samples: list[str] = field(default_factory=list)

    def as_json(self) -> dict:
        """Render the digest as a JSON-serializable dict.

        Returns:
            The digest with its long-wait/retry lists sorted by impact and every
            sample list capped, so the written file stays small.
        """
        return {
            "session": self.session,
            "first_ts": self.first_ts,
            "last_ts": self.last_ts,
            "events": self.events,
            "tool_uses": self.tool_uses,
            "tool_errors": self.tool_errors,
            "long_waits": sorted(
                self.long_waits, key=itemgetter("seconds"), reverse=True
            )[:15],
            "retries": sorted(self.retries, key=itemgetter("count"), reverse=True)[:10],
            "error_samples": self.error_samples[:20],
            "correction_samples": self.correction_samples[:20],
        }


def blocks(message: dict) -> list[dict]:
    """Return a message's content as a list of blocks.

    Returns:
        The content list, or ``[]`` when the content is plain text (a string).
    """
    content = message.get("content")
    return content if isinstance(content, list) else []


def text_of(message: dict) -> str:
    """Extract the plain text of a user message.

    Returns:
        The string content, or the joined text blocks, or ``""``.
    """
    content = message.get("content")
    if isinstance(content, str):
        return content
    return " ".join(
        block.get("text", "")
        for block in blocks(message)
        if block.get("type") == "text"
    )


def note_retry(digest: SessionDigest, command: str, cluster: dict | None) -> dict:
    """Record one back-to-back repeat of ``command``, extending or opening a cluster.

    Each maximal run of consecutive identical Bash commands is its own cluster, so two
    runs of the same command separated by a break are counted separately, not merged.

    Returns:
        The open cluster dict, to thread into the next call.
    """
    if cluster is not None:
        cluster["count"] += 1
        return cluster
    opened = {"cmd": command.strip().replace("\n", " ")[:120], "count": 2}
    digest.retries.append(opened)
    return opened


def add_error_sample(digest: SessionDigest, block: dict) -> None:
    """Capture a short, single-line snippet of an errored tool result."""
    content = block.get("content")
    if isinstance(content, list):
        content = " ".join(
            part.get("text", "") for part in content if isinstance(part, dict)
        )
    snippet = str(content).strip().replace("\n", " ")[:200]
    if snippet:
        digest.error_samples.append(snippet)


def record_wait(
    digest: SessionDigest,
    pending: dict[str, tuple[datetime, str]],
    block: dict,
    end: datetime | None,
    *,
    is_error: bool,
    threshold: float,
) -> None:
    """Record a tool_use->tool_result wall-clock gap that meets ``threshold``."""
    start_entry = pending.pop(block.get("tool_use_id", ""), None)
    if start_entry is None or end is None:
        return
    start, tool = start_entry
    if tool in INTERACTIVE_TOOLS:  # the wait is the human, not the tool
        return
    seconds = (end - start).total_seconds()
    if seconds >= threshold:
        digest.long_waits.append(
            {
                "tool": tool,
                "seconds": round(seconds, 1),
                "ended_in_error": is_error,
                "at": start.isoformat(),
            }
        )


def scan_correction(digest: SessionDigest, text: str) -> None:
    """Capture a short snippet of a human turn that looks like a course-correction."""
    lowered = text.lower()
    if any(marker in lowered for marker in CORRECTION_MARKERS):
        digest.correction_samples.append(text.strip().replace("\n", " ")[:200])


def stream_lines(path: Path) -> Iterator[str]:
    """Yield the transcript's lines lazily, closing the file when the iterator is done.

    Yields:
        Each raw JSONL line, without materializing the whole multi-MB file at once.
    """
    with path.open(encoding="utf-8", errors="replace") as handle:
        yield from handle


def extract(path: Path, label: str, threshold: float) -> SessionDigest:
    """Stream one transcript and build its digest in a single pass.

    Returns:
        The populated [`SessionDigest`] for ``path``, identified by ``label``.
    """
    digest = SessionDigest(session=label)
    pending: dict[str, tuple[datetime, str]] = {}  # tool_use_id -> (start, tool name)
    last_command: str | None = None
    cluster: dict | None = None  # the open back-to-back retry cluster, if any
    seen_human = False  # the first human turn is the initial request, not a correction
    for raw in stream_lines(path):
        try:
            event = json.loads(raw)
        except json.JSONDecodeError:
            continue
        if not isinstance(event, dict):  # a valid-JSON non-object line has no .get
            continue
        digest.events += 1
        timestamp = parse_ts(event.get("timestamp"))
        if event.get("timestamp"):
            digest.first_ts = digest.first_ts or event["timestamp"]
            digest.last_ts = event["timestamp"]
        message = event.get("message")
        if not isinstance(message, dict):
            continue
        for block in blocks(message):
            kind = block.get("type")
            if kind == "tool_use":
                digest.tool_uses += 1
                block_id = block.get("id")
                if (
                    timestamp is not None and block_id
                ):  # skip id-less blocks (no "" key)
                    pending[block_id] = (timestamp, block.get("name", "?"))
                if block.get("name") == "Bash":
                    command = (block.get("input") or {}).get("command", "")
                    if command and command == last_command:
                        cluster = note_retry(digest, command, cluster)
                    else:
                        cluster = None  # a different command ends the current cluster
                    last_command = command or None
                else:
                    # A non-Bash tool between two identical Bash commands breaks the
                    # back-to-back run, so the later one is iteration, not a blind retry.
                    last_command = None
                    cluster = None
            elif kind == "tool_result":
                is_error = bool(block.get("is_error"))
                if is_error:
                    digest.tool_errors += 1
                    add_error_sample(digest, block)
                record_wait(
                    digest,
                    pending,
                    block,
                    timestamp,
                    is_error=is_error,
                    threshold=threshold,
                )
        if event.get("type") == "user":
            text = text_of(message)
            if text.strip():  # a human turn (not a tool_result delivery) ends a run
                last_command = None
                cluster = None
                # Skip the first human turn: it is the initial request, not a correction.
                if seen_human:
                    scan_correction(digest, text)
                seen_human = True
    return digest


def default_project_dir() -> Path:
    """Locate Claude Code's transcript directory for the current working directory.

    Returns:
        ``~/.claude/projects/<cwd with every non-alphanumeric char replaced by ->``
        (Claude Code's naming: ``/``, ``.``, ``_`` all become ``-``).
    """
    mangled = "".join(c if c.isalnum() else "-" for c in str(Path.cwd()))
    return Path.home() / ".claude" / "projects" / mangled


def label_for(path: Path, project_dir: Path) -> str:
    """Build a collision-free session label from ``path`` relative to ``project_dir``.

    Returns:
        The forward-slashed relative path — a bare filename for a top-level session, or
        ``<session>/subagents/...`` for a nested one — unique across the tree.
    """
    return path.relative_to(project_dir).as_posix()


def select_transcripts(
    project_dir: Path, since: str | None, *, include_nested: bool
) -> tuple[list[Path], int]:
    """Find the transcripts to analyze under ``project_dir``.

    Top-level ``*.jsonl`` are the user-facing sessions. Nested transcripts (under
    ``<session>/subagents/`` and ``<session>/workflows/``) are autonomous sub-agent and
    workflow runs, included only with ``include_nested`` — their churn would otherwise
    dominate the user-friction signal (a hung sub-agent already shows as an ``Agent``
    wait in its parent).

    Returns:
        ``(files, skipped_nested)`` — the selected files (optionally only those modified
        on/after ``since``, oldest-modified first) and the count of nested transcripts
        skipped when ``include_nested`` is false (0 otherwise), so the omission is
        reported rather than silent.

    Raises:
        typer.Exit: code 1 when ``project_dir`` does not exist or ``since`` is not a
            valid ISO date.
    """
    if not project_dir.is_dir():
        typer.echo(f"transcript directory not found: {project_dir}", err=True)
        raise typer.Exit(code=1)
    # `since` is a date in local time (its epoch is compared to st_mtime); a typo gets a
    # clean exit, matching the rest of the script rather than a raw traceback.
    cutoff: float | None = None
    if since:
        try:
            cutoff = datetime.fromisoformat(since).timestamp()
        except ValueError:
            typer.echo(
                f"invalid --since (use an ISO date, e.g. 2026-06-01): {since}", err=True
            )
            raise typer.Exit(code=1) from None
    matched = [
        path
        for path in project_dir.rglob("*.jsonl")
        if cutoff is None or path.stat().st_mtime >= cutoff
    ]
    by_mtime = sorted(matched, key=lambda path: path.stat().st_mtime)
    if include_nested:
        return by_mtime, 0
    top_level = [p for p in by_mtime if len(p.relative_to(project_dir).parts) == 1]
    return top_level, len(by_mtime) - len(top_level)


def summary_lines(digests: list[SessionDigest]) -> list[str]:
    """Build the cross-session friction summary.

    Returns:
        The summary as a list of lines (totals, then the longest waits, with the
        error-ending waits — the most frustrating — called out first).
    """
    waits = [{**wait, "session": d.session} for d in digests for wait in d.long_waits]
    broken = [wait for wait in waits if wait["ended_in_error"]]
    lines = [
        f"Sessions: {len(digests)}   tool_uses: {sum(d.tool_uses for d in digests)}",
        (
            f"Tool errors: {sum(d.tool_errors for d in digests)}   "
            f"retry clusters: {sum(len(d.retries) for d in digests)}"
        ),
        (
            f"Long waits (>=threshold): {len(waits)}   "
            f"of which ended in error/cancel: {len(broken)}"
        ),
    ]
    if broken:
        lines.append("\nLongest waits that ended in an error/cancellation (worst):")
        lines.extend(
            f"  {wait['seconds']:>8}s  {wait['tool']:<12} {wait['session']}"
            for wait in sorted(broken, key=itemgetter("seconds"), reverse=True)[:10]
        )
    if waits:
        lines.append("\nLongest waits overall:")
        for wait in sorted(waits, key=itemgetter("seconds"), reverse=True)[:10]:
            flag = " (errored)" if wait["ended_in_error"] else ""
            lines.append(
                f"  {wait['seconds']:>8}s  {wait['tool']:<12} {wait['session']}{flag}"
            )
    return lines


def main(
    *,
    since: Annotated[
        str | None,
        typer.Option(
            help="Only sessions modified on/after this local ISO date (YYYY-MM-DD)."
        ),
    ] = None,
    project_dir: Annotated[
        Path | None,
        typer.Option(help="Transcript directory (default: derived from the cwd)."),
    ] = None,
    out_dir: Annotated[
        Path | None,
        typer.Option(
            help="Write one <session>.digest.json here for the agent fan-out."
        ),
    ] = None,
    wait_threshold: Annotated[
        float,
        typer.Option(help="Flag tool_use->result gaps at or above this many seconds."),
    ] = DEFAULT_WAIT_THRESHOLD,
    include_nested: Annotated[
        bool,
        typer.Option(
            help="Also analyze nested subagent/workflow transcripts "
            "(default: top-level sessions only)."
        ),
    ] = False,
) -> None:
    """Extract per-session friction digests and print a cross-session summary.

    Raises:
        typer.Exit: code 1 when the transcript directory or matching sessions are
            missing (via [`select_transcripts`] or the empty-match guard).
    """
    root = project_dir or default_project_dir()
    transcripts, skipped_nested = select_transcripts(
        root, since, include_nested=include_nested
    )
    if not transcripts:
        if skipped_nested:
            typer.echo(
                f"no top-level transcripts matched, but {skipped_nested} nested "
                "subagent/workflow transcript(s) exist; pass --include-nested to "
                "analyze them",
                err=True,
            )
        else:
            typer.echo("no transcripts matched", err=True)
        raise typer.Exit(code=1)

    digests = [
        extract(path, label_for(path, root), wait_threshold) for path in transcripts
    ]
    typer.echo("\n".join(summary_lines(digests)))
    if skipped_nested:
        typer.echo(
            f"\n(skipped {skipped_nested} nested subagent/workflow transcript(s); "
            "pass --include-nested to analyze them)"
        )

    if out_dir is not None:
        out_dir.mkdir(parents=True, exist_ok=True)
        # Remove only the digests a PRIOR run recorded in its manifest, so the directory
        # reflects exactly this run's selection without ever deleting unrelated
        # *.digest.json a user may keep under out_dir (e.g. --out-dir .).
        manifest = out_dir / MANIFEST_NAME
        if manifest.is_file():
            try:
                prior = json.loads(manifest.read_text(encoding="utf-8"))
            except (json.JSONDecodeError, OSError):
                prior = []
            resolved_root = out_dir.resolve()
            for relative in prior:
                if not isinstance(relative, str) or not relative.endswith(
                    ".digest.json"
                ):
                    continue
                candidate = (out_dir / relative).resolve()
                # Unlink only this tool's digest files that stay under out_dir, so a
                # tampered/foreign manifest can't delete an unrelated file — neither a
                # non-digest path (e.g. Cargo.toml) nor one escaping via absolute/`..`.
                if candidate.is_relative_to(resolved_root):
                    candidate.unlink(missing_ok=True)
        written: list[str] = []
        for digest in digests:
            # Mirror the transcript's relative path under out_dir so nested sessions
            # cannot collide (flattening `/`->`-` is not injective). `digest.session`
            # is the forward-slashed relative path; pathlib splits it into components.
            relative = f"{digest.session}.digest.json"
            target = out_dir / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(json.dumps(digest.as_json(), indent=2), encoding="utf-8")
            written.append(relative)
        manifest.write_text(json.dumps(written, indent=2), encoding="utf-8")
        typer.echo(f"\nWrote {len(digests)} digests to {out_dir}")


if __name__ == "__main__":
    # Windows stdio defaults to cp1252; transcript text in the printed summary
    # carries non-ASCII, so force UTF-8 to avoid an encode crash.
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")
    typer.run(main)
