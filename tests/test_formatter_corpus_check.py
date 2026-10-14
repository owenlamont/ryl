# /// script
# requires-python = ">=3.15"
# dependencies = ["pytest", "typer", "py-yaml12", "pyyaml", "platformdirs"]
# ///
"""Gate logic of scripts/formatter_corpus_check.py, run with:

uv run tests/test_formatter_corpus_check.py
"""

import hashlib
from pathlib import Path
import sys
from typing import Final

import pytest
import typer
import yaml


sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "scripts"))

import formatter_corpus_check as gate


BEFORE: Final = b"a: x\n"
REVIEWED: Final = b"---\na: y\n"
CORRUPTED: Final = b"---\na: CORRUPTED\n"
PATH: Final = "f.yaml"


@pytest.mark.parametrize(
    ("indicator", "body", "expected"),
    [
        pytest.param("|", "", "", id="empty clip"),
        pytest.param("|+", "", "", id="empty keep"),
        pytest.param("|", "  x\n", "x\n", id="nonempty clip"),
        pytest.param("|+", "  x\n", "x\n", id="nonempty keep"),
    ],
)
@pytest.mark.parametrize("end_marker", [False, True], ids=["EOF", "document end"])
def test_document_start_before_block_scalar_keeps_yaml12_verdict(
    indicator: str, body: str, expected: str, end_marker: bool
) -> None:
    end = "...\n" if end_marker else ""
    source = f"k: {indicator}\n{body}{end}".encode()
    assert list(yaml.safe_load_all(source)) == [{"k": expected}]
    assert gate._compare12(source, b"---\n" + source) is gate._compare(
        gate._load12, source, b"---\n" + source
    )


@pytest.mark.parametrize(
    ("before", "after"),
    [
        pytest.param(b"k: |\n", b"k: |\n...\n", id="empty clip at EOF"),
        pytest.param(b"k: |+\n", b"k: |+\n...\n", id="empty keep at EOF"),
        pytest.param(b"--- |1+", b"--- |1+\n", id="explicit indent at EOF"),
        pytest.param(b"a: |\n  x\n  ", b"a: |\n  x\n", id="nonempty clip at EOF"),
    ],
)
def test_false_eof_value_difference_abstains(
    before: bytes, after: bytes, tmp_path: Path
) -> None:
    assert gate._compare(gate._load12, before, after) is gate.Loaded.UNEQUAL
    repo = gate.Repo(url="https://example.com/o/r", sha="0" * 40)
    result = gate.Result(repo=repo.name, mode=gate.Mode.DEFAULT)
    gate._record_change(result, repo, PATH, before, after, pairs=[], pair_dir=tmp_path)
    assert result.yaml12_unequal == []
    assert result.yaml12_unavailable == [PATH]


def test_changed_document_prefix_keeps_yaml12_verdict() -> None:
    before = b"a: |\n  x\n  "
    after = b"---\na: |\n  x\n"
    assert gate._compare(gate._load12, before, after) is gate.Loaded.UNEQUAL
    assert gate._compare12(before, after) is gate.Loaded.UNEQUAL


def test_real_non_eof_value_change_still_fails() -> None:
    before = b"k: x\n"
    after = b"k: y\n"
    assert gate._compare12(before, after) is gate.Loaded.UNEQUAL


@pytest.mark.parametrize(
    "suffix", [b"", b"# comment\n", b"...\n"], ids=["EOF", "comment", "document end"]
)
def test_earlier_value_change_before_final_block_scalar_fails(suffix: bytes) -> None:
    before = b"changed: 1\ntail: |\n  same\n" + suffix
    after = b"changed: 2\ntail: |\n  same\n" + suffix
    assert gate._compare(gate._load12, before, after) is gate.Loaded.UNEQUAL
    assert gate._compare12(before, after) is gate.Loaded.UNEQUAL


def test_yaml12_merge_key_value_change_still_fails() -> None:
    before = b"base: &b {a: x}\nout: {<<: *b}\n"
    after = b"base: &b {a: x}\nout: {a: x}\n"
    assert gate._compare(gate._load12, before, after) is gate.Loaded.UNEQUAL
    assert gate._compare12(before, after) is gate.Loaded.UNEQUAL


def test_yaml12_merge_key_change_with_eof_block_scalar_fails() -> None:
    before = b"base: &b {a: x}\nout: {<<: *b}\ntail: |\n"
    after = b"base: &b {a: x}\nout: {a: x}\ntail: |\n...\n"
    assert gate._compare12(before, after) is gate.Loaded.UNEQUAL


