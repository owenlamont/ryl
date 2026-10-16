# /// script
# requires-python = ">=3.15"
# dependencies = ["typer", "py-yaml12", "pyyaml", "platformdirs"]
# ///
"""Gate `ryl format` on the pinned real-world repos in scripts/formatter_corpus.toml.

Each repo is fetched shallow at its pinned commit into a cache dir, formatted in
place twice per mode, then reset. Separate Markdown pins run through
`format --markdown` and `check --fix --markdown` with the same YAML-region oracles
and a host-byte check; each default-mode operation must change a YAML region. Modes:
`default` (the repo's own config, else built-in targets) and `fold`
(`fold-long-lines = true`, no repo config). Hard gates, any of which exits 1:

  - value: per changed file, the property suite's Rust oracle (`representation` and
    `annotations` in tests/property_format.rs) must find the values, tags, anchors and
    comments unchanged, and py-yaml12 the loaded values, with unequal readings
    adjudicated by reference events; a file that did not parse must not change;
  - idempotence: a second `ryl format` changes nothing;
  - panic or timeout.

Soft signals: py-yaml12 unavailable, PyYAML (YAML 1.1) drift, lint rules whose count
rose, unexpected syntax skips, wall-clock time, and the per-rule diagnostic counts of
`ryl format --check` on the original bytes.

Manual / opt-in, not CI. It needs a ryl source checkout (it builds `ryl` and runs the
Rust oracle with `cargo test --release`), `git` and `uv`. Run from the repo root:

    uv run scripts/formatter_corpus_check.py run
"""

from collections import Counter
from collections.abc import Callable, Container, Iterable, Mapping
from dataclasses import asdict, dataclass, field
from enum import StrEnum
import fnmatch
from functools import partial
import hashlib
from itertools import starmap
import json
import math
import os
from pathlib import Path
import re
import shutil
import subprocess
import time
import tomllib
from typing import Annotated, Any, Final

import platformdirs
from reference_events import reference_equal
import typer
import yaml
from yaml12 import parse_yaml


app = typer.Typer(add_completion=False, help=__doc__)

TESTED_DATE: Final = "2026-10-15"
_ROOT: Final = Path(__file__).resolve().parent.parent
_MANIFEST: Final = Path(__file__).with_name("formatter_corpus.toml")
_RYL: Final = _ROOT / "target" / "release" / ("ryl.exe" if os.name == "nt" else "ryl")
_YAML_PATTERNS: Final = ("*.yaml", "*.yml")
_ALWAYS_CHECKED_OUT: Final = (
    ".gitignore",
    ".yamllint*",
    ".ryl.toml",
    "ryl.toml",
    "/.config/",
)
_CONFIG_NAMES: Final = (
    ".yamllint",
    ".yamllint.yaml",
    ".yamllint.yml",
    ".ryl.toml",
    "ryl.toml",
    ".config/.ryl.toml",
    ".config/ryl.toml",
)
_FOLD_CONFIG: Final = "[format]\nfold-long-lines = true\n"
_DEFAULT_LINT_RULES: Final = (
    "anchors,braces,brackets,colons,commas,comments,comments-indentation,"
    "document-start,empty-lines,hyphens,indentation,key-duplicates,line-length,"
    "new-line-at-end-of-file,new-lines,trailing-spaces,truthy"
)
_SKIP_RE: Final = re.compile(r"^(.+?):\d+:\d+ skipped by ")
RULE_RE: Final = re.compile(r"\(([a-z][a-z0-9-]*)\)\s*$")
# `unreadable` means a side is not UTF-8, so the Rust oracle could not judge the pair.
_RUST_PASSES: Final = frozenset({"ok", "unreadable"})
_DEFAULT_CACHE: Final = Path(platformdirs.user_cache_dir("ryl-corpus"))
KnownError = tuple[str, str, str]


