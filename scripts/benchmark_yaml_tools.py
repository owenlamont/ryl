#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.15"
# dependencies = [
#   "matplotlib>=3.9,<4",
#   "orjson>=3.11,<4",
#   "polars>=1.30,<2",
#   "tqdm>=4.67,<5",
#   "typer>=0.16,<1",
#   "yamllint",
# ]
# ///

from __future__ import annotations

from collections.abc import Iterable
from dataclasses import dataclass
from datetime import datetime, UTC
from hashlib import sha256
import os
from pathlib import Path
import shlex
import shutil
import string
import subprocess
import sys

import matplotlib.pyplot as plt
import orjson
import polars as pl
from tqdm import tqdm
import typer


@dataclass(frozen=True)
class Case:
    file_count: int
    file_size_kib: int
    dataset_dir: Path


@dataclass(frozen=True, kw_only=True)
class Tool:
    name: str
    binary: Path
    version: str
    config: str
    lint_args: tuple[str, ...] = ()
    format_args: tuple[str, ...] = ()


app = typer.Typer(add_completion=False)


def parse_int_list(raw: str) -> list[int]:
    values = [int(part.strip()) for part in raw.split(",") if part.strip()]
    if not values:
        raise ValueError("expected at least one integer")
    if any(value <= 0 for value in values):
        raise ValueError("all values must be > 0")
    return values


def quote_shell(text: str) -> str:
    if os.name == "nt":
        return subprocess.list2cmdline([text])
    return shlex.quote(text)


def shell_command(args: Iterable[str]) -> str:
    return " ".join(quote_shell(arg) for arg in args)


def run_checked(
    args: list[str], *, cwd: Path | None = None
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args, cwd=cwd, check=True, text=True, capture_output=True, encoding="utf-8"
    )


def require_command(name: str) -> None:
    if shutil.which(name) is None:
        raise RuntimeError(f"required command is not installed or not on PATH: {name}")


def resolve_tool_path(name: str) -> Path:
    path = shutil.which(name)
    if path is None:
        raise RuntimeError(
            f"{name} executable not found on PATH in this uv environment"
        )
    return Path(path)


def build_yaml_blob(target_bytes: int, seed: int) -> str:
    lines = ["root:"]
    alphabet = string.ascii_lowercase + string.digits
    index = 0
    while len(("\n".join(lines) + "\n").encode("utf-8")) < target_bytes:
        offset = (seed + index) % len(alphabet)
        token = "".join(alphabet[(offset + i) % len(alphabet)] for i in range(28))
        lines.append(f'  key_{index:06d}:  "{token}"')
        index += 1
    return "\n".join(lines) + "\n"


def materialize_case(case: Case, seed: int) -> None:
    case.dataset_dir.mkdir(parents=True, exist_ok=True)
    target_bytes = case.file_size_kib * 1024
    for file_index in range(case.file_count):
        payload = build_yaml_blob(target_bytes=target_bytes, seed=seed + file_index)
        file_path = case.dataset_dir / f"file_{file_index:05d}.yaml"
        file_path.write_text(payload, encoding="utf-8")


def iter_cases(
    file_counts: Iterable[int], file_sizes_kib: Iterable[int], base_dir: Path
) -> list[Case]:
    cases: list[Case] = []
    for size_kib in file_sizes_kib:
        for count in file_counts:
            case_dir = base_dir / f"files_{count:05d}__size_{size_kib:04d}kib"
            cases.append(
                Case(file_count=count, file_size_kib=size_kib, dataset_dir=case_dir)
            )
    return cases


def expand_range(
    *, start: int | None, end: int | None, step: int | None, value_name: str
) -> list[int] | None:
    values = (start, end, step)
    if all(value is None for value in values):
        return None
    if any(value is None for value in values):
        raise typer.BadParameter(
            f"{value_name} range requires start, end, and step together."
        )
    if start <= 0 or end <= 0 or step <= 0:
        raise typer.BadParameter(f"{value_name} range values must be > 0.")
    if end < start:
        raise typer.BadParameter(f"{value_name} range end must be >= start.")
    return list(range(start, end + 1, step))


