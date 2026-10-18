#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.15"
# dependencies = [
#   "matplotlib>=3.9,<4",
#   "orjson>=3.11,<4",
#   "polars>=1.30,<2",
#   "tqdm>=4.67,<5",
#   "typer>=0.16,<1",
#   "py-yaml12",
#   "pyyaml",
#   "platformdirs",
# ]
# ///

r"""Benchmark YAML tools on generated and pinned real-world files.

Fairness
--------
| Tool | Result cache isolation | Install-time artifacts kept warm |
| --- | --- | --- |
| ryl | Fresh process and empty XDG cache | Release binary |
| yamllint | Fresh process and empty XDG cache | Compiled Python bytecode |
| yamlfmt | Fresh process, no global config, empty XDG cache | Installed binary |
| Prettier | --no-cache and empty XDG cache | Installed Node modules |
| dprint | --incremental=false; compiled-plugin cache checked for other writes | Compiled local WASM plugin |

Each invocation gets a fresh XDG cache directory. Dprint shares only its compiled
plugin cache. Hyperfine warmups use the same wrapper as measured runs.

Smoke test the complete pipeline, even on a busy host, with:
uv run scripts/benchmark_yaml_tools.py --ryl-bin target/release/ryl \
    --install-pinned-tools --file-counts 1,10 --file-sizes-kib 1,9 \
    --runs 2 --warmup 1 --ignore-load
"""

from __future__ import annotations

from collections.abc import Iterable
from dataclasses import dataclass
from datetime import datetime, UTC
import fnmatch
from hashlib import sha256
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib

import formatter_corpus_check as corpus_gate
import matplotlib.pyplot as plt
import orjson
import polars as pl
from tqdm import tqdm
import typer


@dataclass(frozen=True)
class Case:
    dataset: str
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


def require_idle_host(*, ignore_load: bool = False) -> tuple[float, int]:
    if shutil.which("uptime") and shutil.which("nproc"):
        uptime = run_checked(["uptime"]).stdout
        cpus = int(run_checked(["nproc"]).stdout)
        load = float(uptime.rsplit("load average:", 1)[1].split(",", 1)[0])
    else:
        if os.name == "nt":
            raise RuntimeError("host load is unavailable on this platform")
        cpus = os.cpu_count() or 1
        load = os.getloadavg()[0]
    if not ignore_load and load >= cpus / 2:
        raise RuntimeError(
            f"host load {load:.2f} is not below half of {cpus} CPUs; timing aborted"
        )
    return load, cpus


def resolve_tool_path(name: str) -> Path:
    path = shutil.which(name)
    if path is None:
        raise RuntimeError(
            f"{name} executable not found on PATH in this uv environment"
        )
    return Path(path)


def size_bounds(size_kib: int) -> tuple[int, int]:
    target = size_kib * 1024
    return target * 3 // 4, target * 5 // 4


def materialize_case(case: Case, seed: int, documents: list[bytes]) -> None:
    if not documents or any(not document for document in documents):
        raise ValueError("benchmark documents must be nonempty")
    case.dataset_dir.mkdir(parents=True, exist_ok=True)
    target_bytes = case.file_size_kib * 1024
    lower_bound, upper_bound = size_bounds(case.file_size_kib)
    for file_index in range(case.file_count):
        payload = bytearray()
        index = (
            seed + case.file_count * 31 + case.file_size_kib * 17 + file_index
        ) % len(documents)
        while len(payload) < target_bytes:
            for _ in range(len(documents)):
                document = documents[index % len(documents)]
                index += 1
                if len(payload) + len(document) <= upper_bound:
                    payload.extend(document)
                    break
            else:
                if len(payload) < lower_bound:
                    raise RuntimeError(
                        f"{case.dataset} {case.file_size_kib} KiB file {file_index}: "
                        f"cannot reach {lower_bound}..{upper_bound} bytes"
                    )
                break
        file_path = case.dataset_dir / f"file_{file_index:05d}.yaml"
        file_path.write_bytes(payload)