def _values_equal(a: object, b: object) -> bool:
    if isinstance(a, float) and isinstance(b, float):
        return a == b or (math.isnan(a) and math.isnan(b))
    if type(a) is not type(b):
        return False
    if isinstance(a, dict):
        return a.keys() == b.keys() and all(_values_equal(a[k], b[k]) for k in a)
    if isinstance(a, list):
        return len(a) == len(b) and all(starmap(_values_equal, zip(a, b, strict=False)))
    return a == b


class CorpusError(Exception):
    """A repo could not be fetched at its pinned commit."""


class Mode(StrEnum):
    DEFAULT = "default"
    FOLD = "fold"


class Profile(StrEnum):
    STANDARD = "standard"
    BLOCK_CRLF = "block-crlf"
    FLOW_NARROW = "flow-narrow"
    PREVIEW_AUTO = "preview-auto"


class Loaded(StrEnum):
    EQUAL = "equal"
    UNEQUAL = "unequal"
    UNAVAILABLE = "unavailable"


@dataclass(frozen=True, slots=True, kw_only=True)
class Repo:
    url: str
    sha: str
    sparse: tuple[str, ...] = _YAML_PATTERNS
    markdown_sparse: tuple[str, ...] = ()
    expected_skip: tuple[str, ...] = ()
    expected_skip_marker: Mapping[str, str] = field(default_factory=dict)
    tally: bool = True
    modes: tuple[Mode, ...] = tuple(Mode)
    yaml12_known_errors: frozenset[KnownError] = frozenset()
    rust_known_errors: frozenset[KnownError] = frozenset()

    @property
    def name(self) -> str:
        """`owner/name`: the clone's directory and the report's key."""
        return self.url.removeprefix("https://").split("/", 1)[1]


@dataclass(kw_only=True)
class Result:
    repo: str
    mode: Mode
    operation: str = "yaml-format"
    files: int = 0
    regions: int = 0
    changed_regions: int = 0
    front_matter: int = 0
    fences: int = 0
    host_changed: list[str] = field(default_factory=list)
    errors: list[str] = field(default_factory=list)
    changed: list[str] = field(default_factory=list)
    skipped: list[str] = field(default_factory=list)
    unexpected_skips: list[str] = field(default_factory=list)
    seconds: float = 0.0
    not_idempotent: list[str] = field(default_factory=list)
    yaml12_unequal: list[str] = field(default_factory=list)
    yaml12_unavailable: list[str] = field(default_factory=list)
    yaml12_known_errors: list[KnownError] = field(default_factory=list)
    rust_waivable: dict[str, KnownError] = field(default_factory=dict)
    rust_known_errors: list[KnownError] = field(default_factory=list)
    yaml11_drift: list[str] = field(default_factory=list)
    rust: dict[str, list[str]] = field(default_factory=dict)
    lint_rose: dict[str, list[int]] = field(default_factory=dict)
    diagnostics: dict[str, int] = field(default_factory=dict)


def _value_failures(result: Result) -> int:
    rust = (
        p for v, paths in result.rust.items() if v not in _RUST_PASSES for p in paths
    )
    return len(result.yaml12_unequal) + sum(1 for _ in rust)


def _hard_failures(result: Result) -> int:
    return (
        _value_failures(result)
        + len(result.not_idempotent)
        + len(result.errors)
        + len(result.host_changed)
    )


def _manifest(
    names: Iterable[str], profile: Profile = Profile.STANDARD, *, markdown: bool = False
) -> list[Repo]:
    section = "markdown-repo" if markdown else "repo"
    rows = tomllib.loads(_MANIFEST.read_text(encoding="utf-8"))[section]
    repos = [
        Repo(
            url=row["url"],
            sha=row["sha"],
            sparse=tuple(row.get("sparse", _YAML_PATTERNS)),
            markdown_sparse=tuple(row.get("markdown-sparse", ())),
            expected_skip=tuple(row.get("expected-skip", ())),
            expected_skip_marker=row.get("expected-skip-marker", {}),
            tally=row.get("tally", True),
            modes=tuple(Mode(m) for m in row.get("modes", Mode)),
            yaml12_known_errors=_known(row.get("yaml12-known-errors", ()), profile),
            rust_known_errors=_known(row.get("rust-known-errors", ()), profile),
        )
        for row in rows
    ]
    wanted = set(names)
    if unknown := wanted - {r.name for r in repos}:
        msg = f"not in the manifest: {', '.join(sorted(unknown))}"
        raise typer.BadParameter(msg, param_hint="--repo")
    return [r for r in repos if not wanted or r.name in wanted]


