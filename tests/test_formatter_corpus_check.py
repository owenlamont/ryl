# /// script
# requires-python = ">=3.14"
# dependencies = ["pytest", "typer", "py-yaml12", "pyyaml", "platformdirs"]
# ///
"""Gate logic of scripts/formatter_corpus_check.py, run with:

uv run tests/test_formatter_corpus_check.py
"""

import hashlib
from pathlib import Path
import sys

import pytest
import typer


sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "scripts"))

import formatter_corpus_check as gate


# granit and py-yaml12 read this original as "x\n\n"; the reference parser says "x\n".
BEFORE = b"a: |\n  x\n  "
REVIEWED = b"---\na: |\n  x\n"
CORRUPTED = b"---\na: |\n  CORRUPTED\n"
PATH = "f.yaml"


def _record(after: bytes, tmp_path: Path, before: bytes = BEFORE) -> gate.Result:
    digest = (hashlib.sha256(BEFORE).hexdigest(), hashlib.sha256(REVIEWED).hexdigest())
    repo = gate.Repo(
        url="https://example.com/o/r",
        sha="0" * 40,
        rust_known_errors={PATH: digest},
        yaml12_known_errors={PATH: digest},
    )
    result = gate.Result(repo=repo.name, mode=gate.Mode.DEFAULT)
    gate._record_change(result, repo, PATH, before, after, pairs=[], pair_dir=tmp_path)
    gate._assign_verdicts(
        [result], {f"{result.mode}\t{result.repo}\t{PATH}": "value-preservation"}
    )
    stale = gate._stale_known_errors([repo], [result])
    result.errors.extend(stale)
    return result


def test_a_known_error_is_waived_for_its_reviewed_output(tmp_path: Path) -> None:
    assert gate._hard_failures(_record(REVIEWED, tmp_path)) == 0


def test_corrupting_an_allow_listed_file_fails_the_gate(tmp_path: Path) -> None:
    result = _record(CORRUPTED, tmp_path)
    assert result.yaml12_unequal == [PATH]
    assert result.rust == {"value-preservation": [PATH]}
    assert gate._hard_failures(result) == 4, result.errors


def test_corrupting_the_input_of_an_allow_listed_file_fails_the_gate(
    tmp_path: Path,
) -> None:
    result = _record(REVIEWED, tmp_path, before=b"a: |\n  CORRUPTED\n  ")
    assert gate._hard_failures(result) == 4, result.errors


def test_a_dirty_cached_checkout_is_refused(tmp_path: Path) -> None:
    clone = tmp_path / "o__r"
    clone.mkdir()
    gate._git(clone, "init", "-q")
    (clone / "f.yaml").write_text("a: 1\n")
    gate._git(clone, "add", ".")
    gate._git(clone, "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "x")
    (clone / "f.yaml").write_text("a: 2\n")
    repo = gate.Repo(
        url="https://example.com/o/r", sha=gate._git(clone, "rev-parse", "HEAD")
    )
    with pytest.raises(gate.CorpusError, match="local changes"):
        gate._fetch(repo, tmp_path)


def test_only_the_marked_file_beside_the_marker_is_expected(tmp_path: Path) -> None:
    (tmp_path / "bad").mkdir()
    (tmp_path / "bad" / "error").touch()
    repo = gate.Repo(
        url="https://example.com/o/r",
        sha="0" * 40,
        expected_skip_marker={"file": "in.yaml", "marker": "error"},
    )
    skips = ("bad/in.yaml", "bad/out.yaml", "good/in.yaml")
    stderr = "".join(f"{f}:1:1 skipped by ryl format: x\n" for f in skips)
    assert gate._unexpected_skips(repo, tmp_path, stderr) == (
        sorted(skips),
        ["bad/out.yaml", "good/in.yaml"],
    )


def test_an_unknown_repo_selector_is_rejected() -> None:
    with pytest.raises(typer.BadParameter, match="nope/nope"):
        gate._manifest(["nope/nope"])


@pytest.mark.skipif(not gate._RYL.is_file(), reason="needs cargo build --release")
def test_a_ryl_usage_error_is_a_hard_failure(tmp_path: Path) -> None:
    result = gate.Result(repo="o/r", mode=gate.Mode.DEFAULT)
    gate._ryl(tmp_path, result, "check", "--no-such-flag", timeout_s=60.0)
    assert gate._hard_failures(result) == 1, result.errors


if __name__ == "__main__":
    sys.exit(pytest.main([__file__, "-q", "-p", "no:cacheprovider"]))
