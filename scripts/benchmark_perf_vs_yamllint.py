#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.14"
# dependencies = [
#   "matplotlib>=3.9,<4",
#   "orjson>=3.11,<4",
#   "polars>=1.30,<2",
#   "ryl",
#   "tqdm>=4.67,<5",
#   "typer>=0.16,<1",
#   "yamllint",
# ]
# ///

from __future__ import annotations

from collections.abc import Iterable
from dataclasses import dataclass
from datetime import datetime, timezone
import os
from pathlib import Path
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
    import shlex

    return shlex.quote(text)


def run_checked(
    args: list[str], *, cwd: Path | None = None
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(args, cwd=cwd, check=True, text=True, capture_output=True)


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
        lines.append(f'  key_{index:06d}: "{token}"')
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


def benchmark_case(
    case: Case,
    *,
    ryl_bin: Path,
    yamllint_bin: Path,
    runs: int,
    warmup: int,
    output_json_path: Path,
) -> dict[str, dict[str, float | list[float] | str]]:
    cfg = "extends: relaxed"
    ryl_cmd = f"{quote_shell(str(ryl_bin))} -d {quote_shell(cfg)} {quote_shell(str(case.dataset_dir))}"
    yamllint_cmd = f"{quote_shell(str(yamllint_bin))} -d {quote_shell(cfg)} {quote_shell(str(case.dataset_dir))}"
    run_checked(
        [
            "hyperfine",
            "--runs",
            str(runs),
            "--warmup",
            str(warmup),
            "--export-json",
            str(output_json_path),
            "-n",
            "ryl",
            ryl_cmd,
            "-n",
            "yamllint",
            yamllint_cmd,
        ]
    )
    raw = orjson.loads(output_json_path.read_bytes())
    parsed: dict[str, dict[str, float | list[float] | str]] = {}
    for result in raw["results"]:
        parsed[str(result["command"])] = {
            "mean": float(result["mean"]),
            "stddev": float(result["stddev"] or 0.0),
            "min": float(result["min"]),
            "max": float(result["max"]),
            "times": [float(value) for value in result["times"]],
        }
    return parsed


def plot_results(
    df: pl.DataFrame,
    out_png: Path,
    out_svg: Path,
    *,
    runs: int,
    ryl_version: str,
    yamllint_version: str,
) -> None:
    plt.style.use("seaborn-v0_8-whitegrid")
    tools = ["ryl", "yamllint"]
    sizes = sorted(int(value) for value in df["file_size_kib"].unique().to_list())
    cmap = plt.cm.Blues
    min_tone = 0.35
    max_tone = 0.95
    tones = [
        min_tone + (max_tone - min_tone) * idx / max(len(sizes) - 1, 1)
        for idx in range(len(sizes))
    ]
    fig, axes = plt.subplots(1, len(tools), figsize=(14, 5.5), sharey=True)
    if len(tools) == 1:
        axes = [axes]
    version_map = {"ryl": ryl_version, "yamllint": yamllint_version}
    for axis, tool in zip(axes, tools):
        tool_df = df.filter(pl.col("tool") == tool)
        for idx, size in enumerate(sizes):
            size_df = tool_df.filter(pl.col("file_size_kib") == size).sort("file_count")
            x_values = [int(value) for value in size_df["file_count"].to_list()]
            y_values = [float(value) for value in size_df["mean_seconds"].to_list()]
            y_stddev = [float(value) for value in size_df["stddev_seconds"].to_list()]
            color = cmap(tones[idx])
            axis.plot(
                x_values,
                y_values,
                color=color,
                linewidth=2,
                marker="o",
                label=f"{size} KiB",
            )
            axis.fill_between(
                x_values,
                [value - std for value, std in zip(y_values, y_stddev)],
                [value + std for value, std in zip(y_values, y_stddev)],
                color=color,
                alpha=0.16,
            )
        axis.set_title(version_map.get(tool, tool))
        axis.set_xlabel("Number of YAML files")
    axes[0].set_ylabel("Mean runtime (seconds)")
    axes[-1].legend(title="File size", loc="upper left")
    fig.suptitle(f"ryl vs yamllint (hyperfine, {runs} runs per point)", fontsize=13)
    fig.tight_layout()
    out_png.parent.mkdir(parents=True, exist_ok=True)
    fig.savefig(out_png, dpi=170)
    fig.savefig(out_svg)
    plt.close(fig)


@app.command()
def main(
    file_counts: str = typer.Option(
        "25,100,400,1000",
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
        "1,8,32,128",
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
    runs: int = typer.Option(10, help="Number of hyperfine runs per point."),
    warmup: int = typer.Option(2, help="Number of warmup runs per point."),
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

    timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    run_dir = output_dir / timestamp
    run_dir.mkdir(parents=True, exist_ok=True)
    dataset_root = run_dir / "datasets"
    dataset_root.mkdir(parents=True, exist_ok=True)
    raw_dir = run_dir / "hyperfine-json"
    raw_dir.mkdir(parents=True, exist_ok=True)

    ryl_bin = resolve_tool_path("ryl")
    yamllint_bin = resolve_tool_path("yamllint")
    ryl_version = run_checked([str(ryl_bin), "--version"]).stdout.strip()
    yamllint_version = run_checked([str(yamllint_bin), "--version"]).stdout.strip()

    cases = iter_cases(file_counts_values, file_size_values, dataset_root)
    rows: list[dict[str, float | int | str]] = []
    for case_index, case in enumerate(tqdm(cases, desc="Benchmark cases", unit="case")):
        materialize_case(case, seed=seed + case_index * 100_000)
        case_json = (
            raw_dir / f"files_{case.file_count}__size_{case.file_size_kib}kib.json"
        )
        results = benchmark_case(
            case,
            ryl_bin=ryl_bin,
            yamllint_bin=yamllint_bin,
            runs=runs,
            warmup=warmup,
            output_json_path=case_json,
        )
        for tool in ("ryl", "yamllint"):
            row = {
                "tool": tool,
                "file_count": case.file_count,
                "file_size_kib": case.file_size_kib,
                "mean_seconds": float(results[tool]["mean"]),
                "stddev_seconds": float(results[tool]["stddev"]),
                "min_seconds": float(results[tool]["min"]),
                "max_seconds": float(results[tool]["max"]),
            }
            rows.append(row)

    results_df = pl.DataFrame(rows).select(
        [
            "tool",
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
                "ryl_version": ryl_version,
                "yamllint_version": yamllint_version,
                "runs": runs,
                "warmup": warmup,
                "file_counts": file_counts_values,
                "file_sizes_kib": file_size_values,
            },
            option=orjson.OPT_INDENT_2,
        )
        + b"\n"
    )

    plot_png = run_dir / "benchmark.png"
    plot_svg = run_dir / "benchmark.svg"
    plot_results(
        results_df,
        plot_png,
        plot_svg,
        runs=runs,
        ryl_version=ryl_version,
        yamllint_version=yamllint_version,
    )

    if not keep_datasets:
        shutil.rmtree(dataset_root)

    print(f"Benchmark complete. Artifacts: {run_dir}")
    print(f"Versions: {ryl_version}; {yamllint_version}")
    print(f"Plot: {plot_png}")


if __name__ == "__main__":
    try:
        app()
    except KeyboardInterrupt:
        print("Interrupted.", file=sys.stderr)
        raise SystemExit(130)
