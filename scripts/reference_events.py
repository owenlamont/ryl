"""Compare YAML 1.2 reference parser events after normalizing line breaks."""

import os
from pathlib import Path
import re
import shutil
import subprocess  # ruff: ignore[suspicious-subprocess-import]
import tempfile


_PARSER = Path(
    os.environ.get(
        "RYL_REFERENCE_PARSER",
        Path.home()
        / "Code/ryl_repos/yaml-reference-parser/parser-1.2/javascript/bin/yaml-parser",
    )
)


_SCALAR_STYLE = re.compile(r"^(=VAL (?:&\S+ )?(?:<[^>]*> )?)[:'\"|>]")
_FLOW_STYLE = re.compile(r"^([+](?:MAP|SEQ)) (?:\{\}|\[\])")


def _without_style(event: str) -> str:
    return _FLOW_STYLE.sub(r"\1", _SCALAR_STYLE.sub(r"\1:", event))


def _events(source: str) -> list[str] | None:
    node = shutil.which("node")
    if not _PARSER.is_file() or node is None:
        return None
    source = source.replace("\r\n", "\n").replace("\r", "\n")
    # Node truncates piped stdout at 64 KiB when the CLI exits; a file gets all of it.
    with tempfile.TemporaryFile("w+", encoding="utf-8") as out:
        try:
            proc = subprocess.run(  # ruff: ignore[subprocess-without-shell-equals-true]
                [node, str(_PARSER), "--events"],
                input=source.encode("utf-8"),
                stdout=out,
                stderr=subprocess.DEVNULL,
                timeout=10,
                check=False,
            )
        except (OSError, subprocess.TimeoutExpired):
            return None
        if proc.returncode:
            return None
        out.seek(0)
        lines = out.read().split("\n")
    return [
        line.split(" ", 1)[0] if line.startswith(("+DOC", "-DOC")) else line
        for line in lines
        if line
    ]


def reference_equal(
    before: str, after: str, *, ignore_style: bool = False
) -> bool | None:
    """Return None if the reference CLI cannot decide either side.

    ``ignore_style`` compares scalar text and structure only, so a quote or flow
    style change counts as equal; callers must check resolved types separately.
    """
    left = _events(before)
    right = _events(after)
    if left is None or right is None:
        return None
    if ignore_style:
        left, right = map(_without_style, left), map(_without_style, right)
    return list(left) == list(right)
