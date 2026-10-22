# /// script
# requires-python = ">=3.15"
# dependencies = [
#   "pytest", "matplotlib", "typer", "py-yaml12", "pyyaml", "platformdirs"
# ]
# ///
"""Pure helpers of scripts/benchmark_yaml_tools.py, run with:

uv run --with pytest --with matplotlib --with typer --with py-yaml12 --with pyyaml
--with platformdirs pytest tests/test_benchmark_yaml_tools.py
"""

from pathlib import Path
import subprocess
import sys

import pytest


sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "scripts"))

import benchmark_yaml_tools as bench


def test_identical_line_share_counts_common_lines_on_both_sides(tmp_path: Path):
    left, right = tmp_path / "left", tmp_path / "right"
    left.mkdir()
    right.mkdir()
    (left / "a.yaml").write_text("a: 1\nb: 2\nc: 3\n")
    (right / "a.yaml").write_text("a: 1\nb: '2'\nc: 3\nd: 4\n")
    (left / "same.yaml").write_text("x: 1\n")
    (right / "same.yaml").write_text("x: 1\n")
    numstat = subprocess.run(
        ["git", "diff", "--no-index", "--numstat", str(left), str(right)],
        capture_output=True,
        text=True,
        check=False,
    ).stdout

    assert bench.identical_line_share(numstat, left, right) == (2 * 2 + 2, 3 + 4 + 2)


def test_native_executable_refuses_a_script_launcher(tmp_path: Path):
    launcher = tmp_path / "yamlfmt"
    launcher.write_text('#!/bin/sh\nexec real-yamlfmt "$@"\n')

    with pytest.raises(RuntimeError, match="script launcher"):
        bench.native_executable(launcher)


def test_median_rows_keeps_timing_and_memory_runs_apart():
    base = {"sweep": "size", "x": "1024", "mode": "format", "profile": "closest"}
    rows = [
        base | {"tool": "ryl", "wall_seconds": "0.1", "max_rss_kib": ""},
        base | {"tool": "ryl", "wall_seconds": "0.3", "max_rss_kib": ""},
        base | {"tool": "ryl", "wall_seconds": "", "max_rss_kib": "2048"},
    ]

    assert bench.median_rows(rows) == [
        base | {"x": 1024, "tool": "ryl", "wall_seconds": 0.2, "max_rss_mib": 2.0}
    ]


def test_prettier_receives_a_glob_and_other_tools_the_directory(tmp_path: Path):
    prettier = bench.Tool(name="Prettier", mode="format", profile=None, argv=("p",))
    yamlfmt = bench.Tool(name="yamlfmt", mode="format", profile=None, argv=("y",))

    assert prettier.command(tmp_path) == ["p", str(tmp_path / "*.yaml")]
    assert yamlfmt.command(tmp_path) == ["y", str(tmp_path)]
