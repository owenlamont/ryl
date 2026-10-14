# /// script
# requires-python = ">=3.10"
# dependencies = ["markdown-it-py>=4", "typer>=0.12"]
# ///

from collections import defaultdict
from collections.abc import Iterable
from pathlib import Path
import re
import subprocess

from markdown_it import MarkdownIt
import typer


_PR = re.compile(
    r"^(?:(?P<kind>[a-z]+)(?:\([^)]*\))?: )?"
    r"(?P<title>.+) \(#(?P<number>\d+)\)$"
)
app = typer.Typer(
    add_completion=False, context_settings={"help_option_names": ["-h", "--help"]}
)


def release_notes(changelog: str, version: str) -> str:
    """Return the exact version section's body.

    Raises:
        ValueError: The version section is missing, duplicated, or empty.
    """
    tokens = MarkdownIt().parse(changelog)
    sections = [
        (tokens[index + 1].content, token.map)
        for index, token in enumerate(tokens[:-1])
        if token.type == "heading_open" and token.tag == "h2" and token.map is not None
    ]
    matches = [
        (index, bounds)
        for index, (heading, bounds) in enumerate(sections)
        if heading == version or heading.startswith(f"{version} - ")
    ]
    if len(matches) != 1:
        raise ValueError(
            f"expected one changelog section for {version}; found {len(matches)}"
        )
    index, bounds = matches[0]
    lines = changelog.splitlines(keepends=True)
    end = sections[index + 1][1][0] if index + 1 < len(sections) else len(lines)
    notes = "".join(lines[bounds[1] : end]).strip()
    note_tokens = MarkdownIt().parse(notes)
    has_content = any(
        (token.type == "fence" and token.content.strip())
        or (
            token.type == "paragraph_open"
            and note_tokens[index + 1].type == "inline"
            and any(
                child.type == "image"
                or (child.type in {"text", "code_inline"} and child.content.strip())
                for child in note_tokens[index + 1].children or []
            )
        )
        for index, token in enumerate(note_tokens)
    )
    if not has_content:
        raise ValueError(f"empty changelog section for {version}")
    return f"{notes}\n"


def draft_entries(subjects: Iterable[str]) -> dict[str, list[str]]:
    """Group merged PR subjects by their conventional-commit prefix.

    Returns:
        Prefixes mapped to linked PR titles.
    """
    groups: dict[str, list[str]] = defaultdict(list)
    for subject in subjects:
        match = _PR.fullmatch(subject)
        if match is None:
            continue
        kind = match["kind"] or "other"
        title = match["title"]
        number = match["number"]
        groups[kind].append(
            f"{title} ([#{number}](https://github.com/owenlamont/ryl/pull/{number}))"
        )
    return dict(groups)


def _draft(version: str) -> str:
    tags = subprocess.check_output(
        ["git", "tag", "--list", "v*", "--sort=version:refname"], text=True
    ).splitlines()
    if version in tags:
        index = tags.index(version)
        base = tags[index - 1] if index else None
        target = version
    else:
        base = tags[-1] if tags else None
        target = "HEAD"
    revision = f"{base}..{target}" if base else target
    subjects = subprocess.check_output(
        ["git", "log", "--format=%s", revision], text=True
    ).splitlines()
    groups = draft_entries(subjects)
    lines = [f"## {version}", ""]
    for kind, entries in groups.items():
        lines.extend((f"### {kind}", "", *(f"- {entry}" for entry in entries), ""))
    return "\n".join(lines).rstrip() + "\n"


@app.command()
def draft(version: str) -> None:
    """Draft a version from merged PR subjects for an editor to curate."""
    print(_draft(version), end="")


@app.command()
def extract(version: str) -> None:
    """Print the release body for a changelog version.

    Raises:
        typer.BadParameter: The version section cannot supply release notes.
    """
    try:
        print(release_notes(Path("CHANGELOG.md").read_text(), version), end="")
    except ValueError as error:
        raise typer.BadParameter(str(error)) from error


if __name__ == "__main__":
    app()