def _known(
    entries: Iterable[Mapping[str, str]], profile: Profile
) -> frozenset[KnownError]:
    return frozenset(
        (e["path"], e["before-sha256"], e["after-sha256"])
        for e in entries
        if e.get("profile", "standard") == profile
    )


def _git(cwd: Path, *args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=cwd, check=True, capture_output=True, text=True
    ).stdout.strip()


def _fetch(repo: Repo, cache: Path) -> Path:
    clone = cache / repo.name.replace("/", "__")
    if (clone / ".git").is_dir() and _git(clone, "rev-parse", "HEAD") == repo.sha:
        if dirty := _git(clone, "status", "--porcelain"):
            msg = f"{repo.name}: cached checkout has local changes:\n{dirty}"
            raise CorpusError(msg)
        return clone
    shutil.rmtree(clone, ignore_errors=True)
    clone.mkdir(parents=True)
    _git(clone, "init", "-q")
    _git(clone, "config", "core.sparseCheckout", "true")
    _git(clone, "config", "core.sparseCheckoutCone", "false")
    patterns = "\n".join((*_ALWAYS_CHECKED_OUT, *repo.sparse, *repo.markdown_sparse))
    (clone / ".git" / "info" / "sparse-checkout").write_text(f"{patterns}\n")
    _git(clone, "fetch", "-q", "--depth", "1", "--filter=blob:none", repo.url, repo.sha)
    _git(clone, "checkout", "-q", "FETCH_HEAD")
    if (head := _git(clone, "rev-parse", "HEAD")) != repo.sha:
        msg = f"{repo.name}: checked out {head}, pinned {repo.sha}"
        raise CorpusError(msg)
    return clone


def _ryl(
    clone: Path, result: Result, *args: str, timeout_s: float, ok: Container[int] = (0,)
) -> subprocess.CompletedProcess[str]:
    """Run ryl in `clone`; a timeout, panic or exit outside `ok` joins `result.errors`.

    Returns:
        The finished process, or an empty one with exit -1 after a timeout.
    """
    command = f"ryl {' '.join(args)}"
    try:
        proc = subprocess.run(
            [str(_RYL), *args],
            cwd=clone,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout_s,
            check=False,
        )
    except subprocess.TimeoutExpired:
        result.errors.append(f"{command}: timeout after {timeout_s}s")
        return subprocess.CompletedProcess(args, -1, "", "")
    if proc.returncode not in ok or "panicked at" in proc.stderr:
        tail = proc.stderr.strip()[-300:]
        result.errors.append(f"{command}: exit {proc.returncode}: {tail}")
    return proc


def _relative(listed: str) -> str:
    return listed.replace("\\", "/").removeprefix("./")


def _snapshot(clone: Path, files: Iterable[str]) -> dict[str, bytes]:
    return {f: (clone / f).read_bytes() for f in files}


def _rule_counts(output: str) -> Counter[str]:
    return Counter(
        m.group(1)
        for line in output.splitlines()
        if (m := RULE_RE.search(line)) and m.group(1) != "syntax"
    )


def _load12(data: bytes) -> Any:
    return parse_yaml(data.decode("utf-8"), multi=True)


def _load11(data: bytes) -> Any:
    return list(yaml.safe_load_all(data.decode("utf-8")))


def _compare(load: Callable[[bytes], Any], before: bytes, after: bytes) -> Loaded:
    try:
        equal = _values_equal(load(before), load(after))
    except (ValueError, RecursionError, yaml.YAMLError):
        return Loaded.UNAVAILABLE
    return Loaded.EQUAL if equal else Loaded.UNEQUAL