def make_tools(
    *,
    ryl_bin: Path,
    yamllint_bin: Path,
    yamlfmt_bin: Path,
    prettier_bin: Path,
    dprint_bin: Path,
    run_dir: Path,
) -> list[Tool]:
    ryl_config = run_dir / "ryl.toml"
    ryl_config.write_text("[format]\n")
    prettier_ignore = run_dir / "prettier.ignore"
    prettier_ignore.write_text("")
    dprint_config = run_dir / "dprint.json"
    dprint_config.write_bytes(
        orjson.dumps(
            {
                "includes": ["**/*.yaml"],
                "plugins": ["npm:dprint-plugin-yaml@0.6.0"],
                "yaml": {},
            },
            option=orjson.OPT_INDENT_2,
        )
        + b"\n"
    )
    return [
        Tool(
            name="ryl",
            binary=ryl_bin,
            version=run_checked([str(ryl_bin), "--version"]).stdout.strip(),
            config="lint: extends relaxed; format: [format] defaults",
            lint_args=("check", "-d", "extends: relaxed"),
            format_args=("format", "-c", str(ryl_config)),
        ),
        Tool(
            name="yamllint",
            binary=yamllint_bin,
            version=run_checked([str(yamllint_bin), "--version"]).stdout.strip(),
            config="extends: relaxed",
            lint_args=("-d", "extends: relaxed"),
        ),
        Tool(
            name="yamlfmt",
            binary=yamlfmt_bin,
            version=run_checked([str(yamlfmt_bin), "-version"]).stdout.strip(),
            config="basic; line_ending=lf; no global config",
            format_args=("-no_global_conf", "-formatter", "line_ending=lf"),
        ),
        Tool(
            name="Prettier",
            binary=prettier_bin,
            version=run_checked([str(prettier_bin), "--version"]).stdout.strip(),
            config="defaults; no config or editorconfig; empty ignore file",
            format_args=(
                "--write",
                "--no-config",
                "--no-editorconfig",
                "--ignore-path",
                str(prettier_ignore),
            ),
        ),
        Tool(
            name="dprint",
            binary=dprint_bin,
            version=run_checked([str(dprint_bin), "--version"]).stdout.strip(),
            config="dprint.json; pretty_yaml 0.6.0 defaults; incremental=false",
            format_args=(
                "fmt",
                "--config",
                str(dprint_config),
                "--incremental=false",
                "--no-gitignore",
                "--log-level",
                "silent",
            ),
        ),
    ]


def tool_command(tool: Tool, mode: str, dataset: Path) -> list[str]:
    args = tool.lint_args if mode == "lint" else tool.format_args
    input_path = str(dataset / "*.yaml") if tool.name == "Prettier" else str(dataset)
    return [str(tool.binary), *args, input_path]


def benchmark_case(
    case: Case,
    *,
    mode: str,
    tools: list[Tool],
    runs: int,
    warmup: int,
    output_json_path: Path,
    work_dir: Path,
) -> list[dict[str, float | int | str]]:
    selected = [
        tool
        for tool in tools
        if (tool.lint_args if mode == "lint" else tool.format_args)
    ]
    dataset = case.dataset_dir if mode == "lint" else work_dir
    commands = [tool_command(tool, mode, dataset) for tool in selected]
    for command in commands:
        if mode == "format":
            if work_dir.exists():
                shutil.rmtree(work_dir)
            shutil.copytree(case.dataset_dir, work_dir)
        run_checked(command)
        if mode == "format":
            sample = "file_00000.yaml"
            if (case.dataset_dir / sample).read_bytes() == (
                work_dir / sample
            ).read_bytes():
                raise RuntimeError(
                    f"formatter did not change the dirty fixture: {command[0]}"
                )
    hyperfine = [
        "hyperfine",
        "--runs",
        str(runs),
        "--warmup",
        str(warmup),
        "--export-json",
        str(output_json_path),
    ]
    if mode == "format":
        reset = (
            "import shutil,sys; "
            "shutil.rmtree(sys.argv[2],ignore_errors=True); "
            "shutil.copytree(sys.argv[1],sys.argv[2])"
        )
        hyperfine.extend(
            [
                "--prepare",
                shell_command(
                    [sys.executable, "-c", reset, str(case.dataset_dir), str(work_dir)]
                ),
            ]
        )
    for tool, command in zip(selected, commands, strict=True):
        hyperfine.extend(["-n", tool.name, shell_command(command)])
    run_checked(hyperfine)
    raw = orjson.loads(output_json_path.read_bytes())
    return [
        {
            "mode": mode,
            "tool": result["command"],
            "file_count": case.file_count,
            "file_size_kib": case.file_size_kib,
            "mean_seconds": float(result["mean"]),
            "stddev_seconds": float(result["stddev"] or 0),
            "min_seconds": float(result["min"]),
            "max_seconds": float(result["max"]),
        }
        for result in raw["results"]
    ]


