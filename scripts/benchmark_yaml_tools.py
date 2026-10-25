#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.15"
# dependencies = [
#   "matplotlib>=3.9,<4",
#   "typer>=0.16,<1",
#   "py-yaml12",
#   "pyyaml",
#   "platformdirs",
# ]
# ///

"""Benchmark ryl against yamllint, yamlfmt, dprint + pretty_yaml and Prettier.

Every run gets a fresh copy of its inputs and an empty cache directory, tools run in
alternating order, and wall time, CPU time and peak RSS come from `wait4`, so the
script needs Linux or macOS. Inputs are pinned real-world files every tool accepts
under every profile; see the README's Performance section for what each profile
compares and what it cannot equalise.
"""

from collections import Counter
from collections.abc import Iterable, Mapping, Sequence
import concurrent.futures
import csv
from dataclasses import dataclass
from datetime import datetime, UTC
from enum import StrEnum
import fnmatch
from hashlib import sha256
import itertools
import json
import math
import os
from pathlib import Path
import platform
import shutil
import statistics
import subprocess
import tarfile
import tempfile
import time
import tomllib
from typing import Final
import urllib.request

import formatter_corpus_check as corpus_gate
from matplotlib import ticker
import matplotlib.pyplot as plt
import typer


class Profile(StrEnum):
    DEFAULTS = "defaults"
    ALIGNED = "aligned"
    MAX_WORK = "max-work"


class Sweep(StrEnum):
    SIZE = "size"
    COUNT = "count"


FORMATTERS: Final = ("ryl", "yamlfmt", "dprint", "Prettier")
LINTERS: Final = ("ryl", "yamllint")
SIZES_KIB: Final = (1, 4, 16, 64, 256, 1024, 2048, 10240)
COUNTS: Final = (1, 10, 100, 1000)
GNU_TIME: Final = "/usr/bin/time"
RSS_RUNS: Final = 3
TYPICAL_MAX_BYTES: Final = 16 * 1024
LINT_CONFIG: Final = "extends: default\n"

_RYL_ALIGNED: Final = """[format]
quote-style = "double"
max-blank-lines = 1
comment-spacing = 1
comment-starting-space = "preserve"
brace-spacing = true
"""
_YAMLFMT_ALIGNED: Final = """formatter:
  type: basic
  retain_line_breaks_single: true
  force_quote_style: double
  pad_line_comments: 1
  trim_trailing_whitespace: true
"""
PROFILE_CONFIGS: Final[Mapping[Profile, Mapping[str, str | dict[str, str | bool]]]] = {
    Profile.DEFAULTS: {
        "ryl": "[format]\n",
        "yamlfmt": "formatter:\n  type: basic\n",
        "dprint": {},
        "Prettier": {},
    },
    Profile.ALIGNED: {
        "ryl": _RYL_ALIGNED,
        "yamlfmt": _YAMLFMT_ALIGNED,
        "dprint": {},
        "Prettier": {},
    },
    Profile.MAX_WORK: {
        "ryl": _RYL_ALIGNED.replace('"double"', '"single"')
        + "fold-long-lines = true\n",
        "yamlfmt": _YAMLFMT_ALIGNED.replace("double", "single")
        + "  max_line_length: 80\n",
        "dprint": {"quotes": "forceSingle", "proseWrap": "always"},
        "Prettier": {"singleQuote": True, "proseWrap": "always"},
    },
}


@dataclass(frozen=True, slots=True, kw_only=True)
class Case:
    sweep: Sweep
    x: int
    files: tuple[Path, ...]


@dataclass(frozen=True, slots=True, kw_only=True)
class Tool:
    name: str
    mode: str
    profile: Profile | None
    argv: tuple[str, ...]
    """Command with the input directory appended (Prettier gets a glob)."""

    def command(self, inputs: Path) -> list[str]:
        target = str(inputs / "*.yaml") if self.name == "Prettier" else str(inputs)
        return [*self.argv, target]


@dataclass(frozen=True, slots=True)
class Sample:
    wall_seconds: float
    cpu_seconds: float
    max_rss_kib: int
    exit_code: int


app = typer.Typer(add_completion=False)


def run_checked(args: Sequence[str], **kwargs: Path) -> str:
    return subprocess.run(
        args, check=True, text=True, capture_output=True, cwd=kwargs.get("cwd")
    ).stdout


