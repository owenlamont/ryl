#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.14"
# dependencies = []
# ///

"""Report uncovered LLVM coverage regions per source file.

A single ``uv run`` entry point (stdlib only, no ``jq``): runs the coverage
suite, then prints each file's uncovered region line numbers coalesced into
ranges, e.g.::

    src/lint.rs:42,88-90

Pass ``--report-json PATH`` to analyze an existing ``cargo llvm-cov report
--json`` file without re-running the suite.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def find_project_root(start: Path) -> Path:
    """Return the nearest ancestor of ``start`` that contains ``Cargo.toml``.

    Returns:
        The project root directory.
    """
    for parent in (start, *start.parents):
        if (parent / "Cargo.toml").is_file():
            return parent
    sys.exit("could not locate Cargo.toml (project root) above this script")


def coalesce(lines: list[int]) -> str:
    """Collapse line numbers into a ``42,88-90`` range string.

    Returns:
        The sorted-unique lines rendered as comma-separated singletons and
        ``a-b`` ranges.
    """
    ranges: list[list[int]] = []
    for line in sorted(set(lines)):
        if ranges and line == ranges[-1][1] + 1:
            ranges[-1][1] = line
        else:
            ranges.append([line, line])
    return ",".join(str(a) if a == b else f"{a}-{b}" for a, b in ranges)


def build_report(data: dict, root: Path) -> list[str]:
    """Build ``file:ranges`` lines for every file with an uncovered region.

    An LLVM segment is ``[line, col, count, has_count, is_region_entry,
    is_gap_region]``; a line is uncovered when it carries a real (non-gap)
    region that executed zero times (``count == 0 and has_count and not
    is_gap_region``).

    Returns:
        One ``relative/path:ranges`` string per file with uncovered regions.
    """
    normalized_root = root.as_posix().rstrip("/") + "/"
    report: list[str] = []
    for entry in data.get("data", []):
        for file in entry.get("files", []):
            if file["summary"]["regions"]["percent"] >= 100:
                continue
            uncovered = [
                seg[0]
                for seg in file["segments"]
                if seg[2] == 0 and seg[3] and not seg[5]
            ]
            if not uncovered:
                continue
            name = file["filename"].replace("\\", "/").removeprefix(normalized_root)
            report.append(f"{name}:{coalesce(uncovered)}")
    return report


def generate_report_json(root: Path) -> dict:
    """Run the coverage suite and return the parsed ``llvm-cov report`` JSON.

    Returns:
        The decoded ``cargo llvm-cov report --json`` payload.
    """
    if shutil.which("cargo") is None:
        sys.exit("cargo is required to run this script")
    summary = subprocess.run(
        ["cargo", "llvm-cov", "nextest", "--summary-only"],
        cwd=root,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    if summary.returncode != 0:
        sys.exit(
            "cargo llvm-cov nextest --summary-only failed; "
            "rerun it directly to see the error."
        )
    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as handle:
        tmp_path = Path(handle.name)
    try:
        report = subprocess.run(
            ["cargo", "llvm-cov", "report", "--json", "--output-path", str(tmp_path)],
            cwd=root,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        )
        if report.returncode != 0:
            sys.exit("failed to generate coverage report")
        return json.loads(tmp_path.read_text(encoding="utf-8"))
    finally:
        tmp_path.unlink(missing_ok=True)


def main() -> None:
    """Run the coverage suite (or read a saved report) and print uncovered regions."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--report-json",
        type=Path,
        help="analyze an existing `cargo llvm-cov report --json` file "
        "instead of running the suite",
    )
    args = parser.parse_args()

    root = find_project_root(Path(__file__).resolve())
    data = (
        json.loads(args.report_json.read_text(encoding="utf-8"))
        if args.report_json is not None
        else generate_report_json(root)
    )

    report = build_report(data, root)
    if report:
        print("Uncovered regions (file:path line ranges):")
        print("\n".join(report))
    else:
        print("Coverage OK: no uncovered regions.")


if __name__ == "__main__":
    main()