def plot_results(df: pl.DataFrame, out_svg: Path, *, runs: int) -> None:
    plt.style.use("seaborn-v0_8-whitegrid")
    panels = [
        ("lint", "ryl"),
        ("lint", "yamllint"),
        ("format", "ryl"),
        ("format", "yamlfmt"),
        ("format", "Prettier"),
        ("format", "dprint"),
    ]
    sizes = sorted({int(value) for value in df["file_size_kib"].to_list()})
    cmap = plt.cm.Blues
    min_tone = 0.35
    max_tone = 0.95
    tones = [
        min_tone + (max_tone - min_tone) * idx / max(len(sizes) - 1, 1)
        for idx in range(len(sizes))
    ]
    fig = plt.figure(figsize=(18, 9))
    grid = fig.add_gridspec(2, 4)
    panel_axes = [
        fig.add_subplot(grid[0, :2]),
        fig.add_subplot(grid[0, 2:]),
        *(fig.add_subplot(grid[1, index]) for index in range(4)),
    ]
    for axis, (mode, tool) in zip(panel_axes, panels, strict=True):
        tool_df = df.filter((pl.col("mode") == mode) & (pl.col("tool") == tool))
        for idx, size in enumerate(sizes):
            size_df = tool_df.filter(pl.col("file_size_kib") == size).sort("file_count")
            x_values = [int(value) for value in size_df["file_count"].to_list()]
            y_values = [float(value) for value in size_df["mean_seconds"].to_list()]
            color = cmap(tones[idx])
            axis.plot(
                x_values,
                y_values,
                color=color,
                linewidth=2,
                marker="o",
                label=f"{size} KiB",
            )
        axis.set_title(f"{mode}: {tool}")
        axis.set_xlabel("Number of YAML files")
    for group in (panel_axes[:2], panel_axes[2:]):
        ceiling = max(axis.get_ylim()[1] for axis in group)
        for axis in group:
            axis.set_ylim(0, ceiling)
    panel_axes[0].set_ylabel("Mean runtime (seconds)")
    panel_axes[2].set_ylabel("Mean runtime (seconds)")
    panel_axes[-1].legend(title="File size", loc="upper left")
    fig.suptitle(
        f"YAML lint and format (hyperfine, {runs} runs per point)", fontsize=13
    )
    fig.tight_layout()
    out_svg.parent.mkdir(parents=True, exist_ok=True)
    fig.savefig(out_svg)
    plt.close(fig)