def fetch_json(url: str) -> dict:
    with urllib.request.urlopen(url, timeout=60) as response:  # ruff: ignore[suspicious-url-open-usage]
        return json.load(response)


def latest_versions() -> dict[str, str]:
    """Return the newest released version of each comparison tool."""
    npm = {
        key: run_checked(["npm", "view", package, "version"]).strip()
        for key, package in (
            ("prettier", "prettier"),
            ("dprint", "dprint"),
            ("pretty_yaml", "dprint-plugin-yaml"),
        )
    }
    yamlfmt = fetch_json("https://api.github.com/repos/google/yamlfmt/releases/latest")
    pypi = fetch_json("https://pypi.org/pypi/yamllint/json")
    return {
        **npm,
        "yamlfmt": yamlfmt["tag_name"].removeprefix("v"),
        "yamllint": pypi["info"]["version"],
    }


def native_executable(path: Path) -> Path:
    """Resolve symlinks and refuse a script launcher, which adds startup per call.

    Raises:
        RuntimeError: `path` starts with a `#!` line.
    """
    real = path.resolve()
    if real.read_bytes()[:2] == b"#!":
        raise RuntimeError(f"{real} is a script launcher, not a native executable")
    return real


def install_tools(versions: Mapping[str, str], tool_dir: Path) -> dict[str, Path]:
    """Install every comparison tool under `tool_dir`, leaving global installs alone."""
    tool_dir.mkdir(parents=True)
    run_checked(
        [
            "npm",
            "install",
            "--prefix",
            str(tool_dir),
            "--no-save",
            "--no-package-lock",
            "--dangerously-allow-all-scripts",
            f"prettier@{versions['prettier']}",
            f"dprint@{versions['dprint']}",
        ]
    )
    archive = run_checked(
        [
            "npm",
            "pack",
            f"dprint-plugin-yaml@{versions['pretty_yaml']}",
            "--pack-destination",
            str(tool_dir),
        ]
    ).splitlines()[-1]
    with tarfile.open(tool_dir / archive) as package:
        package.extract("package/plugin.wasm", tool_dir, filter="data")
    venv = tool_dir / "yamllint-venv"
    run_checked(["uv", "venv", "-q", str(venv)])
    run_checked(
        [
            "uv",
            "pip",
            "install",
            "-q",
            "--compile-bytecode",
            "-p",
            str(venv),
            f"yamllint=={versions['yamllint']}",
        ]
    )
    machine = {"x86_64": "x86_64", "amd64": "x86_64", "arm64": "arm64"}
    asset = (
        f"yamlfmt_{versions['yamlfmt']}_{platform.system()}_"
        f"{machine[platform.machine().lower()]}.tar.gz"
    )
    url = (
        "https://github.com/google/yamlfmt/releases/download/"
        f"v{versions['yamlfmt']}/{asset}"
    )
    with urllib.request.urlopen(url, timeout=120) as response:  # ruff: ignore[suspicious-url-open-usage]
        (tool_dir / asset).write_bytes(response.read())
    with tarfile.open(tool_dir / asset) as package:
        package.extract("yamlfmt", tool_dir, filter="data")
    modules = tool_dir / "node_modules"
    return {
        "yamllint": venv / "bin" / "yamllint",
        "yamlfmt": native_executable(tool_dir / "yamlfmt"),
        "dprint": native_executable(modules / "dprint" / "dprint"),
        "prettier": modules / "prettier" / "bin" / "prettier.cjs",
        "plugin": tool_dir / "package" / "plugin.wasm",
    }


