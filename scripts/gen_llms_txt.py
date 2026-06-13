#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.14"
# dependencies = []
# ///

"""Generate ``docs/llms.txt`` from the Zensical nav and docs pages.

Emits an llmstxt.org-format index (H1 site name, blockquote summary, one H2
section per nav group, one link per page) into ``docs/llms.txt``. Zensical copies
that file verbatim into the built site, so it is served at ``<site_url>/llms.txt``
for IDE agents that fetch docs live. Run with ``--check`` to verify the committed
file is current (exit 1 if it would change).
"""

from __future__ import annotations

import argparse
from pathlib import Path
import sys

import tomllib


ROOT = Path(__file__).resolve().parents[1]
ZENSICAL = ROOT / "zensical.toml"
DOCS = ROOT / "docs"
OUTPUT = DOCS / "llms.txt"


# Block prefixes that end the lead paragraph (heading, fence, list, quote, table).
BLOCK_PREFIXES = ("#", "```", "-", "*", ">", "|")


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
    desc = first_paragraph((DOCS / rel_path).read_text(encoding="utf-8"))
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


def main() -> None:
    """Generate ``docs/llms.txt``, or verify it is current with ``--check``."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check",
        action="store_true",
        help="fail (exit 1) if the committed docs/llms.txt is stale",
    )
    args = parser.parse_args()

    content = render(tomllib.loads(ZENSICAL.read_text(encoding="utf-8")))
    if args.check:
        # Read raw (newline="") so a CRLF-divergent committed file is detected, not
        # silently normalized to match the always-LF rendered content.
        current = (
            OUTPUT.read_text(encoding="utf-8", newline="") if OUTPUT.is_file() else ""
        )
        if current != content:
            sys.exit("docs/llms.txt is stale; run `uv run scripts/gen_llms_txt.py`")
        return
    # Always write LF regardless of platform so the output is deterministic.
    OUTPUT.write_text(content, encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
