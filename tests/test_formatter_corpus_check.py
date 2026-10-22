# /// script
# requires-python = ">=3.15"
# dependencies = ["pytest", "typer", "py-yaml12", "pyyaml", "platformdirs"]
# ///
"""Gate logic of scripts/formatter_corpus_check.py, run with:

uv run --with pytest --with typer --with py-yaml12 --with pyyaml --with platformdirs
pytest tests/test_formatter_corpus_check.py
"""

from datetime import date, datetime, UTC
import hashlib
import json
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
def test_false_eof_value_difference_follows_reference(
    before: bytes, after: bytes, tmp_path: Path
) -> None:
    assert gate._compare(gate._load12, before, after) is gate.Loaded.UNEQUAL
    repo = gate.Repo(url="https://example.com/o/r", sha="0" * 40)
    result = gate.Result(repo=repo.name, mode=gate.Mode.DEFAULT)
    gate._record_change(result, repo, PATH, before, after, pairs=[], pair_dir=tmp_path)
    assert result.yaml12_unequal == []
    assert result.yaml12_unavailable == []
    assert gate._hard_failures(result) == 0


def test_changed_document_prefix_follows_reference_verdict() -> None:
    before = b"a: |\n  x\n  "
    after = b"---\na: |\n  x\n"
    assert gate._compare(gate._load12, before, after) is gate.Loaded.UNEQUAL
    assert gate._compare12(before, after) is gate.Loaded.EQUAL


def test_unwaived_py_yaml12_disagreement_is_adjudicated_by_reference(
    tmp_path: Path,
) -> None:
    before = b"a: |\n  x\n  "
    after = b"a: |\n  x\n"
    assert gate._compare(gate._load12, before, after) is gate.Loaded.UNEQUAL
    repo = gate.Repo(url="https://example.com/o/r", sha="0" * 40)
    result = gate.Result(repo=repo.name, mode=gate.Mode.DEFAULT)
    gate._record_change(result, repo, PATH, before, after, pairs=[], pair_dir=tmp_path)
    assert result.yaml12_unequal == []
    assert gate._hard_failures(result) == 0


def test_py_yaml12_equal_eof_block_scalar_change_fails_reference_gate(
    tmp_path: Path,
) -> None:
    before = b"a: |+\n  "
    after = b"a: |+\n  \n...\n"
    assert gate._compare(gate._load12, before, after) is gate.Loaded.EQUAL
    repo = gate.Repo(url="https://example.com/o/r", sha="0" * 40)
    result = gate.Result(repo=repo.name, mode=gate.Mode.DEFAULT)
    gate._record_change(result, repo, PATH, before, after, pairs=[], pair_dir=tmp_path)
    assert result.yaml12_unequal == [PATH]
    assert gate._hard_failures(result) == 1


@pytest.mark.parametrize(
    "line_break", [pytest.param(b"\r\n", id="CRLF"), pytest.param(b"\r", id="bare CR")]
)
def test_cr_eof_block_scalar_change_is_a_hard_failure(
    line_break: bytes, tmp_path: Path
) -> None:
    before = b"a: |" + line_break + b"  "
    after = b"a: |" + line_break + b"  " + line_break + b"..." + line_break
    assert gate._compare(gate._load12, before, after) is gate.Loaded.UNEQUAL
    assert gate.reference_equal(before.decode(), after.decode()) is False
    repo = gate.Repo(url="https://example.com/o/r", sha="0" * 40)
    result = gate.Result(repo=repo.name, mode=gate.Mode.DEFAULT)
    gate._record_change(result, repo, PATH, before, after, pairs=[], pair_dir=tmp_path)
    assert result.yaml12_unequal == [PATH]
    assert result.yaml12_unavailable == []
    assert gate._hard_failures(result) == 1


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


