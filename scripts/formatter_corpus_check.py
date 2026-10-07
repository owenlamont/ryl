# /// script
# requires-python = ">=3.14"
# dependencies = ["typer", "py-yaml12", "pyyaml", "platformdirs"]
# ///
"""Gate `ryl format` on the pinned real-world repos in scripts/formatter_corpus.toml.

Each repo is fetched shallow at its pinned commit (YAML files only, plus ignore and
config files) into a cache dir, formatted in place twice per mode, then reset. Modes:
`default` (the repo's own config, else built-in targets) and `fold`
(`fold-long-lines = true`, no repo config). Hard gates, any of which exits 1:

  - value: per changed file, the property suite's Rust oracle (`representation` and
    `annotations` in tests/property_format.rs) and py-yaml12 must both find the values,
    tags, anchors and comments unchanged; a file that did not parse must not change;
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
from collections.abc import Callable, Iterable
from dataclasses import asdict, dataclass, field
from enum import StrEnum
import fnmatch
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import time
from typing import Annotated, Any, Final

from formatter_compat import RULE_RE, values_equal
import platformdirs
import tomllib
import typer
import yaml
from yaml12 import parse_yaml


app = typer.Typer(add_completion=False, help=__doc__)

TESTED_DATE: Final = "2026-10-07"
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
_SKIP_RE: Final = re.compile(r"^(.+?):\d+:\d+ skipped by ")
_PANIC_EXIT: Final = 101
# `unreadable` means a side is not UTF-8, so the Rust oracle could not judge the pair.
_RUST_PASSES: Final = frozenset({"ok", "unreadable"})
_DEFAULT_CACHE: Final = Path(platformdirs.user_cache_dir("ryl-corpus"))


class CorpusError(Exception):
    """A repo could not be fetched at its pinned commit."""


class Mode(StrEnum):
    DEFAULT = "default"
    FOLD = "fold"


class Loaded(StrEnum):
    EQUAL = "equal"
    UNEQUAL = "unequal"
    UNAVAILABLE = "unavailable"


@dataclass(frozen=True, slots=True, kw_only=True)
class Repo:
    url: str
    sha: str
    note: str
    sparse: tuple[str, ...] = _YAML_PATTERNS
    expected_skip: tuple[str, ...] = ()
    tally: bool = True

    @property
    def name(self) -> str:
        """`owner/name`: the clone's directory and the report's key."""
        return self.url.removeprefix("https://").split("/", 1)[1]


@dataclass(kw_only=True)
class Result:
    repo: str
    mode: Mode
    files: int = 0
    changed: list[str] = field(default_factory=list)
    skipped: list[str] = field(default_factory=list)
    unexpected_skips: list[str] = field(default_factory=list)
    seconds: float = 0.0
    crashed: str = ""
    not_idempotent: list[str] = field(default_factory=list)
    yaml12_unequal: list[str] = field(default_factory=list)
    yaml12_unavailable: list[str] = field(default_factory=list)
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
    return _value_failures(result) + len(result.not_idempotent) + bool(result.crashed)


def _manifest(names: Iterable[str]) -> list[Repo]:
    rows = tomllib.loads(_MANIFEST.read_text(encoding="utf-8"))["repo"]
    repos = [
        Repo(
            url=row["url"],
            sha=row["sha"],
            note=row["note"],
            sparse=tuple(row.get("sparse", _YAML_PATTERNS)),
            expected_skip=tuple(row.get("expected-skip", ())),
            tally=row.get("tally", True),
        )
        for row in rows
    ]
    wanted = set(names)
    return [r for r in repos if not wanted or r.name in wanted]


def _git(cwd: Path, *args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=cwd, check=True, capture_output=True, text=True
    ).stdout.strip()


def _fetch(repo: Repo, cache: Path) -> Path:
    clone = cache / repo.name.replace("/", "__")
    if (clone / ".git").is_dir() and _git(clone, "rev-parse", "HEAD") == repo.sha:
        return clone
    shutil.rmtree(clone, ignore_errors=True)
    clone.mkdir(parents=True)
    _git(clone, "init", "-q")
    _git(clone, "config", "core.sparseCheckout", "true")
    _git(clone, "config", "core.sparseCheckoutCone", "false")
    patterns = "\n".join((*repo.sparse, *_ALWAYS_CHECKED_OUT))
    (clone / ".git" / "info" / "sparse-checkout").write_text(f"{patterns}\n")
    _git(clone, "fetch", "-q", "--depth", "1", "--filter=blob:none", repo.url, repo.sha)
    _git(clone, "checkout", "-q", "FETCH_HEAD")
    if (head := _git(clone, "rev-parse", "HEAD")) != repo.sha:
        msg = f"{repo.name}: checked out {head}, pinned {repo.sha}"
        raise CorpusError(msg)
    return clone


def _ryl(
    clone: Path, *args: str, timeout_s: float | None = None
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(_RYL), *args],
        cwd=clone,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout_s,
        check=False,
    )


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
        equal = values_equal(load(before), load(after))
    except (ValueError, RecursionError, yaml.YAMLError):
        return Loaded.UNAVAILABLE
    return Loaded.EQUAL if equal else Loaded.UNEQUAL


def _lint_args(clone: Path) -> list[str]:
    has_config = any((clone / name).is_file() for name in _CONFIG_NAMES)
    return [] if has_config else ["-d", "extends: default"]


def _lint(clone: Path) -> Counter[str]:
    proc = _ryl(clone, "check", "--format", "parsable", *_lint_args(clone), ".")
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
    config = ["-c", str(pair_dir.parent / "fold.toml")] if mode is Mode.FOLD else []
    listed = _ryl(clone, "check", "--list-files", *config, ".").stdout.splitlines()
    files = sorted(f for f in map(_relative, listed) if (clone / f).is_file())
    result.files = len(files)
    before = _snapshot(clone, files)
    if tally and repo.tally:
        check = _ryl(clone, "format", "--check", *config, ".")
        result.diagnostics = dict(sorted(_rule_counts(check.stdout).items()))
    lint_before = _lint(clone) if mode is Mode.DEFAULT else Counter()
    try:
        start = time.monotonic()
        first = _ryl(clone, "format", *config, ".", timeout_s=timeout_s)
        result.seconds = round(time.monotonic() - start, 2)
        after = _snapshot(clone, files)
        second = _ryl(clone, "format", *config, ".", timeout_s=timeout_s)
    except subprocess.TimeoutExpired:
        result.crashed = f"timeout after {timeout_s}s"
        _git(clone, "checkout", "--", ".")
        return result
    for proc in (first, second):
        if proc.returncode == _PANIC_EXIT or "panicked at" in proc.stderr:
            result.crashed = proc.stderr.strip()[-500:]
    twice = _snapshot(clone, files)
    result.not_idempotent = [f for f in files if twice[f] != after[f]]
    if mode is Mode.DEFAULT:
        lint_after = _lint(clone)
        result.lint_rose = {
            rule: [lint_before[rule], count]
            for rule, count in sorted(lint_after.items())
            if count > lint_before[rule]
        }
    _git(clone, "checkout", "--", ".")
    skips = (
        m.group(1) for line in first.stderr.splitlines() if (m := _SKIP_RE.match(line))
    )
    result.skipped = sorted(set(map(_relative, skips)))
    result.unexpected_skips = [
        f
        for f in result.skipped
        if not any(fnmatch.fnmatch(f, glob) for glob in repo.expected_skip)
    ]
    for path in files:
        if after[path] != before[path]:
            _record_change(
                result, path, before[path], after[path], pairs=pairs, pair_dir=pair_dir
            )
    return result


def _record_change(
    result: Result,
    path: str,
    before: bytes,
    after: bytes,
    *,
    pairs: list[tuple[str, Path]],
    pair_dir: Path,
) -> None:
    result.changed.append(path)
    stem = pair_dir / str(len(pairs))
    stem.with_suffix(".before").write_bytes(before)
    stem.with_suffix(".after").write_bytes(after)
    pairs.append((f"{result.mode}\t{result.repo}\t{path}", stem))
    match _compare(_load12, before, after):
        case Loaded.UNEQUAL:
            result.yaml12_unequal.append(path)
        case Loaded.UNAVAILABLE:
            result.yaml12_unavailable.append(path)
        case Loaded.EQUAL:
            pass
    if _compare(_load11, before, after) is Loaded.UNEQUAL:
        result.yaml11_drift.append(path)


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


def _summary(results: Iterable[Result]) -> str:
    columns = (
        "repo",
        "mode",
        "files",
        "changed",
        "skipped (unexpected)",
        "value failures",
        "not idempotent",
        "crash",
        "py-yaml12 unavailable",
        "1.1 drift",
        "lint rose",
        "seconds",
    )
    header = f"| {' | '.join(columns)} |\n|{' --- |' * len(columns)}\n"
    rows = (
        f"| {r.repo} | {r.mode} | {r.files} | {len(r.changed)} "
        f"| {len(r.skipped)} ({len(r.unexpected_skips)}) "
        f"| {_value_failures(r)} "
        f"| {len(r.not_idempotent)} | {'yes' if r.crashed else ''} "
        f"| {len(r.yaml12_unavailable)} | {len(r.yaml11_drift)} "
        f"| {', '.join(r.lint_rose)} | {r.seconds} |\n"
        for r in sorted(results, key=lambda r: (r.repo.lower(), r.mode))
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
def fetch(cache_dir: _CacheOption = _DEFAULT_CACHE, repo: _RepoOption = None) -> None:
    """Fetch each pinned repo into the cache, without formatting anything."""
    for row in _manifest(repo or ()):
        typer.echo(f"{row.name}: {_fetch(row, cache_dir / 'repos')}")


@app.command()
def run(
    cache_dir: _CacheOption = _DEFAULT_CACHE,
    repo: _RepoOption = None,
    tally: Annotated[
        bool,
        typer.Option(
            help="Count `format --check` diagnostics first (slow on huge files)."
        ),
    ] = True,
    timeout_s: Annotated[float, typer.Option(help="Per `ryl format` run.")] = 900.0,
) -> None:
    """Format every pinned repo in each mode and gate the result.

    Raises:
        Exit: 1 on any hard-gate failure.
    """
    subprocess.run(["cargo", "build", "--release"], cwd=_ROOT, check=True)
    work = cache_dir / "work"
    shutil.rmtree(work, ignore_errors=True)
    pair_dir = work / "pairs"
    pair_dir.mkdir(parents=True)
    (work / "fold.toml").write_text(_FOLD_CONFIG, encoding="utf-8")
    results: list[Result] = []
    pairs: list[tuple[str, Path]] = []
    jsonl = (work / "results.jsonl").open("w", encoding="utf-8")
    for row in _manifest(repo or ()):
        clone = _fetch(row, cache_dir / "repos")
        for mode in Mode:
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
    jsonl.close()
    verdicts = _rust_oracle(pairs, work)
    for result in results:
        prefix = f"{result.mode}\t{result.repo}\t"
        for key, verdict in verdicts.items():
            if key.startswith(prefix):
                result.rust.setdefault(verdict, []).append(key.removeprefix(prefix))
    (work / "results.json").write_text(
        json.dumps([asdict(r) for r in results], indent=1), encoding="utf-8"
    )
    summary = _summary(results)
    (work / "summary.md").write_text(summary, encoding="utf-8")
    typer.echo(f"\n{summary}\nReport: {work}")
    if failures := sum(_hard_failures(r) for r in results):
        typer.secho(f"{failures} hard-gate failure(s)", fg=typer.colors.RED, err=True)
        raise typer.Exit(1)


if __name__ == "__main__":
    app()