def _final_block_scalar_prefix(data: bytes) -> bytes | None:
    try:
        source = data.decode("utf-8")
        tokens = list(yaml.scan(source))
    except (ValueError, RecursionError, AssertionError, yaml.YAMLError):
        return None
    for index in range(len(tokens) - 1, -1, -1):
        token = tokens[index]
        if isinstance(token, yaml.ScalarToken):
            if token.style in ("|", ">") and all(
                isinstance(
                    tail,
                    (yaml.BlockEndToken, yaml.DocumentEndToken, yaml.StreamEndToken),
                )
                for tail in tokens[index + 1 :]
            ):
                return source[: token.start_mark.index].encode("utf-8")
            return None
    return None


def _compare12(before: bytes, after: bytes) -> Loaded:
    verdict = _compare(_load12, before, after)
    eof_scalar_changed = (
        _final_block_scalar_prefix(before) is not None
        or _final_block_scalar_prefix(after) is not None
    )
    if verdict is Loaded.UNEQUAL or eof_scalar_changed:
        try:
            reference = reference_equal(
                before.decode("utf-8"),
                after.decode("utf-8"),
                ignore_style=verdict is Loaded.EQUAL,
            )
        except UnicodeDecodeError:
            reference = None
        if reference is not None:
            return Loaded.EQUAL if reference else Loaded.UNEQUAL
        if eof_scalar_changed and verdict is Loaded.EQUAL:
            return Loaded.UNAVAILABLE
    return verdict


def _lint_args(clone: Path) -> list[str]:
    has_config = any((clone / name).is_file() for name in _CONFIG_NAMES)
    return [] if has_config else ["-d", "extends: default"]


def _lint(
    ryl: Callable[..., subprocess.CompletedProcess[str]], clone: Path
) -> Counter[str]:
    proc = ryl("check", "--format", "parsable", *_lint_args(clone), ".", ok=(0, 1))
    return _rule_counts(proc.stdout + proc.stderr)


def _run(
    repo: Repo,
    clone: Path,
    mode: Mode,
    *,
    pairs: list[tuple[str, Path]],
    pair_dir: Path,
    tally: bool,
    timeout_s: float,
) -> Result:
    result = Result(repo=repo.name, mode=mode)
    ryl = partial(_ryl, clone, result, timeout_s=timeout_s)
    config = ["-c", str(pair_dir.parent / "fold.toml")] if mode is Mode.FOLD else []
    listing = ryl("check", "--list-files", *config, ".")
    files = sorted(
        f for f in map(_relative, listing.stdout.splitlines()) if (clone / f).is_file()
    )
    result.files = len(files)
    if result.errors:
        return result
    before = _snapshot(clone, files)
    if tally and repo.tally:
        check = ryl("format", "--check", *config, ".", ok=(0, 1))
        result.diagnostics = dict(sorted(_rule_counts(check.stdout).items()))
    lint_before = _lint(ryl, clone) if mode is Mode.DEFAULT else Counter()
    start = time.monotonic()
    first = ryl("format", *config, ".")
    result.seconds = round(time.monotonic() - start, 2)
    after = _snapshot(clone, files)
    ryl("format", *config, ".")
    twice = _snapshot(clone, files)
    result.not_idempotent = [f for f in files if twice[f] != after[f]]
    if mode is Mode.DEFAULT:
        lint_after = _lint(ryl, clone)
        result.lint_rose = {
            rule: [lint_before[rule], count]
            for rule, count in sorted(lint_after.items())
            if count > lint_before[rule]
        }
    _git(clone, "checkout", "--", ".")
    result.skipped, result.unexpected_skips = _unexpected_skips(
        repo, clone, first.stderr
    )
    for path in files:
        if after[path] != before[path]:
            _record_change(
                result,
                repo,
                path,
                before[path],
                after[path],
                pairs=pairs,
                pair_dir=pair_dir,
            )
    return result


