# /// script
# requires-python = ">=3.15"
# dependencies = ["typer"]
# ///

from collections.abc import Callable, Iterable, Mapping
from dataclasses import dataclass
import itertools
import json
import os
from pathlib import Path
import subprocess
import time
from typing import Annotated, Any, Final

import typer


ROOT: Final = Path(__file__).resolve().parent.parent
REGRESSION_DIRS: Final = (
    "tests/proptest-regressions",
    "crates/ryl-yaml-parser/tests/proptest-regressions",
)
_PROPERTY_FILTER: Final = "test(/^property_/)"
_NEXTEST: Final = ("cargo", "nextest")
_NEXTEST_ARGS: Final = ("--workspace", "--cargo-quiet")
app = typer.Typer(
    add_completion=False, context_settings={"help_option_names": ["-h", "--help"]}
)


@dataclass(frozen=True, slots=True, order=True)
class Suite:
    binary_id: str
    module: str

    @property
    def filterset(self) -> str:
        """The nextest filterset selecting exactly this suite's tests."""
        return f"binary_id({self.binary_id}) & test(/^{self.module}::/)"


@dataclass(frozen=True, slots=True, kw_only=True)
class Run:
    suite: Suite
    seconds: float
    passed: bool
    output: str


def suites(nextest_list: Mapping[str, Any]) -> tuple[Suite, ...]:
    """Group the matching tests of `cargo nextest list --message-format json`.

    Returns:
        One suite per binary and top-level test module, sorted.
    """
    return tuple(
        sorted(
            {
                Suite(suite["binary-id"], name.split("::", 1)[0])
                for suite in nextest_list["rust-suites"].values()
                for name, case in suite["testcases"].items()
                if case["filter-match"]["status"] == "matches" and not case["ignored"]
            }
        )
    )


def soak(
    *,
    schedule: Iterable[Suite],
    deadline: float,
    run: Callable[[Suite], Run],
    clock: Callable[[], float] = time.monotonic,
) -> list[Run]:
    """Run suites round-robin until the deadline or the first failure.

    A run is only started before `deadline`; one in flight may finish after it.

    Returns:
        Every run made, the failing one last.
    """
    runs: list[Run] = []
    for suite in itertools.cycle(schedule):
        if clock() >= deadline:
            break
        runs.append(run(suite))
        if not runs[-1].passed:
            break
    return runs


def summary(runs: Iterable[Run], *, cases: int) -> str:
    """Render the per-suite Markdown table.

    Cases count `cases` per run for each property; a suite's deterministic tests
    add none.

    Returns:
        The table, one row per suite, sorted by binary and module.
    """
    rows = [
        "| Suite | Runs | Cases per property | Seconds | Cases/s |",
        "| --- | ---: | ---: | ---: | ---: |",
    ]
    by_suite = itertools.groupby(
        sorted(runs, key=lambda run: run.suite), key=lambda run: run.suite
    )
    for suite, group in by_suite:
        suite_runs = list(group)
        seconds = sum(run.seconds for run in suite_runs)
        total = cases * len(suite_runs)
        rows.append(
            f"| {suite.module} | {len(suite_runs)} | {total} | {seconds:.1f} "
            f"| {total / seconds:.1f} |"
        )
    return "\n".join(rows)


def child_env(environ: Mapping[str, str], *, cases: int) -> dict[str, str]:
    """Return `environ` set up for fresh random cases with failure persistence on.

    Returns:
        A copy with `PROPTEST_CASES` set and any inherited fixed seed or
        persistence opt-out removed.
    """
    env = {
        key: value
        for key, value in environ.items()
        if key not in {"PROPTEST_RNG_SEED", "PROPTEST_DISABLE_FAILURE_PERSISTENCE"}
    }
    env["PROPTEST_CASES"] = str(cases)
    return env


def _nextest(*args: str, capture: bool) -> str:
    return subprocess.run(
        [*_NEXTEST, *args, *_NEXTEST_ARGS, "-E", _PROPERTY_FILTER],
        cwd=ROOT,
        check=True,
        capture_output=capture,
        text=True,
    ).stdout


def _runner(env: Mapping[str, str]) -> Callable[[Suite], Run]:
    def run(suite: Suite) -> Run:
        start = time.monotonic()
        result = subprocess.run(
            [*_NEXTEST, "run", *_NEXTEST_ARGS, "-E", suite.filterset],
            cwd=ROOT,
            env=env,
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
        )
        return Run(
            suite=suite,
            seconds=time.monotonic() - start,
            passed=result.returncode == 0,
            output=result.stdout,
        )

    return run


def _git(*args: str) -> str:
    return subprocess.run(
        ["git", *args, "--", *REGRESSION_DIRS],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout


def _report_failure(run: Run) -> None:
    typer.echo(run.output)
    typer.echo(f"{run.suite.module} failed; seeds now in the regression files:")
    typer.echo(_git("status", "--porcelain", "--untracked-files=all"))
    typer.echo(_git("diff"))


@app.command()
def main(
    minutes: Annotated[
        float, typer.Option(min=0.0, help="Wall-clock budget, after the build.")
    ],
    cases: Annotated[
        int, typer.Option(min=1, help="PROPTEST_CASES for each suite run.")
    ] = 256,
) -> None:
    """Soak every property suite with fresh seeds until the budget runs out.

    Builds the test binaries before starting the clock, stops at the first
    failure keeping the seed proptest saved, and prints a per-suite table.

    Raises:
        Exit: With status 1 when a suite fails.
    """
    _nextest("run", "--no-run", capture=False)
    schedule = suites(
        json.loads(_nextest("list", "--message-format", "json", capture=True))
    )
    env = child_env(os.environ, cases=cases)
    deadline = time.monotonic() + minutes * 60.0
    run_suite = _runner(env)

    def logged(suite: Suite) -> Run:
        run = run_suite(suite)
        remaining = max(0.0, deadline - time.monotonic())
        status = "ok" if run.passed else "FAILED"
        typer.echo(
            f"{suite.module}: {status} in {run.seconds:.1f}s, "
            f"{remaining / 60.0:.1f} min left"
        )
        return run

    runs = soak(schedule=schedule, deadline=deadline, run=logged)
    typer.echo(summary(runs, cases=cases))
    if runs and not runs[-1].passed:
        _report_failure(runs[-1])
        raise typer.Exit(1)


if __name__ == "__main__":
    app()