@pytest.mark.parametrize(
    ("verdict", "hard_failures"),
    [("value-preservation", 0), ("comment/anchor fidelity", 1)],
)
def test_pinned_rust_pair_waives_only_value_preservation(
    verdict: str, hard_failures: int, tmp_path: Path
) -> None:
    after = b"---\na: x\n"
    reviewed = (
        PATH,
        hashlib.sha256(BEFORE).hexdigest(),
        hashlib.sha256(after).hexdigest(),
    )
    repo = gate.Repo(
        url="https://example.com/o/r",
        sha="0" * 40,
        rust_known_errors=frozenset({reviewed}),
    )
    result = gate.Result(repo=repo.name, mode=gate.Mode.DEFAULT)
    gate._record_change(result, repo, PATH, BEFORE, after, pairs=[], pair_dir=tmp_path)
    gate._assign_verdicts([result], {f"{result.mode}\t{result.repo}\t{PATH}": verdict})
    assert result.rust_known_errors == ([reviewed] if not hard_failures else [])
    assert result.rust == ({verdict: [PATH]} if hard_failures else {})
    assert gate._hard_failures(result) == hard_failures


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


def test_manifest_selects_only_the_active_profile_waivers() -> None:
    name = "johnkoht/hassio-config"
    standard = gate._manifest([name], gate.Profile.STANDARD)[0]
    flow = gate._manifest([name], gate.Profile.FLOW_NARROW)[0]
    assert len(standard.yaml12_known_errors) == 1
    assert len(flow.yaml12_known_errors) == 1
    assert standard.yaml12_known_errors.isdisjoint(flow.yaml12_known_errors)
    result = gate.Result(
        repo=name,
        mode=gate.Mode.DEFAULT,
        yaml12_known_errors=list(standard.yaml12_known_errors),
    )
    assert gate._stale_known_errors([standard], [result]) == []


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


def test_markdown_region_verdicts_keep_host_and_value_failures(tmp_path: Path) -> None:
    repo = gate.Repo(url="https://example.com/o/r", sha="0" * 40)
    result = gate.Result(repo=repo.name, mode=gate.Mode.DEFAULT)
    before = tmp_path / "0.before"
    after = tmp_path / "0.after"
    before.write_bytes(b"a: 1\n")
    after.write_bytes(b"a: 2\n")
    rows = [
        {
            "file": 0,
            "region": 0,
            "kind": "FrontMatter",
            "before": str(before),
            "after": str(after),
        },
        {"file": 0, "host_equal": False, "regions": 1},
    ]
    manifest = tmp_path / "regions.json"
    manifest.write_text(json.dumps(rows))
    pairs: list[tuple[str, Path]] = []
    gate._record_markdown_regions(
        repo, result, ["doc.md"], manifest, pairs=pairs, pair_dir=tmp_path
    )
    assert result.regions == 1
    assert result.changed_regions == 1
    assert result.front_matter == 1
    assert result.fences == 0
    assert result.host_changed == ["doc.md"]
    assert result.yaml12_unequal == ["doc.md#front-matter-1"]
    assert pairs[0][0] == ("default\to/r\tdoc.md#front-matter-1")
    assert gate._hard_failures(result) >= 2


def test_markdown_region_count_mismatch_is_a_hard_failure(tmp_path: Path) -> None:
    repo = gate.Repo(url="https://example.com/o/r", sha="0" * 40)
    result = gate.Result(repo=repo.name, mode=gate.Mode.DEFAULT)
    manifest = tmp_path / "regions.json"
    manifest.write_text('[{"file": 0, "host_equal": false, "regions": 2}]')
    gate._record_markdown_regions(
        repo, result, ["doc.md"], manifest, pairs=[], pair_dir=tmp_path
    )
    assert result.errors