def _markdown_files(repo: Repo, clone: Path) -> list[str]:
    listed = subprocess.run(
        ["git", "ls-files", "-z"], cwd=clone, check=True, capture_output=True
    ).stdout
    return sorted(
        path
        for raw in listed.split(b"\0")
        if (path := os.fsdecode(raw))
        if (clone / path).is_file()
        and any(fnmatch.fnmatch(path, pattern) for pattern in repo.markdown_sparse)
    )


def _markdown_regions(stems: list[Path], work: Path) -> Path:
    pairs_file = work / "markdown-pairs.tsv"
    regions_file = work / "markdown-regions.json"
    pairs_file.write_text(
        "".join(f"{stem}.before\t{stem}.after\n" for stem in stems), encoding="utf-8"
    )
    subprocess.run(
        [
            "cargo",
            "test",
            "--release",
            "--test",
            "property_markdown_format",
            "corpus_markdown_regions",
            "--",
            "--ignored",
            "--exact",
        ],
        cwd=_ROOT,
        env=os.environ
        | {
            "RYL_MARKDOWN_PAIRS": str(pairs_file),
            "RYL_MARKDOWN_REGIONS": str(regions_file),
        },
        check=True,
    )
    return regions_file


def _record_markdown_regions(
    repo: Repo,
    result: Result,
    files: list[str],
    manifest: Path,
    *,
    pairs: list[tuple[str, Path]],
    pair_dir: Path,
) -> None:
    rows = json.loads(manifest.read_text(encoding="utf-8"))
    by_file: dict[int, list[dict[str, Any]]] = {}
    for row in rows:
        by_file.setdefault(row["file"], []).append(row)
    for index, path in enumerate(files):
        entries = by_file.get(index, [])
        if error := next((row["error"] for row in entries if "error" in row), None):
            result.errors.append(f"{path}: {error}")
            continue
        summary = next((row for row in entries if "host_equal" in row), None)
        regions = [row for row in entries if "region" in row]
        if summary is None or summary["regions"] != len(regions):
            result.errors.append(f"{path}: Markdown region extraction mismatch")
            continue
        result.regions += len(regions)
        if not summary["host_equal"]:
            result.host_changed.append(path)
        for row in regions:
            label = "front-matter" if row["kind"] == "FrontMatter" else "fence"
            if label == "front-matter":
                result.front_matter += 1
            else:
                result.fences += 1
            region_path = f"{path}#{label}-{row['region'] + 1}"
            before = Path(row["before"]).read_bytes()
            after = Path(row["after"]).read_bytes()
            if before != after:
                result.changed_regions += 1
                _record_change(
                    result,
                    repo,
                    region_path,
                    before,
                    after,
                    pairs=pairs,
                    pair_dir=pair_dir,
                )


def _run_markdown(
    repo: Repo,
    clone: Path,
    mode: Mode,
    operation: str,
    *,
    pairs: list[tuple[str, Path]],
    pair_dir: Path,
    timeout_s: float,
) -> Result:
    result = Result(repo=repo.name, mode=mode, operation=operation)
    files = _markdown_files(repo, clone)
    result.files = len(files)
    if not files:
        result.errors.append(f"{operation}: no Markdown files visited")
        return result
    ryl = partial(_ryl, clone, result, timeout_s=timeout_s)
    config = (
        ["-c", str(pair_dir.parent / "fold.toml")]
        if mode is Mode.FOLD
        else _lint_args(clone)
    )
    command = ["format"] if operation == "markdown-format" else ["check", "--fix"]
    if operation == "markdown-fix" and mode is Mode.FOLD:
        command.extend(("--enable", _DEFAULT_LINT_RULES))
    before = _snapshot(clone, files)

    def apply() -> str:
        stderr = []
        for start in range(0, len(files), 100):
            proc = ryl(
                *command,
                "--markdown",
                *config,
                *files[start : start + 100],
                ok=(0,) if operation == "markdown-format" else (0, 1),
            )
            stderr.append(proc.stderr)
        return "\n".join(stderr)

    start = time.monotonic()
    first_stderr = apply()
    result.seconds = round(time.monotonic() - start, 2)
    after = _snapshot(clone, files)
    apply()
    twice = _snapshot(clone, files)
    changed = [path for path in files if before[path] != after[path]]
    result.not_idempotent = [path for path in files if after[path] != twice[path]]
    result.skipped, result.unexpected_skips = _unexpected_skips(
        repo, clone, first_stderr
    )
    stems = []
    for index, path in enumerate(files):
        stem = (
            pair_dir
            / f"markdown-{repo.name.replace('/', '-')}-{mode}-{operation}-{index}"
        )
        stem.with_suffix(".before").write_bytes(before[path])
        stem.with_suffix(".after").write_bytes(after[path])
        stems.append(stem)
    _git(clone, "checkout", "--", ".")
    manifest = _markdown_regions(stems, pair_dir.parent)
    _record_markdown_regions(
        repo, result, files, manifest, pairs=pairs, pair_dir=pair_dir
    )
    if not result.changed_regions:
        result.errors.append(f"{operation}: no YAML regions changed")
    result.changed = changed
    return result


