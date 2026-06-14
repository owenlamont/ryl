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

"""Generate ``docs/llms.txt`` from the Zensical nav and docs pages.

Emits an llmstxt.org-format index (H1 site name, blockquote summary, one H2
section per nav group, one link per page) into ``docs/llms.txt``. Zensical copies
that file verbatim into the built site, so it is served at ``<site_url>/llms.txt``
for IDE agents that fetch docs live. Run with ``--check`` to verify the committed
file is current (exit 1 if it would change).
"""

from __future__ import annotations

from pathlib import Path
import re
from typing import Annotated, Final

import tomllib
import typer


ROOT: Final = Path(__file__).resolve().parents[1]
ZENSICAL: Final = ROOT / "zensical.toml"
DOCS: Final = ROOT / "docs"
OUTPUT: Final = DOCS / "llms.txt"

# Block prefixes that end the lead paragraph (heading, fence, list, quote, table).
BLOCK_PREFIXES: Final = ("#", "```", "-", "*", ">", "|")

# Markdown links: `[text](url)` and reference style `[text][ref]`. Both are unusable in
# the served /llms.txt (relative/reference targets do not resolve), so descriptions keep
# only the link text.
LINK: Final = re.compile(r"\[([^\]]+)\]\([^)]*\)|\[([^\]]+)\]\[[^\]]*\]")


def strip_links(text: str) -> str:
    """Replace Markdown links with their link text.

    Returns:
        The text with `[label](url)` / `[label][ref]` reduced to `label`.
    """
    return LINK.sub(lambda m: m.group(1) or m.group(2), text)


def first_paragraph(markdown: str) -> str:
    """Return the first prose paragraph after the page's H1, joined to one line.

    A leading HTML block (e.g. a Material card grid) is skipped, but a ``<...>``
    placeholder *within* prose is kept, so only HTML at the paragraph start is
    dropped.

    Returns:
        Consecutive prose lines following the H1 (up to the next blank line or
        non-prose block) joined with spaces, or an empty string when the page has
        no prose lead.
    """
    seen_h1 = False
    collected: list[str] = []
    for line in markdown.splitlines():
        stripped = line.strip()
        if not seen_h1:
            seen_h1 = stripped.startswith("# ")
            continue
        # A blank line, a block marker, or an HTML closing tag ends the lead paragraph.
        if not stripped or stripped.startswith((*BLOCK_PREFIXES, "</")):
            if collected:
                break
            continue
        # Skip a leading HTML open tag (best-effort, not a full HTML parser); the only
        # page that opens with an HTML block is the card-grid landing page, which the
        # caller omits from the index. A `<placeholder>` within prose is kept.
        if stripped.startswith("<") and not collected:
            continue
        collected.append(stripped)
    return " ".join(collected)


def page_link(label: str, rel_path: str, site_url: str) -> str:
    """Build one llmstxt list item for a docs page.

    Returns:
        A ``- [label](url): description`` line (description omitted when absent).
    """
    slug = rel_path.removesuffix(".md")
    url = site_url if slug == "index" else f"{site_url}{slug}/"
    desc = strip_links(first_paragraph((DOCS / rel_path).read_text(encoding="utf-8")))
    item = f"- [{label}]({url})"
    return f"{item}: {desc}" if desc else item


def render(config: dict) -> str:
    """Render the full ``llms.txt`` body from the parsed Zensical config.

    Returns:
        The llmstxt.org-format document, newline-terminated.
    """
    project = config["project"]
    site_url = project["site_url"].rstrip("/") + "/"
    lines = [f"# {project['site_name']}", "", f"> {project['site_description']}", ""]
    loose: list[str] = []
    for entry in project["nav"]:
        label, value = next(iter(entry.items()))
        if isinstance(value, list):
            lines += [f"## {label}", ""]
            lines += [page_link(*next(iter(sub.items())), site_url) for sub in value]
            lines.append("")
        elif value != "index.md":
            # The landing page is a navigational card grid, not useful doc-index prose.
            loose.append(page_link(label, value, site_url))
    if loose:
        lines += ["## Other pages", "", *loose, ""]
    return "\n".join(lines).rstrip() + "\n"


def main(
    *,
    check: Annotated[
        bool,
        typer.Option(help="Fail (exit 1) if the committed docs/llms.txt is stale."),
    ] = False,
) -> None:
    """Generate ``docs/llms.txt``, or verify it is current with ``--check``.

    Raises:
        typer.Exit: code 1 when ``--check`` finds ``docs/llms.txt`` stale.
    """
    content = render(tomllib.loads(ZENSICAL.read_text(encoding="utf-8")))
    if check:
        # Read raw (newline="") so a CRLF-divergent committed file is detected, not
        # silently normalized to match the always-LF rendered content.
        current = (
            OUTPUT.read_text(encoding="utf-8", newline="") if OUTPUT.is_file() else ""
        )
        if current != content:
            typer.echo(
                "docs/llms.txt is stale; run `uv run scripts/gen_llms_txt.py`", err=True
            )
            raise typer.Exit(code=1)
        return
    # Always write LF regardless of platform so the output is deterministic.
    OUTPUT.write_text(content, encoding="utf-8", newline="\n")


if __name__ == "__main__":
    typer.run(main)
