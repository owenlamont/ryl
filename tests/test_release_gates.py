# /// script
# requires-python = ">=3.15"
# dependencies = [
#   "pytest", "typer", "packaging", "py-yaml12", "pyyaml", "platformdirs",
#   "markdown-it-py", "matplotlib>=3.9,<4", "orjson>=3.11,<4",
#   "polars>=1.30,<2", "tqdm>=4.67,<5",
# ]
# ///

from pathlib import Path
import sys

import pytest


sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "scripts"))

from downstream_check import read_pin
from release_perf_gate import accepted_slowdowns, regressions, Result


@pytest.mark.parametrize(
    ("candidate", "expected_delta", "fails"),
    [
        pytest.param(0.8, -20.0, False, id="faster"),
        pytest.param(1.1, 10.0, False, id="threshold inclusive"),
        pytest.param(1.101, 10.1, True, id="over threshold"),
    ],
)
def test_performance_threshold(
    *, candidate: float, expected_delta: float, fails: bool
) -> None:
    result = Result(
        case="format/100x9KiB",
        previous_seconds=1.0,
        previous_sd=0.0,
        candidate_seconds=candidate,
        candidate_sd=0.0,
    )
    assert result.delta_percent == pytest.approx(expected_delta)
    assert regressions(results=[result], tolerance=10.0, accepted={}) == (
        [result.case] if fails else []
    )


def test_custom_tolerance_and_case_specific_override() -> None:
    result = Result(
        case="lint/20x1KiB",
        previous_seconds=1.0,
        previous_sd=0.0,
        candidate_seconds=1.2,
        candidate_sd=0.0,
    )
    assert regressions(results=[result], tolerance=25.0, accepted={}) == []
    assert (
        regressions(
            results=[result], tolerance=10.0, accepted={result.case: "accepted cost"}
        )
        == []
    )
    assert regressions(
        results=[result], tolerance=10.0, accepted={"format/20x1KiB": "different case"}
    ) == [result.case]


def test_acceptance_only_in_selected_release_bullets() -> None:
    changelog = """# Changelog
## Unreleased
- Performance: lint/20x1KiB: deliberate cost
- Performance: format/20x1KiB:

Performance: lint/40x1KiB: not a bullet

> - Performance: lint/80x1KiB: quoted example

- Example:
  - Performance: lint/100x1KiB: nested example

```text
- Performance: lint/60x1KiB: example only
```

## v0.25.1 - 2026-10-16
- Performance: format/100x9KiB: historical cost
"""
    assert accepted_slowdowns(changelog=changelog, release="Unreleased") == {
        "lint/20x1KiB": "deliberate cost"
    }
    assert accepted_slowdowns(changelog=changelog, release="v0.25.1") == {
        "format/100x9KiB": "historical cost"
    }
    with pytest.raises(ValueError, match="expected one changelog section"):
        accepted_slowdowns(changelog=changelog, release="v0.26.0")


@pytest.mark.parametrize(
    ("filename", "contents"),
    [
        pytest.param(
            "prek.toml",
            """[[repos]]
repo = "https://github.com/owenlamont/ryl-pre-commit.git"
rev = "v0.25.1"
hooks = [{ id = "ryl-check" }, { id = "ryl-format-markdown" }]
""",
            id="TOML",
        ),
        pytest.param(
            ".pre-commit-config.yaml",
            """repos:
  - repo: https://github.com/owenlamont/ryl-pre-commit
    rev: 'v0.25.1'  # release pin
    hooks:
      - id: ryl-check
      - id: ryl-format-markdown
""",
            id="YAML",
        ),
    ],
)
def test_release_pin_and_format_hook(
    *, filename: str, contents: str, tmp_path: Path
) -> None:
    config = tmp_path / filename
    config.write_text(contents)
    pin = read_pin(config)
    assert pin.version == "0.25.1"
    assert pin.hooks == ("ryl-check", "ryl-format-markdown")


@pytest.mark.parametrize(
    "contents",
    [
        pytest.param("repos = []", id="missing pin"),
        pytest.param(
            """[[repos]]
repo = "https://github.com/owenlamont/ryl-pre-commit"
rev = "main"
hooks = []
""",
            id="branch instead of release",
        ),
        pytest.param(
            """[[repos]]
repo = "https://github.com/owenlamont/ryl-pre-commit"
rev = "v0.25.1"
hooks = []
[[repos]]
repo = "https://github.com/owenlamont/ryl-pre-commit"
rev = "v0.25.0"
hooks = []
""",
            id="ambiguous pins",
        ),
    ],
)
def test_unusable_pin_fails(contents: str, tmp_path: Path) -> None:
    config = tmp_path / "prek.toml"
    config.write_text(contents)
    with pytest.raises(ValueError, match=r"expected exactly one|Invalid version"):
        read_pin(config)


if __name__ == "__main__":
    raise SystemExit(pytest.main([__file__, "-q"]))
