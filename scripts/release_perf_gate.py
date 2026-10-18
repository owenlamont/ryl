# /// script
# requires-python = ">=3.15"
# dependencies = [
#   "typer", "packaging", "py-yaml12", "pyyaml", "platformdirs", "markdown-it-py",
#   "matplotlib>=3.9,<4", "orjson>=3.11,<4", "polars>=1.30,<2", "tqdm>=4.67,<5",
# ]
# ///

from collections.abc import Mapping, Sequence
from dataclasses import asdict, dataclass
from enum import StrEnum
import math
import os
from pathlib import Path
import shutil
from statistics import mean, stdev
import subprocess
import tempfile
from time import perf_counter
import tomllib
from typing import Annotated, Final

import benchmark_yaml_tools as harness
from changelog import release_notes
from downstream_check import published_binary
from markdown_it import MarkdownIt
import orjson
import typer


app = typer.Typer(
    add_completion=False, context_settings={"help_option_names": ["-h", "--help"]}
)


class _Mode(StrEnum):
    LINT = "lint"
    FORMAT = "format"


@dataclass(frozen=True, slots=True, kw_only=True)
class Result:
    case: str
    previous_seconds: float
    previous_sd: float
    candidate_seconds: float
    candidate_sd: float

    @property
    def delta_percent(self) -> float:
        """Positive values denote a slowdown."""
        return (self.candidate_seconds / self.previous_seconds - 1.0) * 100.0


_CARGO_MANIFEST: Final = Path("Cargo.toml")


def _manifest_release() -> str:
    return f"v{tomllib.loads(_CARGO_MANIFEST.read_text())['package']['version']}"


def accepted_slowdowns(*, changelog: str, release: str) -> dict[str, str]:
    """Return `Performance: CASE: REASON` bullet acceptances from one release.

    The selected release must have a unique, nonempty section.
    """
    tokens = MarkdownIt().parse(release_notes(changelog, release))
    accepted: dict[str, str] = {}
    for index, token in enumerate(tokens):
        if (
            token.type == "inline"
            and token.level == 3
            and index > 0
            and tokens[index - 1].type == "paragraph_open"
            and index > 1
            and tokens[index - 2].type == "list_item_open"
        ):
            marker, separator, rest = token.content.partition(": ")
            case, reason_separator, reason = rest.partition(": ")
            if (
                marker == "Performance"
                and separator
                and case
                and reason_separator
                and reason.strip()
            ):
                accepted[case] = reason.strip()
    return accepted


def regressions(
    *, results: Sequence[Result], tolerance: float, accepted: Mapping[str, str]
) -> list[str]:
    """Return cases strictly beyond tolerance without a case-specific acceptance."""
    return [
        result.case
        for result in results
        if result.candidate_seconds
        > result.previous_seconds * (1.0 + tolerance / 100.0)
        and result.case not in accepted
    ]


def _tool(*, binary: Path, name: str, config: Path) -> harness.Tool:
    help_text = harness.run_checked([str(binary), "check", "--help"]).stdout
    cache_flags = ("--no-cache",) if "--no-cache" in help_text else ()
    return harness.Tool(
        name=name,
        binary=binary,
        version=harness.run_checked([str(binary), "--version"]).stdout.strip(),
        config="README: relaxed lint; default format; caches disabled",
        lint_args=("check", *cache_flags, "-d", "extends: relaxed"),
        format_args=("format", *cache_flags, "-c", str(config)),
    )


def _measure(
    *,
    case: harness.Case,
    mode: _Mode,
    tools: Sequence[harness.Tool],
    runs: int,
    warmup: int,
    root: Path,
) -> Result:
    samples: dict[str, list[float]] = {tool.name: [] for tool in tools}
    work = root / "work"
    for iteration in range(warmup + runs):
        harness.require_idle_host()
        order = tools if iteration % 2 == 0 else tuple(reversed(tools))
        for tool in order:
            if work.exists():
                shutil.rmtree(work)
            shutil.copytree(case.dataset_dir, work)
            with tempfile.TemporaryDirectory(prefix="ryl-perf-cache-") as cache:
                started = perf_counter()
                subprocess.run(
                    harness.tool_command(tool, mode, work),
                    cwd=root,
                    env=os.environ | {"XDG_CACHE_HOME": cache, "RYL_NO_CACHE": "1"},
                    capture_output=True,
                    check=True,
                    timeout=300,
                )
                elapsed = perf_counter() - started
            if iteration >= warmup:
                samples[tool.name].append(elapsed)
        harness.require_idle_host()
    previous, candidate = (samples[tool.name] for tool in tools)
    return Result(
        case=f"{mode}/{case.file_count}x{case.file_size_kib}KiB",
        previous_seconds=mean(previous),
        previous_sd=stdev(previous),
        candidate_seconds=mean(candidate),
        candidate_sd=stdev(candidate),
    )


