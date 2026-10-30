"""Tests for coverage-missing.py's two gates: strict outside the parser, union inside it."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import subprocess
import sys
from typing import Final


SCRIPT: Final = Path(__file__).with_name("coverage-missing.py")
ROOT: Final = SCRIPT.parents[3]
PARSER_SOURCES: Final = [
    "char_traits.rs",
    "error.rs",
    "input.rs",
    "input/buffered.rs",
    "input/str.rs",
    "observer.rs",
    "options.rs",
    "parser.rs",
    "parser_stack.rs",
    "scanner.rs",
]
SCANNER: Final = "crates/ryl-yaml-parser/src/scanner.rs"


def coverage_file(name: str, percent: float, segments: list[list]) -> dict:
    """Build one file entry of a ``cargo llvm-cov report --json`` payload.

    Returns:
        The file entry.
    """
    return {
        "filename": str(ROOT / name),
        "summary": {"regions": {"percent": percent}},
        "segments": segments,
    }


def instantiation(name: str, count: int) -> dict:
    """Build one instantiation of a function in ``name`` whose one region ran ``count`` times.

    Returns:
        The function entry.
    """
    return {"filenames": [str(ROOT / name)], "regions": [[7, 1, 7, 9, count, 0, 0, 0]]}


def parser_report(*scanner_counts: int) -> tuple[list[dict], list[dict]]:
    """Every parser source, fully run except one instantiation of scanner.rs per count.

    Returns:
        The file and function entries.
    """
    names = [f"crates/ryl-yaml-parser/src/{name}" for name in PARSER_SOURCES]
    functions = [instantiation(name, 1) for name in names if name != SCANNER]
    return (
        [coverage_file(name, 0, []) for name in names],
        functions + [instantiation(SCANNER, count) for count in scanner_counts],
    )


def run(
    report: tuple[list[dict], list[dict]], tmp_path: Path, *flags: str
) -> subprocess.CompletedProcess[str]:
    """Run the helper on a report holding these file and function entries.

    Returns:
        The finished process.
    """
    files, functions = report
    path = tmp_path / "report.json"
    path.write_text(
        json.dumps({"data": [{"files": files, "functions": functions}]}),
        encoding="utf-8",
    )
    return subprocess.run(
        [sys.executable, str(SCRIPT), "--report-json", str(path), *flags],
        capture_output=True,
        text=True,
        check=False,
    )


def test_parser_sources_match_the_crate() -> None:
    spec = importlib.util.spec_from_file_location("coverage_missing", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    assert set(module.missing_parser_files({"data": []}, ROOT)) == {
        f"crates/ryl-yaml-parser/src/{name}" for name in PARSER_SOURCES
    }


def test_empty_report_fails(tmp_path: Path) -> None:
    result = run(([], []), tmp_path)
    assert result.returncode == 1
    assert "lacks parser sources" in result.stderr


def test_parser_without_instantiations_fails(tmp_path: Path) -> None:
    result = run(parser_report(), tmp_path)
    assert result.returncode == 1
    assert (
        "lacks parser sources: crates/ryl-yaml-parser/src/scanner.rs" in result.stderr
    )


def test_report_without_the_parser_fails(tmp_path: Path) -> None:
    result = run(([coverage_file("src/lib.rs", 100, [])], []), tmp_path)
    assert result.returncode == 1
    assert "lacks parser sources" in result.stderr


def test_parser_regions_count_once_any_instantiation_runs_them(tmp_path: Path) -> None:
    result = run(parser_report(0, 3), tmp_path)
    assert result.returncode == 0, result.stdout


def test_unexecuted_parser_region_fails(tmp_path: Path) -> None:
    result = run(parser_report(0), tmp_path)
    assert result.returncode == 1
    assert "crates/ryl-yaml-parser/src/scanner.rs:7" in result.stdout


def test_markdown_reports_the_union_and_the_gate_gaps(tmp_path: Path) -> None:
    result = run(parser_report(0), tmp_path, "--markdown")
    assert result.returncode == 0, result.stderr
    assert "| crates/ryl-yaml-parser/src/scanner.rs | 1 | 1 | 0.00% |" in result.stdout
    assert "- crates/ryl-yaml-parser/src/scanner.rs:7" in result.stdout


def test_ryl_region_missed_by_one_instantiation_fails(tmp_path: Path) -> None:
    ryl = coverage_file("src/lint.rs", 95, [[1, 1, 3, True, True, False]])
    files, functions = parser_report(1)
    result = run(([*files, ryl], functions), tmp_path)
    assert result.returncode == 1
    assert "src/lint.rs: a generic instantiation misses a region" in result.stdout