def make_tools(
    *, ryl: Path, installed: Mapping[str, Path], config_dir: Path
) -> list[Tool]:
    """Write each profile's configs and return every lint and format command."""
    yamllint_config = config_dir / "yamllint.yaml"
    yamllint_config.parent.mkdir(parents=True, exist_ok=True)
    yamllint_config.write_text(LINT_CONFIG)
    run_checked(
        [
            str(ryl),
            "--migrate-configs",
            "--migrate-write",
            "--migrate-root",
            str(yamllint_config),
        ]
    )
    node = shutil.which("node")
    if node is None:
        raise RuntimeError("node is not on PATH")
    tools = [
        Tool(
            name="ryl",
            mode="lint",
            profile=None,
            argv=(
                str(ryl),
                "check",
                "--no-cache",
                "-f",
                "parsable",
                "-c",
                str(yamllint_config.with_suffix(".toml")),
            ),
        ),
        Tool(
            name="yamllint",
            mode="lint",
            profile=None,
            argv=(
                str(installed["yamllint"]),
                "-f",
                "parsable",
                "-c",
                str(yamllint_config),
            ),
        ),
    ]
    for profile, configs in PROFILE_CONFIGS.items():
        directory = config_dir / profile
        directory.mkdir()
        (directory / "ryl.toml").write_text(str(configs["ryl"]))
        (directory / "yamlfmt.yaml").write_text(str(configs["yamlfmt"]))
        dprint = {"plugins": [str(installed["plugin"])], "yaml": configs["dprint"]}
        (directory / "dprint.json").write_text(json.dumps(dprint))
        (directory / "prettier.json").write_text(json.dumps(configs["Prettier"]))
        argvs = {
            "ryl": (
                str(ryl),
                "format",
                "--no-cache",
                "-c",
                str(directory / "ryl.toml"),
            ),
            "yamlfmt": (
                str(installed["yamlfmt"]),
                "-no_global_conf",
                "-conf",
                str(directory / "yamlfmt.yaml"),
            ),
            "dprint": (
                str(installed["dprint"]),
                "fmt",
                "--incremental=false",
                "--no-gitignore",
                "--log-level",
                "silent",
                "--config",
                str(directory / "dprint.json"),
            ),
            "Prettier": (
                node,
                str(installed["prettier"]),
                "--write",
                "--no-cache",
                "--no-editorconfig",
                "--config",
                str(directory / "prettier.json"),
            ),
        }
        tools.extend(
            Tool(name=name, mode="format", profile=profile, argv=argv)
            for name, argv in argvs.items()
        )
    return tools


def measure(
    command: Sequence[str], *, cwd: Path, cache: Path, rss: bool = False
) -> Sample:
    """Run `command` once with every cache variable pointed at the empty `cache`.

    Peak RSS needs GNU time: a child spawned straight from this process inherits
    this process's RSS high-water mark across exec. With `rss=False` the returned
    `max_rss_kib` is 0; with `rss=True` the wall time includes GNU time's own exec.
    """
    rss_file = cache / "max-rss-kib"
    if rss:
        command = [GNU_TIME, "-f", "%M", "-o", str(rss_file), *command]
    env = os.environ | {"XDG_CACHE_HOME": str(cache)}
    start = time.perf_counter()
    process = subprocess.Popen(
        command, cwd=cwd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
    )
    _, status, usage = os.wait4(process.pid, 0)
    wall = time.perf_counter() - start
    process.returncode = os.waitstatus_to_exitcode(status)
    return Sample(
        wall_seconds=wall,
        cpu_seconds=usage.ru_utime + usage.ru_stime,
        max_rss_kib=int(rss_file.read_text().split()[-1]) if rss else 0,
        exit_code=process.returncode,
    )


def stage(files: Iterable[Path], directory: Path) -> Path:
    shutil.rmtree(directory, ignore_errors=True)
    directory.mkdir(parents=True)
    for index, source in enumerate(files):
        shutil.copyfile(source, directory / f"file_{index:05d}.yaml")
    return directory


def accepts(path: Path, tools: Sequence[Tool], scratch: Path) -> bool:
    """Whether every tool succeeds on `path` (lint may report problems)."""
    for tool in tools:
        work = stage([path], scratch / "in")
        cache = stage([], scratch / "cache")
        allowed = {0, 1} if tool.mode == "lint" else {0}
        sample = measure(tool.command(work), cwd=scratch, cache=cache)
        if sample.exit_code not in allowed:
            return False
    return True


def corpus_files() -> list[Path]:
    clone_dir = corpus_gate._DEFAULT_CACHE / "repos"
    files = []
    for repo in corpus_gate._manifest(()):
        clone = corpus_gate._fetch(repo, clone_dir)
        marker = repo.expected_skip_marker
        for path in sorted(clone.rglob("*")):
            relative = path.relative_to(clone).as_posix()
            if (
                path.suffix.lower() in {".yaml", ".yml"}
                and ".git" not in path.parts
                and not path.is_symlink()
                and path.is_file()
                and not any(fnmatch.fnmatch(relative, p) for p in repo.expected_skip)
                and not (
                    path.name == marker.get("file")
                    and (path.parent / marker.get("marker", "")).exists()
                )
            ):
                files.append(path)
    return sorted(files, key=lambda path: sha256(str(path).encode()).digest())


