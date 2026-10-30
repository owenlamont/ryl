# /// script
# requires-python = ">=3.15"
# dependencies = ["pytest", "typer", "py-yaml12", "pyyaml", "platformdirs"]
# ///
import os
from pathlib import Path
import sys

import pytest


sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "scripts"))

import formatter_output_parity as gate


@pytest.fixture
def binary_factory(tmp_path: Path):
    if os.name == "nt":
        pytest.skip("fixture executables use POSIX shebangs")

    def make(name: str, replacement: bytes, notice: str = "", status: int = 0) -> Path:
        binary = tmp_path / name
        binary.write_text(
            "#!/usr/bin/env python3\n"
            "from pathlib import Path\n"
            "import sys\n"
            "assert '--no-cache' in sys.argv\n"
            "path = Path('a.yaml')\n"
            f"path.write_bytes({replacement!r})\n"
            f"sys.stderr.write({notice!r})\n"
            f"sys.exit({status})\n",
            encoding="utf-8",
        )
        binary.chmod(0o755)
        return binary

    return make


def test_output_byte_difference_is_reported_and_input_restored(
    tmp_path: Path, binary_factory
) -> None:
    source = tmp_path / "a.yaml"
    source.write_bytes(b"a: 1\n")
    baseline = binary_factory("baseline", b"a: 1\n")
    candidate = binary_factory("candidate", b"a: 2\n")

    differences = gate._case(
        baseline, candidate, tmp_path, [source.name], [], 10, "fixture/a.yaml"
    )

    assert differences == [
        "fixture/a.yaml: a.yaml: differs at byte 3: b'1\\n' vs b'2\\n'"
    ]
    assert source.read_bytes() == b"a: 1\n"


def test_notice_difference_is_reported(tmp_path: Path, binary_factory) -> None:
    source = tmp_path / "a.yaml"
    source.write_bytes(b"a: 1\n")
    baseline = binary_factory("baseline", b"a: 1\n")
    candidate = binary_factory("candidate", b"a: 1\n", "refused\n")

    differences = gate._case(
        baseline, candidate, tmp_path, [source.name], [], 10, "fixture/a.yaml"
    )

    assert differences == [
        "fixture/a.yaml: stderr differs at byte 0: b'' vs b'refused\\n'"
    ]


def test_exit_status_difference_is_reported(tmp_path: Path, binary_factory) -> None:
    source = tmp_path / "a.yaml"
    source.write_bytes(b"a: 1\n")
    baseline = binary_factory("baseline", b"a: 1\n")
    candidate = binary_factory("candidate", b"a: 1\n", status=1)

    differences = gate._case(
        baseline, candidate, tmp_path, [source.name], [], 10, "fixture/a.yaml"
    )

    assert differences == ["fixture/a.yaml: status differs: 0 vs 1"]


def test_matching_failed_formatter_runs_do_not_pass(
    tmp_path: Path, binary_factory
) -> None:
    source = tmp_path / "a.yaml"
    source.write_bytes(b"a: 1\n")
    baseline = binary_factory("baseline", b"a: 1\n", status=2)
    candidate = binary_factory("candidate", b"a: 1\n", status=2)

    differences = gate._case(
        baseline, candidate, tmp_path, [source.name], [], 10, "fixture/a.yaml"
    )

    assert differences == ["fixture/a.yaml: baseline formatter exited 2"]


def test_expected_refusal_notice_must_appear(tmp_path: Path, binary_factory) -> None:
    source = tmp_path / "a.yaml"
    source.write_bytes(b"a: 1\n")
    silent = binary_factory("silent", b"a: 1\n")
    notice = gate._REFUSAL_NOTICE
    reported = binary_factory("reported", b"a: 1\n", f"{notice.decode()}\n")

    missing = gate._case(
        silent,
        silent,
        tmp_path,
        [source.name],
        [],
        10,
        "fixture/a.yaml",
        expected_notice=notice,
    )
    assert missing == [f"fixture/a.yaml: expected notice missing: {notice!r}"]

    present = gate._case(
        reported,
        reported,
        tmp_path,
        [source.name],
        [],
        10,
        "fixture/a.yaml",
        expected_notice=notice,
    )
    assert present == []


if __name__ == "__main__":
    raise SystemExit(pytest.main([__file__, "-q"]))