def _unexpected_skips(
    repo: Repo, clone: Path, stderr: str
) -> tuple[list[str], list[str]]:
    skips = (m.group(1) for line in stderr.splitlines() if (m := _SKIP_RE.match(line)))
    skipped = sorted(set(map(_relative, skips)))
    marker = repo.expected_skip_marker
    return skipped, [
        f
        for f in skipped
        if not any(fnmatch.fnmatch(f, glob) for glob in repo.expected_skip)
        and not (
            marker
            and Path(f).name == marker["file"]
            and (clone / f).with_name(marker["marker"]).is_file()
        )
    ]


def _record_change(
    result: Result,
    repo: Repo,
    path: str,
    before: bytes,
    after: bytes,
    *,
    pairs: list[tuple[str, Path]],
    pair_dir: Path,
) -> None:
    """Record a changed file; a known-error waiver holds only for reviewed output."""
    result.changed.append(path)
    stem = pair_dir / str(len(pairs))
    stem.with_suffix(".before").write_bytes(before)
    stem.with_suffix(".after").write_bytes(after)
    key = str(result.mode)
    if result.operation != "yaml-format":
        key = f"{key}:{result.operation}"
    pairs.append((f"{key}\t{result.repo}\t{path}", stem))
    reviewed = (
        path,
        hashlib.sha256(before).hexdigest(),
        hashlib.sha256(after).hexdigest(),
    )
    if reviewed in repo.rust_known_errors:
        result.rust_waivable[path] = reviewed
    match _compare12(before, after):
        case Loaded.UNEQUAL if reviewed in repo.yaml12_known_errors:
            result.yaml12_known_errors.append(reviewed)
        case Loaded.UNEQUAL:
            result.yaml12_unequal.append(path)
        case Loaded.UNAVAILABLE:
            result.yaml12_unavailable.append(path)
        case Loaded.EQUAL:
            pass
    if _compare(_load11, before, after) is Loaded.UNEQUAL:
        result.yaml11_drift.append(path)


def _assign_verdicts(results: Iterable[Result], verdicts: Mapping[str, str]) -> None:
    """Only a value-preservation verdict on reviewed output is waivable."""
    for result in results:
        key = str(result.mode)
        if result.operation != "yaml-format":
            key = f"{key}:{result.operation}"
        prefix = f"{key}\t{result.repo}\t"
        for key, verdict in verdicts.items():
            if not key.startswith(prefix):
                continue
            path = key.removeprefix(prefix)
            if verdict == "value-preservation" and path in result.rust_waivable:
                result.rust_known_errors.append(result.rust_waivable[path])
            else:
                result.rust.setdefault(verdict, []).append(path)