def test_markdown_oracle_verdicts_do_not_cross_operations(tmp_path: Path) -> None:
    repo = gate.Repo(url="https://example.com/o/r", sha="0" * 40)
    pairs: list[tuple[str, Path]] = []
    formatted = gate.Result(
        repo=repo.name, mode=gate.Mode.FOLD, operation="markdown-format"
    )
    fixed = gate.Result(repo=repo.name, mode=gate.Mode.FOLD, operation="markdown-fix")
    gate._record_change(
        formatted,
        repo,
        "doc.md#front-matter-1",
        BEFORE,
        REVIEWED,
        pairs=pairs,
        pair_dir=tmp_path,
    )
    gate._record_change(
        fixed,
        repo,
        "doc.md#front-matter-1",
        BEFORE,
        CORRUPTED,
        pairs=pairs,
        pair_dir=tmp_path,
    )
    gate._assign_verdicts(
        [formatted, fixed], {pairs[0][0]: "ok", pairs[1][0]: "value-preservation"}
    )
    assert formatted.rust == {"ok": ["doc.md#front-matter-1"]}
    assert fixed.rust == {"value-preservation": ["doc.md#front-matter-1"]}


def test_markdown_sparse_patterns_are_opt_in() -> None:
    old = gate._manifest(["kubernetes/examples"])[0]
    yaml_mkdocs = gate._manifest(["squidfunk/mkdocs-material"])[0]
    new = gate._manifest(["squidfunk/mkdocs-material"], markdown=True)[0]
    assert old.markdown_sparse == ()
    assert yaml_mkdocs.sha == "6d3dc570d51064a3f55d189bd22c2390b07d46fe"
    assert yaml_mkdocs.markdown_sparse == ()
    assert new.sha == "a0f2a2a70ee978c2b0afc80347fed6eb4cec7867"
    assert new.markdown_sparse
    assert len(gate._manifest(())) == 40
    assert len(gate._manifest((), markdown=True)) == 6


def test_markdown_files_include_non_ascii_tracked_names(tmp_path: Path) -> None:
    gate._git(tmp_path, "init", "-q")
    (tmp_path / "français.md").write_text("---\na: 1\n---\n")
    (tmp_path / "plain.md").write_text("---\na: 1\n---\n")
    (tmp_path / " leading.md").write_text("---\na: 1\n---\n")
    gate._git(tmp_path, "add", ".")
    repo = gate.Repo(
        url="https://example.com/o/r", sha="0" * 40, markdown_sparse=("*.md",)
    )
    assert gate._markdown_files(repo, tmp_path) == [
        " leading.md",
        "français.md",
        "plain.md",
    ]


