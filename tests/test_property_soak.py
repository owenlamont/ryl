# /// script
# requires-python = ">=3.15"
# dependencies = ["pytest", "typer"]
# ///

from collections.abc import Callable
import itertools
from pathlib import Path
import sys
from typing import Any, Final

import pytest


sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "scripts"))

from property_soak import child_env, Run, soak, Suite, suites, summary


CHECK: Final = Suite("ryl::integration", "property_check")
FORMAT: Final = Suite("ryl::integration", "property_format")
PARSER: Final = Suite("ryl-yaml-parser::integration", "property_parser")


def _case(*, status: str = "matches", ignored: bool = False) -> dict[str, Any]:
    return {"kind": "test", "ignored": ignored, "filter-match": {"status": status}}


def _passing(suite: Suite) -> Run:
    return Run(suite=suite, seconds=1.0, passed=True, output="")


@pytest.fixture
def ticking_clock() -> Callable[[], float]:
    return itertools.count().__next__


def test_suites_group_matching_tests_by_binary_and_module() -> None:
    nextest_list = {
        "rust-suites": {
            "ryl::integration": {
                "binary-id": "ryl::integration",
                "testcases": {
                    "property_format::idempotent": _case(),
                    "property_check::spans": _case(),
                    "property_check::never_panics": _case(),
                    "property_diff::slow": _case(ignored=True),
                    "cli_fix::fixes": _case(status="mismatch"),
                },
            },
            "ryl-yaml-parser::integration": {
                "binary-id": "ryl-yaml-parser::integration",
                "testcases": {"property_parser::agree": _case()},
            },
        }
    }
    assert suites(nextest_list) == (PARSER, CHECK, FORMAT)


def test_filterset_selects_one_module_in_one_binary() -> None:
    assert CHECK.filterset == (
        "binary_id(ryl::integration) & test(/^property_check::/)"
    )


def test_soak_cycles_suites_until_the_deadline(
    ticking_clock: Callable[[], float],
) -> None:
    runs = soak(
        schedule=(CHECK, FORMAT), deadline=5.0, run=_passing, clock=ticking_clock
    )
    assert [run.suite for run in runs] == [CHECK, FORMAT, CHECK, FORMAT, CHECK]


def test_soak_stops_at_the_first_failure(ticking_clock: Callable[[], float]) -> None:
    failure = Run(suite=FORMAT, seconds=2.0, passed=False, output="seed")
    runs = soak(
        schedule=(CHECK, FORMAT),
        deadline=100.0,
        run=lambda suite: failure if suite == FORMAT else _passing(suite),
        clock=ticking_clock,
    )
    assert runs == [_passing(CHECK), failure]


def test_summary_totals_cases_and_rate_per_suite() -> None:
    runs = [
        Run(suite=FORMAT, seconds=30.0, passed=True, output=""),
        Run(suite=CHECK, seconds=2.0, passed=True, output=""),
        Run(suite=FORMAT, seconds=34.0, passed=True, output=""),
    ]
    assert summary(runs, cases=64).splitlines()[2:] == [
        "| property_check | 1 | 64 | 2.0 | 32.0 |",
        "| property_format | 2 | 128 | 64.0 | 2.0 |",
    ]


def test_child_env_draws_fresh_seeds_and_keeps_failure_persistence() -> None:
    environ = {
        "PATH": "/bin",
        "PROPTEST_CASES": "0",
        "PROPTEST_RNG_SEED": "8",
        "PROPTEST_DISABLE_FAILURE_PERSISTENCE": "1",
    }
    assert child_env(environ, cases=256) == {"PATH": "/bin", "PROPTEST_CASES": "256"}


if __name__ == "__main__":
    raise SystemExit(pytest.main([__file__, "-q"]))