def _rust_oracle(pairs: list[tuple[str, Path]], work: Path) -> dict[str, str]:
    pairs_file, verdicts_file = work / "pairs.tsv", work / "verdicts.txt"
    pairs_file.write_text(
        "".join(
            f"{stem.with_suffix('.before')}\t{stem.with_suffix('.after')}\n"
            for _, stem in pairs
        ),
        encoding="utf-8",
    )
    subprocess.run(
        [
            "cargo",
            "test",
            "--release",
            "--test",
            "property_format",
            "corpus_pairs_keep_the_guarantee",
            "--",
            "--ignored",
            "--exact",
        ],
        cwd=_ROOT,
        env=os.environ
        | {
            "RYL_CORPUS_PAIRS": str(pairs_file),
            "RYL_CORPUS_VERDICTS": str(verdicts_file),
        },
        check=True,
    )
    verdicts = verdicts_file.read_text(encoding="utf-8").splitlines()
    return {key: verdict for (key, _), verdict in zip(pairs, verdicts, strict=True)}


def _stale_known_errors(repos: Iterable[Repo], results: list[Result]) -> list[str]:
    stale = []
    for row in repos:
        ran = [r for r in results if r.repo == row.name]
        for oracle, listed, hit in (
            (
                "rust",
                row.rust_known_errors,
                {entry for r in ran for entry in r.rust_known_errors},
            ),
            (
                "yaml12",
                row.yaml12_known_errors,
                {entry for r in ran for entry in r.yaml12_known_errors},
            ),
        ):
            stale.extend(
                f"{oracle} {row.name}: {entry[0]} ({entry[1]} -> {entry[2]})"
                for entry in sorted(listed - hit)
            )
    return stale


def _property_failures(cases: int) -> list[str]:
    suites = sorted(path.stem for path in (_ROOT / "tests").glob("property_*.rs"))
    return [
        suite
        for suite in suites
        if subprocess.run(
            ["cargo", "test", "--release", "--test", suite],
            cwd=_ROOT,
            env=os.environ | {"PROPTEST_CASES": str(cases)},
            check=False,
        ).returncode
    ]


def _summary(results: Iterable[Result]) -> str:
    columns = (
        "repo",
        "mode",
        "operation",
        "files",
        "regions",
        "changed",
        "host changed",
        "skipped (unexpected)",
        "value failures",
        "not idempotent",
        "ryl errors",
        "py-yaml12 unavailable (known errors)",
        "Rust known errors",
        "1.1 drift",
        "lint rose",
        "seconds",
    )
    header = f"| {' | '.join(columns)} |\n|{' --- |' * len(columns)}\n"
    rows = (
        f"| {r.repo} | {r.mode} | {r.operation} | {r.files} | {r.regions} "
        f"| {len(r.changed)} | {len(r.host_changed)} "
        f"| {len(r.skipped)} ({len(r.unexpected_skips)}) "
        f"| {_value_failures(r)} "
        f"| {len(r.not_idempotent)} | {len(r.errors)} "
        f"| {len(r.yaml12_unavailable)} ({len(r.yaml12_known_errors)}) "
        f"| {len(r.rust_known_errors)} | {len(r.yaml11_drift)} "
        f"| {', '.join(r.lint_rose)} | {r.seconds} |\n"
        for r in sorted(results, key=lambda r: (r.repo.lower(), r.mode, r.operation))
    )
    return header + "".join(rows)


_CacheOption = Annotated[
    Path, typer.Option(help="Where pinned clones and the report go.")
]
_RepoOption = Annotated[
    list[str] | None,
    typer.Option("--repo", help="Only this owner/name (repeatable); default all."),
]


@app.command()
def fetch(
    cache_dir: _CacheOption = _DEFAULT_CACHE,
    repo: _RepoOption = None,
    markdown: Annotated[bool, typer.Option(help="Fetch Markdown corpus pins.")] = False,
) -> None:
    """Fetch each pinned repo into the cache, without formatting anything."""
    clone_dir = cache_dir / ("markdown-repos" if markdown else "repos")
    for row in _manifest(repo or (), markdown=markdown):
        typer.echo(f"{row.name}: {_fetch(row, clone_dir)}")