@pytest.mark.parametrize("operation", ["markdown-format", "markdown-fix"])
@pytest.mark.parametrize("mode", list(gate.Mode))
def test_noop_markdown_operation_fails_positive_control(
    operation: str, mode: gate.Mode, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    clone = tmp_path / "clone"
    clone.mkdir()
    gate._git(clone, "init", "-q")
    source = clone / "doc.md"
    source.write_text("---\na:  1\n---\n")
    gate._git(clone, "add", ".")
    gate._git(clone, "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "x")
    repo = gate.Repo(
        url="https://example.com/o/r", sha="0" * 40, markdown_sparse=("*.md",)
    )

    def noop_ryl(
        _clone: Path, _result: gate.Result, *args: str, **_kwargs: object
    ) -> gate.subprocess.CompletedProcess[str]:
        return gate.subprocess.CompletedProcess(args, 0, "", "")

    def unchanged_region(_stems: list[Path], work: Path) -> Path:
        before = work / "region.before"
        after = work / "region.after"
        before.write_bytes(b"a:  1\n")
        after.write_bytes(b"a:  1\n")
        manifest = work / "regions.json"
        manifest.write_text(
            json.dumps(
                [
                    {
                        "file": 0,
                        "region": 0,
                        "kind": "FrontMatter",
                        "before": str(before),
                        "after": str(after),
                    },
                    {"file": 0, "host_equal": True, "regions": 1},
                ]
            )
        )
        return manifest

    monkeypatch.setattr(gate, "_ryl", noop_ryl)
    monkeypatch.setattr(gate, "_markdown_regions", unchanged_region)
    pair_dir = tmp_path / "pairs"
    pair_dir.mkdir()
    result = gate._run_markdown(
        repo, clone, mode, operation, pairs=[], pair_dir=pair_dir, timeout_s=1
    )
    assert result.files == 1
    assert result.regions == 1
    assert result.changed_regions == 0
    assert result.errors == [f"{operation}: no YAML regions changed"]
    assert gate._hard_failures(result) == 1


@pytest.mark.parametrize("operation", ["markdown-format", "markdown-fix"])
@pytest.mark.parametrize("mode", list(gate.Mode))
def test_markdown_run_uses_profile_and_detects_second_edit(
    operation: str, mode: gate.Mode, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    clone = tmp_path / "clone"
    clone.mkdir()
    gate._git(clone, "init", "-q")
    source = clone / "doc.md"
    source.write_text("---\na: 1\n---\n")
    gate._git(clone, "add", ".")
    gate._git(clone, "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "x")
    repo = gate.Repo(
        url="https://example.com/o/r", sha="0" * 40, markdown_sparse=("*.md",)
    )
    calls: list[tuple[str, ...]] = []

    def fake_ryl(
        _clone: Path, _result: gate.Result, *args: str, **_kwargs: object
    ) -> gate.subprocess.CompletedProcess[str]:
        calls.append(args)
        source.write_text(source.read_text() + "x")
        return gate.subprocess.CompletedProcess(args, 0, "", "")

    def fake_regions(_stems: list[Path], work: Path) -> Path:
        manifest = work / "regions.json"
        manifest.write_text('[{"file": 0, "host_equal": false, "regions": 0}]')
        return manifest

    monkeypatch.setattr(gate, "_ryl", fake_ryl)
    monkeypatch.setattr(gate, "_markdown_regions", fake_regions)
    pair_dir = tmp_path / "pairs"
    pair_dir.mkdir()
    result = gate._run_markdown(
        repo, clone, mode, operation, pairs=[], pair_dir=pair_dir, timeout_s=1
    )
    assert len(calls) == 2
    assert all("--markdown" in call for call in calls)
    config = (
        ("-d", "extends: default")
        if mode is gate.Mode.DEFAULT
        else ("-c", str(tmp_path / "fold.toml"))
    )
    assert all(call[-3:] == (*config, "doc.md") for call in calls)
    assert calls[0][0] == ("format" if operation == "markdown-format" else "check")
    if operation == "markdown-fix" and mode is gate.Mode.FOLD:
        assert calls[0][2:4] == ("--enable", gate._DEFAULT_LINT_RULES)
    else:
        assert "--enable" not in calls[0]
    assert result.not_idempotent == ["doc.md"]
    assert result.host_changed == ["doc.md"]
    assert source.read_text() == "---\na: 1\n---\n"


if __name__ == "__main__":
    sys.exit(pytest.main([__file__, "-q", "-p", "no:cacheprovider"]))


def test_style_only_change_before_an_eof_block_scalar_is_equal() -> None:
    before = b'a: "128Mi"\nb: {c: [1]}\nd: |\n  t\n'
    after = b"a: 128Mi\nb:\n  c:\n    - 1\nd: |\n  t\n"
    assert gate._compare(gate._load12, before, after) is gate.Loaded.EQUAL
    assert gate.reference_equal(before.decode(), after.decode()) is False
    assert gate._compare12(before, after) is gate.Loaded.EQUAL


def test_quoted_to_typed_scalar_before_an_eof_block_scalar_still_fails() -> None:
    before = b'a: "128"\nd: |\n  t\n'
    after = b"a: 128\nd: |\n  t\n"
    assert gate._compare12(before, after) is gate.Loaded.UNEQUAL


def test_reference_events_keep_output_past_64_kib(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    parser = tmp_path / "yaml-parser"
    script = 'process.stdout.write("=VAL :x\\n".repeat(20000)); process.exit(0);'
    parser.write_text(script)
    events = sys.modules["reference_events"]
    monkeypatch.setattr(events, "_PARSER", parser)
    assert len(events._events("a: 1\n")) == 20000


OLD_A: Final = "a" * 40
OLD_B: Final = "b" * 40
OLD_C: Final = "c" * 40
OLD_D: Final = "d" * 40
NEW_A: Final = "1" * 40
NEW_B: Final = "2" * 40
WAIVED_PATH: Final = "w.yaml"
WAIVED_SHA256: Final = "e" * 64
NOW: Final = datetime(2026, 10, 22, tzinfo=UTC)
TODAY: Final = date(2026, 10, 23)
MANIFEST: Final = f"""# comment kept
tested-date = 2026-01-01

[[repo]]
url = "https://github.com/o/a"
sha = "{OLD_A}"  # trailing
note = "x"

[[repo.yaml12-known-errors]]
path = "{WAIVED_PATH}"
before-sha256 = "{WAIVED_SHA256}"
after-sha256 = "{"f" * 64}"

[[repo]]
url = "https://github.com/o/held"
sha = "{OLD_C}"
refresh = false

[[repo]]
url = "https://github.com/o/b"
sha = "{OLD_B}"

[[markdown-repo]]
url = "https://github.com/o/b"
sha = "{OLD_D}"
"""


class FakeRemote:
    def head(self, pin: gate.Pin) -> str:
        return {"o/a": NEW_A, "o/b": NEW_B}[pin.name]

    def committed(self, pin: gate.Pin, sha: str) -> datetime:
        return datetime(2026, 1, 1, tzinfo=UTC) if sha == OLD_A else NOW

    def sha256(self, pin: gate.Pin, sha: str, path: str) -> str | None:
        return None


@pytest.fixture
def manifest(tmp_path: Path) -> Path:
    path = tmp_path / "corpus.toml"
    path.write_bytes(MANIFEST.encode())
    return path


def test_refresh_rewrites_only_the_sha_lines(manifest: Path) -> None:
    gate.refresh_manifest(
        manifest, names=(), dry_run=False, remote=FakeRemote(), today=TODAY
    )
    expected = (
        MANIFEST.replace(OLD_A, NEW_A)
        .replace(OLD_B, NEW_B)
        .replace(OLD_D, NEW_B)
        .replace("tested-date = 2026-01-01", f"tested-date = {TODAY}")
    )
    assert manifest.read_bytes().decode() == expected


def test_refresh_holds_an_opted_out_pin(manifest: Path) -> None:
    report = gate.refresh_manifest(
        manifest, names=("o/held",), dry_run=False, remote=FakeRemote(), today=TODAY
    )
    assert manifest.read_bytes().decode() == MANIFEST
    assert report == [
        f"o/held: held at {OLD_C[:12]} (refresh = false)",
        (f"0 of 1 pins move; oldest current pin: o/held {OLD_C[:12]} (2026-10-22)"),
    ]


def test_refresh_moves_only_the_named_repo(manifest: Path) -> None:
    gate.refresh_manifest(
        manifest, names=("o/b",), dry_run=False, remote=FakeRemote(), today=TODAY
    )
    expected = MANIFEST.replace(OLD_B, NEW_B).replace(OLD_D, NEW_B)
    assert manifest.read_bytes().decode() == expected


def test_dry_run_reports_moves_and_lapsing_waivers_without_writing(
    manifest: Path,
) -> None:
    report = gate.refresh_manifest(
        manifest, names=("o/a",), dry_run=True, remote=FakeRemote(), today=TODAY
    )
    assert manifest.read_bytes().decode() == MANIFEST
    assert report == [
        f"o/a: {OLD_A[:12]} (2026-01-01) -> {NEW_A[:12]} (2026-10-22)",
        f"waiver may lapse: yaml12 o/a: {WAIVED_PATH} (file removed)",
        f"1 of 1 pins move; oldest current pin: o/a {OLD_A[:12]} (2026-01-01)",
    ]