@pytest.mark.parametrize(
    ("before", "after"),
    [
        pytest.param(b"- |+\n   ", b"- |+\n...\n", id="empty keep"),
        pytest.param(b"a: |+\n  x\n  ", b"a: |+\n  x\n", id="nonempty keep"),
    ],
)
def test_blank_only_keep_scalar_value_change_does_not_pass(
    before: bytes, after: bytes
) -> None:
    assert gate._compare(gate._load12, before, after) is gate.Loaded.UNEQUAL
    assert gate._compare12(before, after) is not gate.Loaded.EQUAL


@pytest.mark.parametrize(
    ("before", "after"),
    [
        pytest.param(
            b'"": 1\n? |\n  x\n', b'"": 2\n? |\n  x\n', id="masked duplicate key"
        ),
        pytest.param(
            "k: |\n  x\u0085  y\n".encode(),
            b"k: |\n  x\n  y\n",
            id="NEL in block scalar",
        ),
    ],
)
def test_eof_review_cases_never_pass(before: bytes, after: bytes) -> None:
    assert gate._compare(gate._load12, before, after) is gate.Loaded.UNEQUAL
    assert gate._compare12(before, after) is not gate.Loaded.EQUAL


def test_omap_value_change_keeps_yaml12_value_verdict(tmp_path: Path) -> None:
    before = b"a: !!omap [{x: 1}, {x: 2}]\n"
    after = b"a: !!omap [{x: 1}, {x: 3}]\n"
    assert gate._compare(gate._load12, before, after) is gate.Loaded.UNEQUAL
    repo = gate.Repo(url="https://example.com/o/r", sha="0" * 40)
    result = gate.Result(repo=repo.name, mode=gate.Mode.DEFAULT)
    gate._record_change(result, repo, PATH, before, after, pairs=[], pair_dir=tmp_path)
    assert result.yaml12_unequal == [PATH]


def test_rust_oracle_value_loss_remains_a_hard_failure(tmp_path: Path) -> None:
    before = b"- |+\n   "
    after = b"- |+\n"
    repo = gate.Repo(url="https://example.com/o/r", sha="0" * 40)
    result = gate.Result(repo=repo.name, mode=gate.Mode.DEFAULT)
    pairs: list[tuple[str, Path]] = []
    gate._record_change(
        result, repo, PATH, before, after, pairs=pairs, pair_dir=tmp_path
    )
    gate._assign_verdicts([result], {pairs[0][0]: "value-preservation"})
    assert result.rust == {"value-preservation": [PATH]}
    assert gate._hard_failures(result) > 0


def _record(after: bytes, tmp_path: Path, before: bytes = BEFORE) -> gate.Result:
    reviewed = (
        PATH,
        hashlib.sha256(BEFORE).hexdigest(),
        hashlib.sha256(REVIEWED).hexdigest(),
    )
    repo = gate.Repo(
        url="https://example.com/o/r",
        sha="0" * 40,
        rust_known_errors=frozenset({reviewed}),
        yaml12_known_errors=frozenset({reviewed}),
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
    result = _record(REVIEWED, tmp_path, before=b"a: CORRUPTED\n")
    assert gate._hard_failures(result) == 4, result.errors


def test_each_hash_pair_must_be_hit_to_avoid_staleness(tmp_path: Path) -> None:
    other = b"---\na: z\n"
    entries = [
        {
            "path": PATH,
            "before-sha256": hashlib.sha256(BEFORE).hexdigest(),
            "after-sha256": hashlib.sha256(after).hexdigest(),
        }
        for after in (REVIEWED, other)
    ]
    repo = gate.Repo(
        url="https://example.com/o/r",
        sha="0" * 40,
        yaml12_known_errors=gate._known(entries, gate.Profile.STANDARD),
    )
    result = gate.Result(repo=repo.name, mode=gate.Mode.DEFAULT)
    gate._record_change(
        result, repo, PATH, BEFORE, REVIEWED, pairs=[], pair_dir=tmp_path
    )
    assert len(result.yaml12_known_errors) == 1
    assert len(gate._stale_known_errors([repo], [result])) == 1
    second = gate.Result(repo=repo.name, mode=gate.Mode.FOLD)
    gate._record_change(second, repo, PATH, BEFORE, other, pairs=[], pair_dir=tmp_path)
    assert gate._stale_known_errors([repo], [result, second]) == []


def test_known_errors_are_scoped_to_the_corpus_profile() -> None:
    entries = [
        {"path": PATH, "before-sha256": "a" * 64, "after-sha256": "b" * 64},
        {
            "profile": "block-crlf",
            "path": PATH,
            "before-sha256": "a" * 64,
            "after-sha256": "c" * 64,
        },
    ]
    assert len(gate._known(entries, gate.Profile.STANDARD)) == 1
    assert len(gate._known(entries, gate.Profile.BLOCK_CRLF)) == 1
    assert gate._known(entries, gate.Profile.STANDARD) != gate._known(
        entries, gate.Profile.BLOCK_CRLF
    )


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