@app.command()
def run(
    cache_dir: _CacheOption = _DEFAULT_CACHE,
    repo: _RepoOption = None,
    markdown: Annotated[
        bool,
        typer.Option(help="Use Markdown pins and gate their YAML and Markdown files."),
    ] = False,
    profile: Annotated[
        Profile, typer.Option(help="Select known errors for this corpus profile.")
    ] = Profile.STANDARD,
    tally: Annotated[
        bool,
        typer.Option(
            help="Count `format --check` diagnostics first (slow on huge files)."
        ),
    ] = True,
    timeout_s: Annotated[float, typer.Option(help="Per `ryl format` run.")] = 900.0,
    proptest_cases: Annotated[
        int,
        typer.Option(
            help="Then run every property suite at this case count, one after "
            "another; 0 skips them."
        ),
    ] = 0,
) -> None:
    """Format every pinned repo in each mode and gate the result.

    Raises:
        Exit: 1 on any hard-gate failure.
    """
    rows = _manifest(repo or (), profile, markdown=markdown)
    clone_dir = cache_dir / ("markdown-repos" if markdown else "repos")
    typer.echo(f"Corpus pinned on {TESTED_DATE}")
    subprocess.run(["cargo", "build", "--release"], cwd=_ROOT, check=True)
    work = cache_dir / "work" / str(time.time_ns())
    pair_dir = work / "pairs"
    pair_dir.mkdir(parents=True)
    (work / "fold.toml").write_text(_FOLD_CONFIG, encoding="utf-8")
    results: list[Result] = []
    pairs: list[tuple[str, Path]] = []
    jsonl = (work / "results.jsonl").open("w", encoding="utf-8")
    for row in rows:
        clone = _fetch(row, clone_dir)
        for mode in row.modes:
            result = _run(
                row,
                clone,
                mode,
                pairs=pairs,
                pair_dir=pair_dir,
                tally=tally,
                timeout_s=timeout_s,
            )
            results.append(result)
            jsonl.write(json.dumps(asdict(result)) + "\n")
            jsonl.flush()
            changed = len(result.changed)
            typer.echo(f"{row.name} [{mode}]: {result.files} files, {changed} changed")
            for error in result.errors:
                typer.secho(f"  {error}", err=True)
            if markdown and row.markdown_sparse:
                for operation in ("markdown-format", "markdown-fix"):
                    md_result = _run_markdown(
                        row,
                        clone,
                        mode,
                        operation,
                        pairs=pairs,
                        pair_dir=pair_dir,
                        timeout_s=timeout_s,
                    )
                    results.append(md_result)
                    jsonl.write(json.dumps(asdict(md_result)) + "\n")
                    jsonl.flush()
                    typer.echo(
                        f"{row.name} [{mode} {operation}]: "
                        f"{md_result.files} files, {md_result.regions} regions, "
                        f"{len(md_result.changed)} changed"
                    )
                    for error in md_result.errors:
                        typer.secho(f"  {error}", err=True)
    jsonl.close()
    verdicts = _rust_oracle(pairs, work)
    _assign_verdicts(results, verdicts)
    stale = _stale_known_errors(rows, results)
    (work / "results.json").write_text(
        json.dumps([asdict(r) for r in results], indent=1), encoding="utf-8"
    )
    summary = _summary(results)
    (work / "summary.md").write_text(summary, encoding="utf-8")
    typer.echo(f"\n{summary}\nReport: {work}")
    for entry in stale:
        typer.secho(f"Stale known-error entry, now passes: {entry}", err=True)
    failures = sum(_hard_failures(r) for r in results) + len(stale)
    if proptest_cases:
        failed_suites = _property_failures(proptest_cases)
        typer.echo(
            f"Property suites failing at {proptest_cases} cases: {failed_suites}"
        )
        failures += len(failed_suites)
    if failures:
        typer.secho(f"{failures} hard-gate failure(s)", fg=typer.colors.RED, err=True)
        raise typer.Exit(1)


if __name__ == "__main__":
    app()