def size_bounds(size_kib: int) -> range:
    """Byte sizes within 25% of `size_kib`, inclusive of both ends."""
    target = size_kib * 1024
    return range(target * 3 // 4, target * 5 // 4 + 1)


def first_accepted(
    candidates: Iterable[Path], wanted: int, tools: Sequence[Tool], scratch: Path
) -> list[Path]:
    """Return the first `wanted` candidates every tool accepts, probed in parallel."""
    accepted: list[Path] = []
    pending = iter(candidates)
    workers = max(1, (os.cpu_count() or 2) // 2)
    with concurrent.futures.ThreadPoolExecutor(workers) as pool:
        while len(accepted) < wanted:
            batch = list(itertools.islice(pending, workers * 2))
            if not batch:
                break
            verdicts = pool.map(
                lambda item: accepts(item[1], tools, scratch / str(item[0])),
                enumerate(batch),
            )
            accepted.extend(p for p, ok in zip(batch, verdicts, strict=True) if ok)
    return accepted[:wanted]


def build_cases(
    tools: Sequence[Tool],
    scratch: Path,
    *,
    sizes_kib: Sequence[int] = SIZES_KIB,
    counts: Sequence[int] = COUNTS,
) -> list[Case]:
    """Pick one file per size bucket and the shared prefix of a typical-file pool."""
    files = corpus_files()
    sizes = {path: path.stat().st_size for path in files}
    cases = []
    for size_kib in sizes_kib:
        bounds = size_bounds(size_kib)
        bucket = sorted(
            (p for p in files if sizes[p] in bounds),
            key=lambda p: abs(sizes[p] - size_kib * 1024),
        )
        chosen = first_accepted(bucket, 1, tools, scratch)
        if not chosen:
            raise RuntimeError(f"no accepted file within 25% of {size_kib} KiB")
        cases.append(Case(sweep=Sweep.SIZE, x=sizes[chosen[0]], files=tuple(chosen)))
    typical = [p for p in files if sizes[p] < TYPICAL_MAX_BYTES]
    pool = first_accepted(typical, max(counts), tools, scratch)
    cases.extend(
        Case(sweep=Sweep.COUNT, x=count, files=tuple(pool[:count])) for count in counts
    )
    return cases


def load_average() -> float:
    return os.getloadavg()[0]


def time_case(
    case: Case, tools: Sequence[Tool], *, runs: int, scratch: Path
) -> list[dict[str, str | int | float]]:
    """Time every tool `runs` times, reversing the tool order on odd runs."""
    rows: list[dict[str, str | int | float]] = []
    for run in range(runs + RSS_RUNS):
        ordered = tools if run % 2 == 0 else tools[::-1]
        rss = run >= runs
        for tool in ordered:
            work = stage(case.files, scratch / "work")
            cache = stage([], scratch / "cache")
            sample = measure(tool.command(work), cwd=scratch, cache=cache, rss=rss)
            if sample.exit_code not in ({0, 1} if tool.mode == "lint" else {0}):
                raise RuntimeError(f"{tool} exited {sample.exit_code} on {case}")
            rows.append(
                {
                    "sweep": case.sweep,
                    "x": case.x,
                    "mode": tool.mode,
                    "profile": tool.profile or "lint",
                    "tool": tool.name,
                    "run": run,
                    "order": "forward" if run % 2 == 0 else "reverse",
                    "wall_seconds": "" if rss else sample.wall_seconds,
                    "cpu_seconds": "" if rss else sample.cpu_seconds,
                    "max_rss_kib": sample.max_rss_kib if rss else "",
                }
            )
    return rows


def identical_line_share(numstat: str, left: Path, right: Path) -> tuple[int, int]:
    """Return (identical lines, total lines) for two output directories.

    Identical lines count once per side, so the share is 2·common / (|left|+|right|).
    """
    deleted = {}
    for line in numstat.splitlines():
        _, removed, name = line.split("\t", 2)
        deleted[Path(name).name] = int(removed)
    common = total = 0
    for file in sorted(left.iterdir()):
        left_lines = len(file.read_bytes().splitlines())
        right_lines = len((right / file.name).read_bytes().splitlines())
        common += 2 * (left_lines - deleted.get(file.name, 0))
        total += left_lines + right_lines
    return common, total


def output_agreement(
    files: Sequence[Path], tools: Sequence[Tool], scratch: Path
) -> dict[str, dict[str, float]]:
    """Format every input once per tool and profile; report pairwise identical lines."""
    agreement: dict[str, dict[str, float]] = {}
    for profile in Profile:
        outputs = {}
        for tool in (t for t in tools if t.profile == profile):
            work = stage(files, scratch / profile / tool.name)
            sample = measure(
                tool.command(work), cwd=scratch, cache=stage([], scratch / "c")
            )
            if sample.exit_code != 0:
                raise RuntimeError(f"{tool} failed while measuring agreement")
            if all(
                stage(files, scratch / "orig")
                and (work / f.name).read_bytes()
                == (scratch / "orig" / f.name).read_bytes()
                for f in work.iterdir()
            ):
                raise RuntimeError(f"{tool} changed no file; is it reading the inputs?")
            outputs[tool.name] = work
        agreement[profile] = {}
        for left, right in itertools.combinations(FORMATTERS, 2):
            numstat = subprocess.run(
                [
                    "git",
                    "diff",
                    "--no-index",
                    "--numstat",
                    "--no-renames",
                    str(outputs[left]),
                    str(outputs[right]),
                ],
                capture_output=True,
                text=True,
                check=False,
            ).stdout
            common, total = identical_line_share(numstat, outputs[left], outputs[right])
            agreement[profile][f"{left} vs {right}"] = round(100.0 * common / total, 2)
    return agreement


def lint_agreement(
    files: Sequence[Path], tools: Sequence[Tool], scratch: Path
) -> dict[str, dict[str, int]]:
    """Per rule, count diagnostics both linters report and those only one reports."""
    work = stage(files, scratch / "lint")
    reported = {}
    for tool in (t for t in tools if t.mode == "lint"):
        result = subprocess.run(
            tool.command(work), capture_output=True, text=True, check=False, cwd=scratch
        )
        lines = (result.stdout + result.stderr).splitlines()
        reported[tool.name] = Counter(line for line in lines if ": [" in line)
    ryl, yamllint = reported["ryl"], reported["yamllint"]
    by_rule: dict[str, Counter[str]] = {}
    for kind, diagnostics in (
        ("both", ryl & yamllint),
        ("ryl only", ryl - yamllint),
        ("yamllint only", yamllint - ryl),
    ):
        for line, count in diagnostics.items():
            rule = line.rsplit("(", 1)[1].rstrip(")")
            by_rule.setdefault(rule, Counter())[kind] += count
    return {rule: dict(counts) for rule, counts in sorted(by_rule.items())}


def record_versions(tools: Sequence[Tool], versions: Mapping[str, str]) -> dict:
    ryl = next(tool for tool in tools if tool.name == "ryl").argv[0]
    return {
        "pins": dict(versions),
        "ryl": run_checked([ryl, "--version"]).strip(),
        "binaries": {
            tool.name: {
                "path": tool.argv[0],
                "sha256": sha256(Path(tool.argv[0]).read_bytes()).hexdigest(),
            }
            for tool in tools
        },
    }


def median_rows(rows: Iterable[Mapping[str, str]]) -> list[dict[str, str | float]]:
    """Collapse runs to one row per case, tool and profile with medians."""
    groups: dict[tuple[str, ...], list[Mapping[str, str]]] = {}
    for row in rows:
        key = (row["sweep"], row["x"], row["mode"], row["profile"], row["tool"])
        groups.setdefault(key, []).append(row)
    return [
        {
            "sweep": key[0],
            "x": int(key[1]),
            "mode": key[2],
            "profile": key[3],
            "tool": key[4],
            "wall_seconds": statistics.median(
                float(r["wall_seconds"]) for r in group if r["wall_seconds"]
            ),
            "max_rss_mib": statistics.median(
                int(r["max_rss_kib"]) for r in group if r["max_rss_kib"]
            )
            / 1024,
        }
        for key, group in sorted(groups.items())
    ]


COLORS: Final = {
    "ryl": "#2a78d6",
    "yamlfmt": "#eb6834",
    "dprint": "#1baf7a",
    "Prettier": "#eda100",
    "yamllint": "#e87ba4",
}
PANEL_TOOLS: Final = {
    "lint": {"ryl": "o", "yamllint": "v"},
    Profile.MAX_WORK: {"ryl": "o", "yamlfmt": "s", "dprint": "^", "Prettier": "D"},
}
LABELS: Final = {"dprint": "dprint + pretty_yaml"}
PANEL_TITLES: Final = {
    "lint": "Lint, the same yamllint `extends: default` rules",
    Profile.MAX_WORK: "Format, max common work",
}
X_LABELS: Final = {
    Sweep.SIZE: "Single file size",
    Sweep.COUNT: "Number of files (real-world, each under 16 KiB)",
}
_INK: Final = "#3d3c38"
_MUTED: Final = "#8a8981"
_DECADES_FOR_SPARSE_TICKS: Final = 2.0


def _plain(value: float) -> str:
    return f"{value:,.10g}"


def _size_label(value: float) -> str:
    return f"{value / 1024**2:g} MiB" if value >= 1024**2 else f"{value / 1024:g} KiB"


def _log_axis(axis: plt.Axis, values: Sequence[float]) -> None:
    """Label 1-2-5 steps over a narrow range and powers of ten over a wide one."""
    span = math.log10(max(values) / min(values))
    subs = (1.0,) if span > _DECADES_FOR_SPARSE_TICKS else (1.0, 2.0, 5.0)
    axis.set_major_locator(ticker.LogLocator(subs=subs))
    axis.set_major_formatter(ticker.FuncFormatter(lambda v, _: _plain(v)))
    axis.set_minor_locator(ticker.NullLocator())


def _panel(
    axis: plt.Axes,
    summary: Sequence[Mapping[str, str | float]],
    *,
    profile: str,
    sweep: Sweep,
    metric: str,
) -> None:
    rows = [r for r in summary if r["profile"] == profile and r["sweep"] == sweep]
    values = [float(r[metric]) for r in rows]
    if metric == "max_rss_mib":
        scale, label = 1.0, "Peak memory (MiB)"
    elif max(values) < 1.0:
        scale, label = 1000.0, "Median wall time (ms)"
    else:
        scale, label = 1.0, "Median wall time (s)"
    for tool, marker in PANEL_TOOLS[profile].items():
        points = sorted(
            (int(r["x"]), float(r[metric])) for r in rows if r["tool"] == tool
        )
        axis.plot(
            [x for x, _ in points],
            [y * scale for _, y in points],
            color=COLORS[tool],
            marker=marker,
            markersize=6,
            linewidth=2,
            markeredgecolor="white",
            label=LABELS.get(tool, tool),
        )
    axis.set_xscale("log")
    axis.set_yscale("log")
    _log_axis(axis.yaxis, [v * scale for v in values])
    xs = [float(r["x"]) for r in rows]
    if sweep == Sweep.SIZE:
        axis.xaxis.set_major_locator(ticker.LogLocator(base=4))
        axis.xaxis.set_major_formatter(
            ticker.FuncFormatter(lambda v, _: _size_label(v))
        )
        axis.xaxis.set_minor_locator(ticker.NullLocator())
    else:
        _log_axis(axis.xaxis, xs)
    axis.grid(visible=True, which="major", color="#e4e3dd", linewidth=0.8)
    axis.tick_params(colors=_MUTED, labelcolor=_INK, labelsize=8)
    for spine in axis.spines.values():
        spine.set_color("#d0cfc8")
    axis.set_xlabel(X_LABELS[sweep], color=_INK, fontsize=9)
    axis.set_ylabel(label, color=_INK, fontsize=9)
    axis.set_title(PANEL_TITLES[profile], fontsize=9, color=_INK, loc="left")


def plot_chart(summary: Sequence[Mapping[str, str | float]], path: Path) -> None:
    """Lint and aligned-config format: wall time and peak memory over both sweeps."""
    plt.rcParams.update({"svg.fonttype": "none", "font.family": "sans-serif"})
    figure, axes = plt.subplots(2, 4, figsize=(17, 7.5))
    figure.patch.set_facecolor("white")
    panels = list(itertools.product(Sweep, ("wall_seconds", "max_rss_mib")))
    for row, profile in zip(axes, PANEL_TITLES, strict=True):
        for axis, (sweep, metric) in zip(row, panels, strict=True):
            _panel(axis, summary, profile=profile, sweep=sweep, metric=metric)
        row[0].legend(fontsize=8, frameon=False)
    figure.suptitle(
        "Wall time and peak memory", color=_INK, fontsize=12, x=0.01, ha="left"
    )
    figure.tight_layout()
    figure.savefig(path)
    plt.close(figure)


@app.command()
def run(
    ryl_bin: Path = typer.Option(..., help="ryl release binary to benchmark."),
    tool_versions: str = typer.Option(
        "pinned", help="'pinned' (benchmark_tool_versions.toml) or 'latest'."
    ),
    runs: int = typer.Option(10, min=2, help="Runs per tool and case (even)."),
    output_dir: Path = typer.Option(Path("manual_outputs") / "benchmarks"),
    sizes_kib: str = typer.Option(",".join(map(str, SIZES_KIB))),
    counts: str = typer.Option(",".join(map(str, COUNTS))),
    max_load: float = typer.Option(
        4.0, help="Retry a case whose 1-minute load average exceeds this."
    ),
) -> None:
    if tool_versions == "latest":
        versions = latest_versions()
    elif tool_versions == "pinned":
        versions = tomllib.loads(
            Path(__file__).with_name("benchmark_tool_versions.toml").read_text()
        )
    else:
        raise typer.BadParameter("--tool-versions must be 'pinned' or 'latest'")
    run_dir = (output_dir / datetime.now(UTC).strftime("%Y%m%dT%H%M%SZ")).resolve()
    with tempfile.TemporaryDirectory(prefix="ryl-benchmark-") as temporary:
        scratch = Path(temporary)
        installed = install_tools(versions, run_dir / "tools")
        # The compiled plugin is an install artifact; dprint's result cache stays
        # off through --incremental=false.
        os.environ["DPRINT_CACHE_DIR"] = str(run_dir / "tools" / "dprint-cache")
        tools = make_tools(
            ryl=ryl_bin.resolve(), installed=installed, config_dir=run_dir / "configs"
        )
        count_values = [int(v) for v in counts.split(",")]
        cases = build_cases(
            tools,
            scratch / "probe",
            sizes_kib=[int(v) for v in sizes_kib.split(",")],
            counts=count_values,
        )
        typical = next(
            c for c in cases if c.sweep == Sweep.COUNT and c.x == max(count_values)
        ).files
        agreement_inputs = [*typical, *(c.files[0] for c in cases if c.sweep == "size")]
        meta = {
            "versions": record_versions(tools, versions),
            "runs": runs,
            "cases": [
                {"sweep": c.sweep, "x": c.x, "files": [str(f) for f in c.files]}
                for c in cases
            ],
            "output_agreement_percent": output_agreement(
                agreement_inputs, tools, scratch / "agreement"
            ),
            "lint_diagnostics_by_rule": lint_agreement(
                agreement_inputs, tools, scratch / "agreement"
            ),
            "load": [],
        }
        rows = []
        for case in cases:
            for attempt in itertools.count():
                before = load_average()
                case_rows = time_case(case, tools, runs=runs, scratch=scratch / "time")
                after = load_average()
                meta["load"].append(
                    {"case": f"{case.sweep}-{case.x}", "before": before, "after": after}
                )
                if max(before, after) <= max_load or attempt == 2:
                    break
                print(f"load {after:.2f} during {case.sweep} {case.x}; retrying")
                time.sleep(60)
            rows.extend(case_rows)
            print(f"done {case.sweep} {case.x}", flush=True)
    with (run_dir / "runs.csv").open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)
    (run_dir / "meta.json").write_text(json.dumps(meta, indent=2) + "\n")
    print(f"Benchmark complete: {run_dir}")


@app.command()
def plot(
    run_dir: Path = typer.Argument(..., help="A directory written by `run`."),
    output_dir: Path | None = typer.Option(None, help="Defaults to RUN_DIR."),
) -> None:
    """Draw the README chart from a finished run."""
    with (run_dir / "runs.csv").open() as handle:
        summary = median_rows(csv.DictReader(handle))
    target = output_dir or run_dir
    target.mkdir(parents=True, exist_ok=True)
    plot_chart(summary, target / "benchmark.svg")
    with (target / "summary.csv").open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=list(summary[0]))
        writer.writeheader()
        writer.writerows(summary)


if __name__ == "__main__":
    app()
