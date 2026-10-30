#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.15"
# dependencies = []
# ///

"""Report uncovered LLVM coverage regions per source file.

A single ``uv run`` entry point (stdlib only, no ``jq``): runs the coverage
suite, then prints each file's uncovered region line numbers coalesced into
ranges, e.g.::

    src/lint.rs:42,88-90

Pass ``--report-json PATH`` to analyze an existing ``cargo llvm-cov report
--json`` file without re-running the suite, and ``--markdown`` to render the
parser's union coverage and the same gaps for the CI comment.
"""

from __future__ import annotations

import argparse
from collections import defaultdict
import json
import os
from pathlib import Path
import re
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


# The parser is generic over `Input`, so CI gates it on regions covered by any instantiation;
# every other file must cover each region within a single instantiation.
UNION_GATED_PREFIX = "crates/ryl-yaml-parser/"
CODE_REGION = 0


FUNCTION_DEFINITION = re.compile(
    r"^\s*(pub(\([a-z]+\))?\s+)?(const\s+)?fn ", re.MULTILINE
)


def missing_parser_files(data: dict, root: Path) -> list[str]:
    """List parser sources defining functions that the report does not cover at all.

    The strict gate in CI ignores the parser, so a report that omits it must not pass.

    Returns:
        The repo-relative paths absent from the report.
    """
    reported = parser_union(data, root)
    expected = (
        path.relative_to(root).as_posix()
        for path in sorted((root / UNION_GATED_PREFIX / "src").rglob("*.rs"))
        if FUNCTION_DEFINITION.search(path.read_text(encoding="utf-8"))
    )
    return [path for path in expected if path not in reported]


def relative(filename: str, root: Path) -> str:
    """Return ``filename`` relative to ``root``, with forward slashes.

    Returns:
        The repo-relative path.
    """
    return filename.replace("\\", "/").removeprefix(root.as_posix().rstrip("/") + "/")


def parser_union(data: dict, root: Path) -> dict[str, dict[tuple, bool]]:
    """Map each parser file's code regions to whether any instantiation ran them.

    An LLVM region is ``[line_start, col_start, line_end, col_end, count, file_id,
    expanded_file_id, kind]``, with ``file_id`` indexing its function's ``filenames``.

    Returns:
        The repo-relative paths, each mapped from region coordinates to coverage.
    """
    union: dict[str, dict[tuple, bool]] = defaultdict(dict)
    for entry in data.get("data", []):
        # `functions` also spans the test sources that `files` leaves out.
        reported = {relative(file["filename"], root) for file in entry.get("files", [])}
        for function in entry.get("functions", []):
            for region in function["regions"]:
                name = relative(function["filenames"][region[5]], root)
                if (
                    name in reported
                    and name.startswith(UNION_GATED_PREFIX)
                    and region[7] == CODE_REGION
                ):
                    key = tuple(region[:4])
                    union[name][key] = union[name].get(key, False) or region[4] > 0
    return union


def build_report(data: dict, root: Path) -> list[str]:
    """Build ``file:ranges`` lines for every file with an uncovered region.

    An LLVM segment is ``[line, col, count, has_count, is_region_entry,
    is_gap_region]``; a line is uncovered when it carries a real (non-gap)
    region that executed zero times (``count == 0 and has_count and not
    is_gap_region``).

    Outside the parser crate, a file whose summary still misses a region with every segment
    executed is reported too: CI's strict gate scores each generic instantiation separately.
    Parser files are reported from ``parser_union`` instead.

    Returns:
        One ``relative/path:ranges`` string per file with uncovered regions.
    """
    report = [
        f"{name}:{coalesce([key[0] for key in missed])}"
        for name, regions in sorted(parser_union(data, root).items())
        if (missed := [key for key, covered in regions.items() if not covered])
    ]
    for entry in data.get("data", []):
        for file in entry.get("files", []):
            name = relative(file["filename"], root)
            if (
                name.startswith(UNION_GATED_PREFIX)
                or file["summary"]["regions"]["percent"] >= 100
            ):
                continue
            uncovered = [
                seg[0]
                for seg in file["segments"]
                if seg[2] == 0 and seg[3] and not seg[5]
            ]
            if uncovered:
                report.append(f"{name}:{coalesce(uncovered)}")
            else:
                report.append(f"{name}: a generic instantiation misses a region")
    return report