@app.command()
def main(
    file_counts: str = typer.Option(
        "20,40,60,80,100",
        help="Comma-separated file counts. Ignored when --file-count-start/end/step are set.",
    ),
    file_count_start: int | None = typer.Option(
        None, help="Start of file-count range (inclusive)."
    ),
    file_count_end: int | None = typer.Option(
        None, help="End of file-count range (inclusive)."
    ),
    file_count_step: int | None = typer.Option(
        None, help="Increment for file-count range."
    ),
    file_sizes_kib: str = typer.Option(
        "1,3,5,7,9",
        help="Comma-separated file sizes in KiB. Ignored when --file-size-start-kib/end/step are set.",
    ),
    file_size_start_kib: int | None = typer.Option(
        None, help="Start of file-size range in KiB (inclusive)."
    ),
    file_size_end_kib: int | None = typer.Option(
        None, help="End of file-size range in KiB (inclusive)."
    ),
    file_size_step_kib: int | None = typer.Option(
        None, help="Increment for file-size range in KiB."
    ),
    runs: int = typer.Option(5, help="Number of hyperfine runs per point."),
    warmup: int = typer.Option(1, help="Number of warmup runs per point."),
    ryl_bin: Path | None = typer.Option(
        None,
        "--ryl-bin",
        help="ryl binary to benchmark and label (default: the `ryl` on PATH). "
        "Point at this branch's target/release/ryl so the image reflects the "
        "release build rather than a stale global install; the version label is "
        "read from this binary's --version, so it always matches what was timed.",
    ),
    yamllint_bin: Path | None = typer.Option(None, help="yamllint executable."),
    yamlfmt_bin: Path | None = typer.Option(None, help="yamlfmt executable."),
    prettier_bin: Path | None = typer.Option(None, help="Prettier executable."),
    dprint_bin: Path | None = typer.Option(None, help="dprint executable."),
    seed: int = typer.Option(7331, help="Base RNG seed for synthetic YAML generation."),
    output_dir: Path = typer.Option(
        Path("manual_outputs") / "benchmarks",
        help="Directory where all artifacts are written.",
    ),
    keep_datasets: bool = typer.Option(
        False, help="Keep generated YAML datasets on disk instead of deleting them."
    ),
) -> None:
    if runs <= 0:
        raise typer.BadParameter("--runs must be > 0")
    if warmup < 0:
        raise typer.BadParameter("--warmup must be >= 0")

    file_counts_range = expand_range(
        start=file_count_start,
        end=file_count_end,
        step=file_count_step,
        value_name="file count",
    )
    file_sizes_range = expand_range(
        start=file_size_start_kib,
        end=file_size_end_kib,
        step=file_size_step_kib,
        value_name="file size",
    )
    try:
        file_counts_values = (
            file_counts_range
            if file_counts_range is not None
            else parse_int_list(file_counts)
        )
        file_size_values = (
            file_sizes_range
            if file_sizes_range is not None
            else parse_int_list(file_sizes_kib)
        )
    except ValueError as err:
        raise typer.BadParameter(str(err)) from err

    require_command("uv")
    require_command("hyperfine")

    timestamp = datetime.now(UTC).strftime("%Y%m%dT%H%M%SZ")
    run_dir = (output_dir / timestamp).resolve()
    run_dir.mkdir(parents=True, exist_ok=True)
    dataset_root = run_dir / "datasets"
    dataset_root.mkdir(parents=True, exist_ok=True)
    raw_dir = run_dir / "hyperfine-json"
    raw_dir.mkdir(parents=True, exist_ok=True)

    def binary(value: Path | None, name: str) -> Path:
        path = value if value is not None else resolve_tool_path(name)
        if not path.is_file():
            raise typer.BadParameter(f"--{name}-bin does not exist: {path}")
        return path.resolve()

    tools = make_tools(
        ryl_bin=binary(ryl_bin, "ryl"),
        yamllint_bin=binary(yamllint_bin, "yamllint"),
        yamlfmt_bin=binary(yamlfmt_bin, "yamlfmt"),
        prettier_bin=binary(prettier_bin, "prettier"),
        dprint_bin=binary(dprint_bin, "dprint"),
        run_dir=run_dir,
    )

    cases = iter_cases(file_counts_values, file_size_values, dataset_root)
    rows: list[dict[str, float | int | str]] = []
    for case_index, case in enumerate(tqdm(cases, desc="Benchmark cases", unit="case")):
        materialize_case(case, seed=seed + case_index * 100_000)
        for mode in ("lint", "format"):
            rows.extend(
                benchmark_case(
                    case,
                    mode=mode,
                    tools=tools,
                    runs=runs,
                    warmup=warmup,
                    output_json_path=raw_dir
                    / f"{mode}-{case.file_count}-{case.file_size_kib}.json",
                    work_dir=run_dir / "working",
                )
            )

    results_df = pl.DataFrame(rows).select(
        [
            "tool",
            "mode",
            "file_count",
            "file_size_kib",
            "mean_seconds",
            "stddev_seconds",
            "min_seconds",
            "max_seconds",
        ]
    )
    csv_path = run_dir / "summary.csv"
    results_df.write_csv(csv_path)

    meta_path = run_dir / "meta.json"
    meta_path.write_bytes(
        orjson.dumps(
            {
                "generated_at_utc": timestamp,
                "runs": runs,
                "warmup": warmup,
                "seed": seed,
                "fixture": "same YAML for lint and format; extra space after colons",
                "file_counts": file_counts_values,
                "file_sizes_kib": file_size_values,
                "tools": [
                    {
                        "name": tool.name,
                        "version": tool.version,
                        "binary": str(tool.binary),
                        "sha256": sha256(tool.binary.read_bytes()).hexdigest(),
                        "config": tool.config,
                        "lint_command": tool_command(tool, "lint", Path("DATASET"))
                        if tool.lint_args
                        else None,
                        "format_command": tool_command(tool, "format", Path("DATASET"))
                        if tool.format_args
                        else None,
                    }
                    for tool in tools
                ],
            },
            option=orjson.OPT_INDENT_2,
        )
        + b"\n"
    )

    plot_svg = run_dir / "benchmark.svg"
    plot_results(results_df, plot_svg, runs=runs)

    if not keep_datasets:
        shutil.rmtree(dataset_root)
        shutil.rmtree(run_dir / "working")

    print(f"Benchmark complete. Artifacts: {run_dir}")
    print(f"Plot: {plot_svg}")


if __name__ == "__main__":
    try:
        app()
    except KeyboardInterrupt:
        print("Interrupted.", file=sys.stderr)
        raise SystemExit(130)
