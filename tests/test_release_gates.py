# /// script
# requires-python = ">=3.15"
# dependencies = [
#   "pytest", "pytest-mock", "typer", "packaging", "py-yaml12", "pyyaml",
#   "platformdirs",
#   "markdown-it-py", "matplotlib>=3.9,<4", "orjson>=3.11,<4",
# ]
# ///

from pathlib import Path
import sys
from typing import TYPE_CHECKING

import pytest


if TYPE_CHECKING:
    from pytest_mock import MockerFixture


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
    "ref",
    [
        pytest.param("refs/tags/v0.25.1", id="lightweight tag"),
        pytest.param("refs/tags/v0.25.1^{}", id="peeled annotated tag"),
    ],
)
@pytest.mark.parametrize(
    ("filename", "contents"),
    [
        pytest.param(
            "prek.toml",
            """[[repos]]
repo = "https://github.com/owenlamont/ryl-pre-commit"
rev = "44a7653dd5d871b2b40224d504f18b5d5d1adc44" # frozen: v0.25.1
hooks = [{ id = "ryl-check" }, { id = "ryl-format" }]
""",
            id="TOML",
        ),
        pytest.param(
            ".pre-commit-config.yaml",
            """repos:
  - repo: https://github.com/owenlamont/ryl-pre-commit
    rev: 44a7653dd5d871b2b40224d504f18b5d5d1adc44 # frozen: v0.25.1
    hooks: [{id: ryl-check}, {id: ryl-format}]
""",
            id="YAML",
        ),
    ],
)
def test_frozen_release_pin(
    *, filename: str, contents: str, ref: str, tmp_path: Path, mocker: MockerFixture
) -> None:
    config = tmp_path / filename
    config.write_text(contents)
    request = mocker.patch("subprocess.run", autospec=True)
    request.return_value.stdout = f"44a7653dd5d871b2b40224d504f18b5d5d1adc44\t{ref}\n"
    pin = read_pin(config)
    assert pin.version == "0.25.1"
    assert pin.hooks == ("ryl-check", "ryl-format")
    request.assert_called_once_with(
        [
            "git",
            "ls-remote",
            "--tags",
            "--",
            "https://github.com/owenlamont/ryl-pre-commit",
            "v[0-9]*",
        ],
        capture_output=True,
        text=True,
        check=True,
        timeout=60,
    )


def test_frozen_revision_without_release_tag_fails(
    tmp_path: Path, mocker: MockerFixture
) -> None:
    config = tmp_path / "prek.toml"
    config.write_text("""[[repos]]
repo = "https://github.com/owenlamont/ryl-pre-commit"
rev = "44a7653dd5d871b2b40224d504f18b5d5d1adc44"
hooks = []
""")
    request = mocker.patch("subprocess.run", autospec=True)
    request.return_value.stdout = ""
    with pytest.raises(ValueError, match="has no release tag"):
        read_pin(config)


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
