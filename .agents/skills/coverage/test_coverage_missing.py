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


def run(files: list[dict], tmp_path: Path) -> subprocess.CompletedProcess[str]:
    """Run the helper on a report holding ``files``.

    Returns:
        The finished process.
    """
    report = tmp_path / "report.json"
    report.write_text(json.dumps({"data": [{"files": files}]}), encoding="utf-8")
    return subprocess.run(
        [sys.executable, str(SCRIPT), "--report-json", str(report)],
        capture_output=True,
        text=True,
        check=False,
    )


def parser_files(percent: float, segments: list[list]) -> list[dict]:
    """Every parser source with the same coverage.

    Returns:
        The file entries.
    """
    return [
        coverage_file(f"crates/ryl-yaml-parser/src/{name}", percent, segments)
        for name in PARSER_SOURCES
    ]


def test_parser_sources_match_the_crate() -> None:
    spec = importlib.util.spec_from_file_location("coverage_missing", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    assert set(module.missing_parser_files({"data": []}, ROOT)) == {
        f"crates/ryl-yaml-parser/src/{name}" for name in PARSER_SOURCES
    }


def test_empty_report_fails(tmp_path: Path) -> None:
    result = run([], tmp_path)
    assert result.returncode == 1
    assert "lacks parser sources" in result.stderr


def test_report_without_the_parser_fails(tmp_path: Path) -> None:
    result = run([coverage_file("src/lib.rs", 100, [])], tmp_path)
    assert result.returncode == 1
    assert "lacks parser sources" in result.stderr


def test_parser_regions_count_once_any_instantiation_runs_them(tmp_path: Path) -> None:
    result = run(parser_files(95, [[1, 1, 3, True, True, False]]), tmp_path)
    assert result.returncode == 0, result.stdout


def test_unexecuted_parser_region_fails(tmp_path: Path) -> None:
    result = run(parser_files(95, [[7, 1, 0, True, True, False]]), tmp_path)
    assert result.returncode == 1
    assert "crates/ryl-yaml-parser/src/scanner.rs:7" in result.stdout


def test_ryl_region_missed_by_one_instantiation_fails(tmp_path: Path) -> None:
    ryl = coverage_file("src/lint.rs", 95, [[1, 1, 3, True, True, False]])
    result = run([*parser_files(100, []), ryl], tmp_path)
    assert result.returncode == 1
    assert "src/lint.rs: a generic instantiation misses a region" in result.stdout
