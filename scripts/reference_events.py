"""Compare YAML 1.2 reference parser events after normalizing line breaks."""

import os
from pathlib import Path
import shutil
import subprocess  # ruff: ignore[suspicious-subprocess-import]


_PARSER = Path(
    os.environ.get(
        "RYL_REFERENCE_PARSER",
        Path.home()
        / "Code/ryl_repos/yaml-reference-parser/parser-1.2/javascript/bin/yaml-parser",
    )
)


def _events(source: str) -> list[str] | None:
    node = shutil.which("node")
    if not _PARSER.is_file() or node is None:
        return None
    source = source.replace("\r\n", "\n").replace("\r", "\n")
    try:
        proc = subprocess.run(  # ruff: ignore[subprocess-without-shell-equals-true]
            [node, str(_PARSER), "--events"],
            input=source,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    if proc.returncode:
        return None
    return [
        line.split(" ", 1)[0] if line.startswith(("+DOC", "-DOC")) else line
        for line in proc.stdout.splitlines()
    ]


def reference_equal(before: str, after: str) -> bool | None:
    """Return None if the reference CLI cannot decide either side."""
    left = _events(before)
    right = _events(after)
    return left == right if left is not None and right is not None else None