@app.command()
def main(
    *,
    ryl_bin: Annotated[Path, typer.Option(exists=True, dir_okay=False)],
    previous_version: Annotated[
        str | None, typer.Option(help="Default: newest published stable v* release.")
    ] = None,
    previous_bin: Annotated[
        Path | None,
        typer.Option(
            exists=True,
            dir_okay=False,
            help="Local A/B baseline instead of a published release.",
        ),
    ] = None,
    tolerance: Annotated[float, typer.Option(min=0.0)] = 10.0,
    runs: Annotated[int, typer.Option(min=15)] = 20,
    warmup: Annotated[int, typer.Option(min=1)] = 3,
    file_counts: str = "20,40,60,80,100",
    file_sizes_kib: str = "1,3,5,7,9",
    changelog: Path = Path("CHANGELOG.md"),
    release: Annotated[
        str | None,
        typer.Option(
            help="CHANGELOG section to read; defaults to the Cargo.toml version."
        ),
    ] = None,
    output: Path = Path("manual_outputs/benchmarks/release-perf.json"),
) -> None:
    """Gate README real-world lint/format cases with alternating A/B samples.

    Acquire the machine's benchmark lock before invoking; keep the JSON as evidence.

    Raises:
        typer.BadParameter: Tolerance is not finite.
        typer.Exit: Status 1 indicates an unaccepted regression.
    """
    if not math.isfinite(tolerance):
        raise typer.BadParameter("tolerance must be finite")
    harness.require_idle_host()
    counts = harness.parse_int_list(file_counts)
    sizes = harness.parse_int_list(file_sizes_kib)
    accepted = accepted_slowdowns(
        changelog=changelog.read_text(), release=release or _manifest_release()
    )
    with tempfile.TemporaryDirectory(prefix="ryl-release-perf-") as temporary:
        root = Path(temporary)
        if previous_bin is None:
            version = (
                previous_version
                or harness.run_checked(
                    [
                        "gh",
                        "release",
                        "list",
                        "--repo",
                        "owenlamont/ryl",
                        "--exclude-drafts",
                        "--exclude-pre-releases",
                        "--json",
                        "tagName",
                        "--jq",
                        '[.[] | select(.tagName | startswith("v"))][0].tagName',
                    ]
                ).stdout.strip()
            )
            previous_bin = published_binary(version=version, root=root)
        config = root / "format.toml"
        config.write_text("[format]\n")
        tools = (
            _tool(binary=previous_bin.resolve(), name="previous", config=config),
            _tool(binary=ryl_bin.resolve(), name="candidate", config=config),
        )
        for tool in tools:
            typer.echo(f"{tool.name}: {tool.version} ({tool.binary})", err=True)
        runner = harness.write_runner(root)
        typer.echo(
            "Selecting shared dirty files from the pinned real-world corpus...",
            err=True,
        )
        pool, pins, attempts = harness.real_world_pool(
            seed=7331,
            sizes=sizes,
            count=max(counts),
            root=root,
            tools=list(tools),
            runner=runner,
        )
        cases = harness.iter_cases(counts, sizes, root / "cases", "real-world")
        results: list[Result] = []
        typer.echo("| Case | Previous ms ± sd | Candidate ms ± sd | Delta | Verdict |")
        typer.echo("| --- | ---: | ---: | ---: | --- |")
        for case in cases:
            harness.materialize_real_case(case, pool)
            for mode in _Mode:
                result = _measure(
                    case=case,
                    mode=mode,
                    tools=tools,
                    runs=runs,
                    warmup=warmup,
                    root=root,
                )
                results.append(result)
                failing = regressions(
                    results=[result], tolerance=tolerance, accepted=accepted
                )
                noise = abs(
                    result.candidate_seconds - result.previous_seconds
                ) <= math.hypot(result.previous_sd, result.candidate_sd)
                verdict = "FAIL" if failing else "PASS"
                if result.case in accepted:
                    verdict += f"; accepted: {accepted[result.case]}"
                if noise:
                    verdict += "; within sample noise"
                typer.echo(
                    f"| {result.case} | {result.previous_seconds * 1000:.3f} "
                    f"± {result.previous_sd * 1000:.3f} | "
                    f"{result.candidate_seconds * 1000:.3f} "
                    f"± {result.candidate_sd * 1000:.3f} | "
                    f"{result.delta_percent:+.2f}% | {verdict} |"
                )
                output.parent.mkdir(parents=True, exist_ok=True)
                output.write_bytes(
                    orjson.dumps(
                        {
                            "versions": [tool.version for tool in tools],
                            "runs": runs,
                            "warmup": warmup,
                            "tolerance_percent": tolerance,
                            "release": release,
                            "accepted": accepted,
                            "corpus_pins": pins,
                            "corpus_attempts": attempts,
                            "results": [asdict(row) for row in results],
                        },
                        option=orjson.OPT_INDENT_2,
                    )
                    + b"\n"
                )
    failures = regressions(results=results, tolerance=tolerance, accepted=accepted)
    typer.echo(f"Gate: {'FAIL: ' + ', '.join(failures) if failures else 'PASS'}")
    raise typer.Exit(int(bool(failures)))


if __name__ == "__main__":
    app()
