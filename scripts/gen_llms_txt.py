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

"""Generate ``docs/llms.txt`` and ``docs/llms-full.txt`` from the Zensical docs.

``llms.txt`` is the llmstxt.org index (H1 site name, blockquote summary, one H2
section per nav group, one link + one-line description per page). ``llms-full.txt``
is the self-contained companion: every page's full Markdown concatenated in nav
order, useful because the index links resolve to HTML, not raw Markdown. Zensical
copies both into the built site, so they are served at ``<site_url>/llms.txt`` and
``<site_url>/llms-full.txt``. Run with ``--check`` to verify both committed files
are current (exit 1 if either would change).
"""

from __future__ import annotations

from pathlib import Path
import posixpath
import re
from typing import Annotated, Final

import tomllib
import typer


ROOT: Final = Path(__file__).resolve().parents[1]
ZENSICAL: Final = ROOT / "zensical.toml"
DOCS: Final = ROOT / "docs"
OUTPUT: Final = DOCS / "llms.txt"
OUTPUT_FULL: Final = DOCS / "llms-full.txt"

# Block prefixes that end the lead paragraph (heading, fence, list, quote, table).
BLOCK_PREFIXES: Final = ("#", "```", "-", "*", ">", "|")

# Markdown links: `[text](url)` and reference style `[text][ref]`. Both are unusable in
# the served /llms.txt descriptions (relative/reference targets do not resolve), so they
# keep only the link text.
LINK: Final = re.compile(r"\[([^\]]+)\]\([^)]*\)|\[([^\]]+)\]\[[^\]]*\]")

# Inline link `[text](target)`, for rewriting relative .md targets in the full feed.
INLINE_LINK: Final = re.compile(r"\[([^\]]+)\]\(([^)]+)\)")


def strip_links(text: str) -> str:
    """Replace Markdown links with their link text.

    Returns:
        The text with `[label](url)` / `[label][ref]` reduced to `label`.
    """
    return LINK.sub(lambda m: m.group(1) or m.group(2), text)


def absolutize_links(body: str, rel_path: str, site_url: str) -> str:
    """Rewrite a page's relative ``.md`` links to absolute deployed URLs.

    llms-full.txt concatenates pages, so a relative ``(other.md)`` would not resolve;
    each is rewritten to its served URL (preserving any ``#anchor``). Absolute,
    external, anchor-only, and non-``.md`` targets are left untouched.

    Returns:
        The body with relative ``.md`` links absolutized.
    """
    page_dir = posixpath.dirname(rel_path)

    def fix(match: re.Match) -> str:
        text, target = match.group(1), match.group(2)
        base, sep, anchor = target.partition("#")
        if not base.endswith(".md") or base.startswith(
            ("/", "http://", "https://", "mailto:")
        ):
            return match.group(0)
        resolved = posixpath.normpath(posixpath.join(page_dir, base))
        return f"[{text}]({page_url(resolved, site_url)}{sep}{anchor})"

    return INLINE_LINK.sub(fix, body)


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


def page_url(rel_path: str, site_url: str) -> str:
    """Return the deployed URL for a docs page path.

    Returns:
        ``<site_url>`` for the landing page, else ``<site_url><slug>/``.
    """
    slug = rel_path.removesuffix(".md")
    return site_url if slug == "index" else f"{site_url}{slug}/"


def page_link(label: str, rel_path: str, site_url: str) -> str:
    """Build one llmstxt index list item for a docs page.

    Returns:
        A ``- [label](url): description`` line (description omitted when absent).
    """
    desc = strip_links(first_paragraph((DOCS / rel_path).read_text(encoding="utf-8")))
    item = f"- [{label}]({page_url(rel_path, site_url)})"
    return f"{item}: {desc}" if desc else item


def nav_pages(nav: list) -> list[str]:
    """Return docs page paths in nav order, excluding the landing page.

    Returns:
        Flattened ``rel_path`` strings for every nav entry except ``index.md``.
    """
    pages: list[str] = []
    for entry in nav:
        value = next(iter(entry.values()))
        if isinstance(value, list):
            pages += [next(iter(sub.values())) for sub in value]
        elif value != "index.md":
            pages.append(value)
    return pages


def all_doc_pages(nav: list) -> list[str]:
    """All docs page paths for the full feed: nav order first, then remaining pages.

    Pages not listed in the nav (e.g. the per-rule pages under ``docs/rules/``) are
    appended in sorted order so ``llms-full.txt`` is complete; ``index.md`` is excluded.

    Returns:
        Flattened ``rel_path`` strings covering every ``docs/*.md`` except ``index.md``.
    """
    ordered = nav_pages(nav)
    seen = {*ordered, "index.md"}
    extra = sorted(
        rel
        for path in DOCS.rglob("*.md")
        if (rel := path.relative_to(DOCS).as_posix()) not in seen
    )
    return ordered + extra


def render(config: dict) -> str:
    """Render the ``llms.txt`` index from the parsed Zensical config.

    Returns:
        The llmstxt.org-format index document, newline-terminated.
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


def render_full(config: dict) -> str:
    """Render the self-contained ``llms-full.txt`` from the parsed Zensical config.

    Returns:
        The H1/summary header followed by every page's full Markdown (verbatim, so
        intra-doc links are preserved) in nav order, each under a ``Source:`` URL.
    """
    project = config["project"]
    site_url = project["site_url"].rstrip("/") + "/"
    parts = [f"# {project['site_name']}", "", f"> {project['site_description']}", ""]
    for rel_path in all_doc_pages(project["nav"]):
        raw = (DOCS / rel_path).read_text(encoding="utf-8").strip()
        body = absolutize_links(raw, rel_path, site_url)
        parts += ["---", "", f"Source: {page_url(rel_path, site_url)}", "", body, ""]
    return "\n".join(parts).rstrip() + "\n"


def main(
    *,
    check: Annotated[
        bool, typer.Option(help="Fail (exit 1) if a committed llms file is stale.")
    ] = False,
) -> None:
    """Generate the llms files, or verify they are current with ``--check``.

    Raises:
        typer.Exit: code 1 when ``--check`` finds a committed llms file stale.
    """
    config = tomllib.loads(ZENSICAL.read_text(encoding="utf-8"))
    targets = [(OUTPUT, render(config)), (OUTPUT_FULL, render_full(config))]
    for path, content in targets:
        if not check:
            # Always write LF regardless of platform so the output is deterministic.
            path.write_text(content, encoding="utf-8", newline="\n")
            continue
        # Read raw (newline="") so a CRLF-divergent committed file is detected, not
        # silently normalized to match the always-LF rendered content.
        current = path.read_text(encoding="utf-8", newline="") if path.is_file() else ""
        if current != content:
            typer.echo(
                f"{path.name} is stale; run `uv run scripts/gen_llms_txt.py`", err=True
            )
            raise typer.Exit(code=1)


if __name__ == "__main__":
    typer.run(main)