def iter_cases(
    file_counts: Iterable[int],
    file_sizes_kib: Iterable[int],
    base_dir: Path,
    dataset: str,
) -> list[Case]:
    cases: list[Case] = []
    for size_kib in file_sizes_kib:
        for count in file_counts:
            case_dir = base_dir / dataset / f"files_{count:05d}__size_{size_kib:04d}kib"
            cases.append(
                Case(
                    dataset=dataset,
                    file_count=count,
                    file_size_kib=size_kib,
                    dataset_dir=case_dir,
                )
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
    versions: dict[str, str],
    yamlfmt_version: str | None = None,
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
                "plugins": [str(run_dir / "plugin.wasm")],
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
            lint_args=("check", "--no-cache", "-d", "extends: relaxed"),
            format_args=("format", "--no-cache", "-c", str(ryl_config)),
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
            version=yamlfmt_version
            or run_checked([str(yamlfmt_bin), "-version"]).stdout.strip(),
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
                "--no-cache",
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
            config=f"dprint.json; local pretty_yaml {versions['pretty_yaml']}; incremental=false",
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


def install_tools(versions: dict[str, str], run_dir: Path) -> dict[str, Path]:
    tool_dir = run_dir / "tools"
    tool_dir.mkdir()
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
    run_checked(
        [
            "uv",
            "tool",
            "install",
            "--force",
            "--compile-bytecode",
            f"yamllint=={versions['yamllint']}",
        ]
    )
    run_checked(["pixi", "global", "install", f"yamlfmt=={versions['yamlfmt']}"])
    pixi_environments = orjson.loads(
        run_checked(["pixi", "global", "list", "--json"]).stdout
    )
    yamlfmt_environment = next(
        environment
        for environment in pixi_environments
        if environment["name"] == "yamlfmt"
    )
    yamlfmt_package = next(
        package
        for package in yamlfmt_environment["dependencies"]
        if package["name"] == "yamlfmt"
    )
    if yamlfmt_package["version"] != versions["yamlfmt"]:
        raise RuntimeError(
            f"yamlfmt version {yamlfmt_package['version']} differs from pin {versions['yamlfmt']}"
        )
    pixi_info = orjson.loads(run_checked(["pixi", "info", "--json"]).stdout)
    executable_suffix = ".exe" if os.name == "nt" else ""
    uv_bin = Path(run_checked(["uv", "tool", "dir", "--bin"]).stdout.strip())
    return {
        "yamllint": uv_bin / f"yamllint{executable_suffix}",
        "yamlfmt": Path(pixi_info["global_info"]["bin_dir"])
        / f"yamlfmt{executable_suffix}",
        "prettier": tool_dir
        / "node_modules/.bin"
        / f"prettier{'.cmd' if os.name == 'nt' else ''}",
        "dprint": tool_dir
        / "node_modules/.bin"
        / f"dprint{'.cmd' if os.name == 'nt' else ''}",
    }


def install_plugin(version: str, run_dir: Path) -> None:
    archive = (
        run_checked(
            [
                "npm",
                "pack",
                f"dprint-plugin-yaml@{version}",
                "--pack-destination",
                str(run_dir),
            ]
        )
        .stdout.strip()
        .splitlines()[-1]
    )
    with tarfile.open(run_dir / archive) as package:
        source = package.extractfile("package/plugin.wasm")
        if source is None:
            raise RuntimeError("pinned dprint YAML plugin has no wasm payload")
        (run_dir / "plugin.wasm").write_bytes(source.read())


def write_runner(run_dir: Path) -> Path:
    runner = run_dir / "cold_run.py"
    dprint_cache = run_dir / "dprint-cache"
    runner.write_text(
        "import os, subprocess, sys, tempfile\n"
        "with tempfile.TemporaryDirectory(prefix='ryl-tool-cache-') as cache:\n"
        "    env = os.environ.copy()\n"
        f"    env.update(XDG_CACHE_HOME=cache, DPRINT_CACHE_DIR={str(dprint_cache)!r})\n"
        "    sys.exit(subprocess.run(sys.argv[1:], env=env, check=False).returncode)\n"
    )
    return runner


def cold_command(command: list[str], runner: Path) -> list[str]:
    return [sys.executable, str(runner), *command]


def write_warm_runner(run_dir: Path) -> Path:
    runner = run_dir / "warm_run.py"
    runner.write_text(
        "import os, subprocess, sys\n"
        "from pathlib import Path\n"
        "cache = Path(sys.argv[1])\n"
        "cache.mkdir(parents=True, exist_ok=True)\n"
        "env = os.environ.copy()\n"
        "env.update(XDG_CACHE_HOME=str(cache), DPRINT_CACHE_DIR=str(cache))\n"
        "sys.exit(subprocess.run(sys.argv[2:], env=env, check=False).returncode)\n"
    )
    return runner


def warm_tool_command(tool: Tool, mode: str, dataset: Path, cache: Path) -> list[str]:
    command = tool_command(tool, mode, dataset)
    if tool.name == "ryl":
        command.remove("--no-cache")
        command[2:2] = ["--cache-dir", str(cache)]
    elif tool.name == "Prettier":
        command.remove("--no-cache")
        command[1:1] = ["--cache", "--cache-location", str(cache / "prettier-cache")]
    elif tool.name == "dprint":
        command.remove("--incremental=false")
    return command


def benchmark_warm_case(
    case: Case,
    *,
    mode: str,
    tools: list[Tool],
    runs: int,
    output_json_path: Path,
    root: Path,
    runner: Path,
) -> list[dict[str, float | int | str]]:
    selected = [
        tool
        for tool in tools
        if (tool.lint_args if mode == "lint" else tool.format_args)
    ]
    root.mkdir(parents=True, exist_ok=True)
    commands = []
    for tool in selected:
        work = root / f"{tool.name}-{mode}-work"
        cache = root / f"{tool.name}-{mode}-cache"
        if work.exists():
            shutil.rmtree(work)
        if cache.exists():
            shutil.rmtree(cache)
        shutil.copytree(case.dataset_dir, work)
        command = [
            sys.executable,
            str(runner),
            str(cache),
            *warm_tool_command(tool, mode, work, cache),
        ]
        run_checked(command)
        commands.append(command)
    hyperfine = [
        "hyperfine",
        "--runs",
        str(runs),
        "--warmup",
        "0",
        "--export-json",
        str(output_json_path),
    ]
    for tool, command in zip(selected, commands, strict=True):
        hyperfine.extend(["-n", tool.name, shell_command(command)])
    run_checked(hyperfine)
    raw = orjson.loads(output_json_path.read_bytes())
    return [
        {
            "mode": mode,
            "dataset": case.dataset,
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


def accepted_documents(
    *, seed: int, root: Path, tools: list[Tool]
) -> tuple[list[bytes], int, list[int]]:
    candidates = root / "candidates"
    environment = os.environ.copy()
    environment["BENCH_CORPUS_DIR"] = str(candidates)
    environment["BENCH_CORPUS_SEED"] = str(seed)
    subprocess.run(
        [
            str(resolve_tool_path("cargo")),
            "test",
            "--release",
            "--features",
            "benchmark-corpus",
            "--test",
            "benchmark_corpus",
            "--",
            "--ignored",
            "--exact",
            "write_benchmark_candidates",
        ],
        check=True,
        env=environment,
    )
    probe = root / "probe"
    probe.mkdir()
    sample = probe / "file_00000.yaml"
    accepted: list[bytes] = []
    accepted_indices: list[int] = []
    attempted = 0
    checks = [
        (next(tool for tool in tools if tool.name == "yamllint"), "lint"),
        (next(tool for tool in tools if tool.name == "ryl"), "lint"),
        *((tool, "format") for tool in tools if tool.format_args),
    ]
    for candidate in sorted(candidates.glob("*.yaml")):
        attempted += 1
        raw = candidate.read_bytes()
        document = b"---\n" + raw
        if not document.endswith(b"\n"):
            document += b"\n"
        valid = True
        for tool, mode in checks:
            sample.write_bytes(document)
            command = tool_command(tool, mode, probe)
            result = subprocess.run(command, capture_output=True, check=False)
            if result.returncode != 0 or (
                mode == "format" and sample.read_bytes() == document
            ):
                valid = False
                break
        if valid:
            accepted.append(document)
            accepted_indices.append(attempted - 1)
    if len(accepted) < 32:
        raise RuntimeError(
            f"only {len(accepted)} of {attempted} generated documents passed every tool"
        )
    return accepted, attempted, accepted_indices


def accepts_document(
    document: bytes, probe: Path, tools: list[Tool], runner: Path
) -> bool:
    sample = probe / "file_00000.yaml"
    checks = [
        (tool, mode)
        for tool in tools
        for mode in ("lint", "format")
        if (tool.lint_args if mode == "lint" else tool.format_args)
    ]
    for tool, mode in checks:
        sample.write_bytes(document)
        result = subprocess.run(
            cold_command(tool_command(tool, mode, probe), runner),
            capture_output=True,
            check=False,
        )
        if result.returncode != 0 or (
            mode == "format" and sample.read_bytes() == document
        ):
            return False
    return True


def real_world_pool(
    *,
    seed: int,
    sizes: list[int],
    count: int,
    root: Path,
    tools: list[Tool],
    runner: Path,
) -> tuple[dict[int, list[Path]], list[dict[str, str]], dict[str, int]]:
    repos = corpus_gate._manifest(())
    clone_dir = corpus_gate._DEFAULT_CACHE / "repos"
    files: list[tuple[Path, int]] = []
    for repo in repos:
        clone = corpus_gate._fetch(repo, clone_dir)
        for path in sorted(clone.rglob("*")):
            if (
                path.suffix.lower() not in {".yaml", ".yml"}
                or ".git" in path.parts
                or path.is_symlink()
                or not path.is_file()
            ):
                continue
            relative = path.relative_to(clone).as_posix()
            if any(
                fnmatch.fnmatch(relative, pattern) for pattern in repo.expected_skip
            ):
                continue
            marker = repo.expected_skip_marker
            if (
                path.name == marker.get("file")
                and (path.parent / marker.get("marker", "")).exists()
            ):
                continue
            files.append((path, path.stat().st_size))
    probe = root / "real-probe"
    probe.mkdir()
    chosen: dict[int, list[Path]] = {}
    attempts: dict[str, int] = {}
    for size in sizes:
        lower_bound, upper_bound = size_bounds(size)
        candidates = [
            path for path, length in files if lower_bound <= length <= upper_bound
        ]
        candidates.sort(
            key=lambda path: sha256(
                f"{seed}:{size}:{path.relative_to(clone_dir)}".encode()
            ).digest()
        )
        accepted: list[Path] = []
        for path in candidates:
            if accepts_document(path.read_bytes(), probe, tools, runner):
                accepted.append(path)
                if len(accepted) == count:
                    break
        if len(accepted) < count:
            raise RuntimeError(
                f"real-world {size} KiB bucket: {len(accepted)}/{count} accepted "
                f"from {len(candidates)} candidates"
            )
        chosen[size] = accepted
        attempts[str(size)] = candidates.index(path) + 1
    pins = [{"url": repo.url, "sha": repo.sha} for repo in repos]
    return chosen, pins, attempts


def materialize_real_case(case: Case, pool: dict[int, list[Path]]) -> None:
    case.dataset_dir.mkdir(parents=True, exist_ok=True)
    for index, source in enumerate(pool[case.file_size_kib][: case.file_count]):
        shutil.copyfile(source, case.dataset_dir / f"file_{index:05d}.yaml")


def cache_snapshot(paths: Iterable[Path]) -> list[tuple[str, int, int]]:
    return sorted(
        (str(file), file.stat().st_size, file.stat().st_mtime_ns)
        for root in paths
        if root.exists()
        for file in root.rglob("*")
        if file.is_file()
    )


def compiled_plugin_snapshot(cache: Path) -> list[tuple[str, int, int]]:
    snapshot = cache_snapshot([cache])
    files = [Path(path).relative_to(cache) for path, _, _ in snapshot]
    if len(files) != 2 or {path.suffix for path in files} != {".cwasm", ".json"}:
        raise RuntimeError(
            f"dprint cache contains more than the compiled plugin: {files}"
        )
    if (
        any(path.parent != Path("plugins") for path in files)
        or len({path.stem for path in files}) != 1
    ):
        raise RuntimeError(f"dprint cache contains unexpected files: {files}")
    return snapshot


def warm_dprint(tool: Tool, root: Path, runner: Path) -> None:
    scratch = root / "dprint-warmup"
    scratch.mkdir()
    (scratch / "file_00000.yaml").write_text("key:  value\n")
    run_checked(cold_command(tool_command(tool, "format", scratch), runner))
    shutil.rmtree(runner.parent / "dprint-cache" / "locks", ignore_errors=True)
    compiled_plugin_snapshot(runner.parent / "dprint-cache")


def verify_cold_runs(
    tools: list[Tool], document: bytes, root: Path, runner: Path
) -> None:
    persistent = [
        Path(corpus_gate.platformdirs.user_cache_dir(name))
        for name in ("ryl", "yamllint", "yamlfmt", "dprint")
    ]
    persistent.extend(
        (
            runner.parent / "tools" / "node_modules" / ".cache",
            Path.cwd() / "node_modules" / ".cache" / "prettier",
        )
    )
    probe = root / "cache-probe"
    probe.mkdir()
    dprint_cache = runner.parent / "dprint-cache"
    for tool in tools:
        for mode in ("lint", "format"):
            if not (tool.lint_args if mode == "lint" else tool.format_args):
                continue
            before = cache_snapshot(persistent)
            compiled_before = compiled_plugin_snapshot(dprint_cache)
            for _ in range(2):
                (probe / "file_00000.yaml").write_bytes(document)
                run_checked(cold_command(tool_command(tool, mode, probe), runner))
            if cache_snapshot(persistent) != before:
                raise RuntimeError(f"{tool.name} {mode} wrote to a persistent cache")
            if compiled_plugin_snapshot(dprint_cache) != compiled_before:
                raise RuntimeError(
                    f"{tool.name} {mode} changed the dprint plugin cache"
                )


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
    runner: Path,
) -> list[dict[str, float | int | str]]:
    selected = [
        tool
        for tool in tools
        if (tool.lint_args if mode == "lint" else tool.format_args)
    ]
    dataset = case.dataset_dir if mode == "lint" else work_dir
    commands = [
        cold_command(tool_command(tool, mode, dataset), runner) for tool in selected
    ]
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
    dprint_cache = runner.parent / "dprint-cache"
    compiled_before = compiled_plugin_snapshot(dprint_cache)
    run_checked(hyperfine)
    if compiled_plugin_snapshot(dprint_cache) != compiled_before:
        raise RuntimeError("timed run changed the dprint plugin cache")
    raw = orjson.loads(output_json_path.read_bytes())
    return [
        {
            "mode": mode,
            "dataset": case.dataset,
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


def format_profile(
    case: Case,
    ryl_tool: Tool,
    root: Path,
    raw_dir: Path,
    runs: int,
    warmup: int,
    runner: Path,
) -> dict[str, float | int]:
    clean_dir = root / "already-formatted"
    shutil.copytree(case.dataset_dir, clean_dir)
    run_checked(cold_command(tool_command(ryl_tool, "format", clean_dir), runner))
    commands = [
        (
            "already_formatted",
            cold_command(tool_command(ryl_tool, "format", clean_dir), runner),
        ),
        (
            "check_dirty",
            cold_command(
                [
                    str(ryl_tool.binary),
                    *ryl_tool.format_args,
                    "--check",
                    str(case.dataset_dir),
                ],
                runner,
            ),
        ),
    ]
    results: dict[str, float | int] = {
        "file_count": case.file_count,
        "file_size_kib": case.file_size_kib,
    }
    for name, command in commands:
        output = raw_dir / f"ryl-{name}.json"
        run_checked(
            [
                "hyperfine",
                "--ignore-failure",
                "--runs",
                str(runs),
                "--warmup",
                str(warmup),
                "--export-json",
                str(output),
                shell_command(command),
            ]
        )
        results[f"{name}_mean_seconds"] = orjson.loads(output.read_bytes())["results"][
            0
        ]["mean"]
    return results


def plot_results(
    df: pl.DataFrame, out_svg: Path, *, runs: int, warm: bool = False
) -> None:
    plt.style.use("seaborn-v0_8-whitegrid")
    plt.rcParams["svg.fonttype"] = "none"
    panels = [
        ("lint", "ryl"),
        ("lint", "yamllint"),
        ("format", "ryl"),
        ("format", "yamlfmt"),
        ("format", "Prettier"),
        ("format", "dprint"),
    ]
    sizes = sorted({int(value) for value in df["file_size_kib"].to_list()})
    plotted_datasets = set(df["dataset"].to_list())
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
            for dataset, style in (("real-world", "-"), ("stress", "--")):
                if dataset not in plotted_datasets:
                    continue
                size_df = tool_df.filter(
                    (pl.col("file_size_kib") == size) & (pl.col("dataset") == dataset)
                ).sort("file_count")
                axis.plot(
                    [int(value) for value in size_df["file_count"].to_list()],
                    [float(value) for value in size_df["mean_seconds"].to_list()],
                    color=cmap(tones[idx]),
                    linestyle=style,
                    linewidth=2,
                    marker="o",
                    label=(
                        f"{dataset}, {size} KiB"
                        if len(plotted_datasets) > 1
                        else f"{size} KiB"
                    ),
                )
        axis.set_title(f"{mode}: {tool}")
        axis.set_xlabel("Number of YAML files")
    for group in (panel_axes[:2], panel_axes[2:]):
        ceiling = max(axis.get_ylim()[1] for axis in group)
        for axis in group:
            axis.set_ylim(0, ceiling)
    panel_axes[0].set_ylabel("Mean runtime (seconds)")
    panel_axes[2].set_ylabel("Mean runtime (seconds)")
    legend_title = "Dataset and file size" if len(plotted_datasets) > 1 else "File size"
    panel_axes[-1].legend(title=legend_title, loc="upper left")
    fig.suptitle(
        f"YAML lint and format ({'warm' if warm else 'cold'} cache, {runs} runs per point)",
        fontsize=13,
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
        help="ryl binary to benchmark (default: the `ryl` on PATH). "
        "Point at this branch's target/release/ryl so the image reflects the "
        "release build rather than a stale global install.",
    ),
    yamllint_bin: Path | None = typer.Option(None, help="yamllint executable."),
    yamlfmt_bin: Path | None = typer.Option(None, help="yamlfmt executable."),
    prettier_bin: Path | None = typer.Option(None, help="Prettier executable."),
    dprint_bin: Path | None = typer.Option(None, help="dprint executable."),
    seed: int = typer.Option(7331, help="Base RNG seed for synthetic YAML generation."),
    install_pinned_tools: bool = typer.Option(
        False, help="Install exact tool versions from benchmark_tool_versions.toml."
    ),
    profile_format: bool = typer.Option(
        False, help="Also time ryl on formatted input and dirty --check input."
    ),
    warm: bool = typer.Option(False, help="Also chart each tool's second, cached run."),
    ignore_load: bool = typer.Option(
        False, help="Ignore the host load guard for smoke tests only."
    ),
    output_dir: Path = typer.Option(
        Path("manual_outputs") / "benchmarks",
        help="Directory where all artifacts are written.",
    ),
    datasets: str = typer.Option(
        "real-world,stress", help="Comma-separated datasets: real-world, stress."
    ),
    replot: Path | None = typer.Option(
        None,
        help="Rebuild benchmark.svg (and benchmark-warm.svg) from an existing run.",
    ),
) -> None:
    selected_datasets = {part.strip() for part in datasets.split(",")}
    if not selected_datasets <= {"real-world", "stress"}:
        raise typer.BadParameter("--datasets must contain real-world and/or stress")
    if replot is not None:
        run_dir = replot.resolve()
        results_df = pl.read_csv(run_dir / "summary.csv").filter(
            pl.col("dataset").is_in(selected_datasets)
        )
        if not selected_datasets <= set(results_df["dataset"].to_list()):
            raise typer.BadParameter("selected datasets are missing from summary.csv")
        runs = orjson.loads((run_dir / "meta.json").read_bytes())["runs"]
        plot_svg = run_dir / "benchmark.svg"
        plot_results(results_df, plot_svg, runs=runs)
        print(f"Plot: {plot_svg}")
        warm_csv = run_dir / "warm-summary.csv"
        if warm_csv.exists():
            warm_svg = run_dir / "benchmark-warm.svg"
            plot_results(
                pl.read_csv(warm_csv).filter(
                    pl.col("dataset").is_in(selected_datasets)
                ),
                warm_svg,
                runs=runs,
                warm=True,
            )
            print(f"Plot: {warm_svg}")
        return
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
    versions = tomllib.loads(
        (Path(__file__).with_name("benchmark_tool_versions.toml")).read_text()
    )

    timestamp = datetime.now(UTC).strftime("%Y%m%dT%H%M%SZ")
    run_dir = (output_dir / timestamp).resolve()
    run_dir.mkdir(parents=True, exist_ok=True)
    raw_dir = run_dir / "hyperfine-json"
    raw_dir.mkdir(parents=True, exist_ok=True)
    installed = install_tools(versions, run_dir) if install_pinned_tools else {}
    install_plugin(versions["pretty_yaml"], run_dir)
    runner = write_runner(run_dir)
    warm_runner = write_warm_runner(run_dir) if warm else None

    def binary(value: Path | None, name: str) -> Path:
        path = value if value is not None else resolve_tool_path(name)
        if not path.is_file():
            raise typer.BadParameter(f"--{name}-bin does not exist: {path}")
        return path.resolve()

    tools = make_tools(
        ryl_bin=binary(ryl_bin, "ryl"),
        yamllint_bin=binary(installed.get("yamllint") or yamllint_bin, "yamllint"),
        yamlfmt_bin=binary(installed.get("yamlfmt") or yamlfmt_bin, "yamlfmt"),
        prettier_bin=binary(installed.get("prettier") or prettier_bin, "prettier"),
        dprint_bin=binary(installed.get("dprint") or dprint_bin, "dprint"),
        run_dir=run_dir,
        versions=versions,
        yamlfmt_version=versions["yamlfmt"] if install_pinned_tools else None,
    )
    warm_dprint(next(tool for tool in tools if tool.name == "dprint"), run_dir, runner)
    for tool in tools:
        pin = versions.get(tool.name.lower())
        if pin and tool.name != "yamlfmt" and pin not in tool.version:
            raise RuntimeError(
                f"{tool.name} version {tool.version!r} differs from pin {pin}"
            )
    rows: list[dict[str, float | int | str]] = []
    warm_rows: list[dict[str, float | int | str]] = []
    with tempfile.TemporaryDirectory(prefix="ryl-benchmark-") as temporary:
        temporary_root = Path(temporary)
        documents, attempted, accepted_indices = accepted_documents(
            seed=seed, root=temporary_root, tools=tools
        )
        verify_cold_runs(tools, documents[0], temporary_root, runner)
        real_pool, repo_pins, real_attempts = (
            real_world_pool(
                seed=seed,
                sizes=file_size_values,
                count=max(file_counts_values),
                root=temporary_root,
                tools=tools,
                runner=runner,
            )
            if "real-world" in selected_datasets
            else ({}, [], {})
        )
        cases = [
            case
            for dataset in ("stress", "real-world")
            if dataset in selected_datasets
            for case in iter_cases(
                file_counts_values,
                file_size_values,
                temporary_root / "datasets",
                dataset,
            )
        ]
        load, cpus = require_idle_host(ignore_load=ignore_load)
        corpus_digests = {dataset: sha256() for dataset in selected_datasets}
        bucket_sizes = []
        for case in tqdm(cases, desc="Benchmark cases", unit="case"):
            if case.dataset == "stress":
                materialize_case(case, seed=seed, documents=documents)
            else:
                materialize_real_case(case, real_pool)
            paths = sorted(case.dataset_dir.glob("*.yaml"))
            sizes = [path.stat().st_size for path in paths]
            lower_bound, upper_bound = size_bounds(case.file_size_kib)
            if len(sizes) != case.file_count or any(
                not lower_bound <= size <= upper_bound for size in sizes
            ):
                raise RuntimeError(f"invalid {case.dataset} size bucket: {case}")
            bucket_sizes.append(
                {
                    "dataset": case.dataset,
                    "file_count": case.file_count,
                    "file_size_kib": case.file_size_kib,
                    "min_bytes": min(sizes),
                    "max_bytes": max(sizes),
                }
            )
            for path in paths:
                corpus_digests[case.dataset].update(
                    str(path.relative_to(temporary_root)).encode()
                )
                corpus_digests[case.dataset].update(path.read_bytes())
            for mode in ("lint", "format"):
                rows.extend(
                    benchmark_case(
                        case,
                        mode=mode,
                        tools=tools,
                        runs=runs,
                        warmup=warmup,
                        output_json_path=raw_dir
                        / f"{case.dataset}-{mode}-{case.file_count}-{case.file_size_kib}.json",
                        work_dir=temporary_root / "working",
                        runner=runner,
                    )
                )
                if warm_runner is not None:
                    warm_rows.extend(
                        benchmark_warm_case(
                            case,
                            mode=mode,
                            tools=tools,
                            runs=runs,
                            output_json_path=raw_dir
                            / f"warm-{case.dataset}-{mode}-{case.file_count}-{case.file_size_kib}.json",
                            root=temporary_root / "warm",
                            runner=warm_runner,
                        )
                    )
        profile = None
        if profile_format:
            largest_case = max(
                cases, key=lambda case: case.file_count * case.file_size_kib
            )
            profile = format_profile(
                largest_case, tools[0], temporary_root, raw_dir, runs, warmup, runner
            )
        corpus_hashes = {
            name: digest.hexdigest() for name, digest in corpus_digests.items()
        }

    results_df = pl.DataFrame(rows).select(
        [
            "tool",
            "mode",
            "dataset",
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
    if warm_rows:
        warm_df = pl.DataFrame(warm_rows).select(results_df.columns)
        warm_df.write_csv(run_dir / "warm-summary.csv")
        plot_results(warm_df, run_dir / "benchmark-warm.svg", runs=runs, warm=True)

    meta_path = run_dir / "meta.json"
    meta_path.write_bytes(
        orjson.dumps(
            {
                "generated_at_utc": timestamp,
                "runs": runs,
                "warmup": warmup,
                "warm_cache_view": warm,
                "seed": seed,
                "pre_run_load_1m": load,
                "cpu_count": cpus,
                "strategy_set": [
                    "property_format::arb_document_with_properties",
                    "property_safe_fix::arb_document",
                ],
                "corpus_sha256": corpus_hashes,
                "bucket_sizes": bucket_sizes,
                "real_world": {
                    "repos": repo_pins,
                    "selection_seed": seed,
                    "accepted_after_candidates": real_attempts,
                    "size_tolerance": "within 25% of target KiB",
                },
                "cache_probe": "two fresh-process runs per tool and mode; persistent cache snapshots unchanged",
                "candidate_reject_rate": (attempted - len(documents)) / attempted,
                "candidate_attempts": attempted,
                "candidate_accepted": len(documents),
                "accepted_candidate_indices": accepted_indices,
                "tool_pins": versions,
                "ryl_format_profile": profile,
                "fixture": "proptest documents joined as YAML streams; every formatter changes each candidate",
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

    print(f"Benchmark complete. Artifacts: {run_dir}")
    print(f"Plot: {plot_svg}")


if __name__ == "__main__":
    try:
        app()
    except KeyboardInterrupt:
        print("Interrupted.", file=sys.stderr)
        raise SystemExit(130)