def render_markdown(data: dict, root: Path, gaps: list[str]) -> str:
    """Render the parser's union region coverage and the gate's gaps as Markdown.

    Returns:
        The parser and gaps sections of the CI coverage comment.
    """
    rows = [
        (name, len(regions), sum(not covered for covered in regions.values()))
        for name, regions in sorted(parser_union(data, root).items())
    ]
    rows.append(("TOTAL", sum(row[1] for row in rows), sum(row[2] for row in rows)))
    table = "\n".join(
        f"| {name} | {total} | {missed} | {100 * (total - missed) / max(total, 1):.2f}% |"
        for name, total, missed in rows
    )
    return (
        "#### ryl-yaml-parser (a region counts once any `Input` instantiation runs it)"
        "\n\n| File | Regions | Missed Regions | Cover |\n| --- | ---: | ---: | ---: |\n"
        f"{table}\n\n#### Gaps failing the gate\n\n"
        + ("\n".join(f"- {gap}" for gap in gaps) or "None.")
    )


def generate_report_json(root: Path) -> dict:
    """Run the coverage suite and return the parsed ``llvm-cov report`` JSON.

    Returns:
        The decoded ``cargo llvm-cov report --json`` payload.
    """
    if shutil.which("cargo") is None:
        sys.exit("cargo is required to run this script")
    summary = subprocess.run(
        ["cargo", "llvm-cov", "nextest", "--workspace", "--summary-only"],
        cwd=root,
        env=os.environ
        | {"PROPTEST_CASES": "0", "PROPTEST_DISABLE_FAILURE_PERSISTENCE": "1"},
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        encoding="utf-8",
        check=False,
    )
    if summary.returncode != 0:
        # `--summary-only` suppresses the coverage table, not nextest's FAIL/panic output, so
        # the captured stream still diagnoses the failure here without a manual re-run. The
        # highlights name the failing tests; the tail carries the panic/assertion context the
        # highlight lines omit.
        lines = (summary.stdout or "").splitlines()
        highlights = [
            line
            for line in lines
            if "FAIL" in line
            or "panicked" in line
            or "assertion" in line
            or line.startswith("error")
        ]
        tail = lines[-60:]
        parts = (
            [*highlights[:40], "", "--- output tail ---", *tail] if highlights else tail
        )
        sys.exit("cargo llvm-cov nextest failed:\n" + "\n".join(parts))
    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as handle:
        tmp_path = Path(handle.name)
    try:
        report = subprocess.run(
            [
                "cargo",
                "llvm-cov",
                "report",
                "--workspace",
                "--json",
                "--output-path",
                str(tmp_path),
            ],
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
    parser.add_argument(
        "--markdown",
        action="store_true",
        help="print the parser's union coverage and the gaps as Markdown, exiting 0",
    )
    args = parser.parse_args()

    root = find_project_root(Path(__file__).resolve())
    data = (
        json.loads(args.report_json.read_text(encoding="utf-8"))
        if args.report_json is not None
        else generate_report_json(root)
    )

    report = build_report(data, root)
    if missing := missing_parser_files(data, root):
        report.insert(0, "coverage report lacks parser sources: " + ", ".join(missing))
        if not args.markdown:
            sys.exit(report[0])
    if args.markdown:
        print(render_markdown(data, root, report))
    elif report:
        print("Uncovered regions (file:path line ranges):")
        print("\n".join(report))
        sys.exit(1)
    else:
        print("Coverage OK: no uncovered regions.")


if __name__ == "__main__":
    # Windows stdio defaults to cp1252; cargo output (e.g. unicode test names)
    # printed on failure would crash the encode, so force UTF-8.
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")
    main()
