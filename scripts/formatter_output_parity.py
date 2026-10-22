# /// script
# requires-python = ">=3.15"
# dependencies = ["typer", "py-yaml12", "pyyaml", "platformdirs"]
# ///
from dataclasses import dataclass
from pathlib import Path
import subprocess  # ruff: ignore[suspicious-subprocess-import]
import tempfile
from typing import Annotated, Final

import formatter_corpus_check as corpus
import typer


app = typer.Typer(add_completion=False)

PROFILES: Final[dict[str, str | None]] = {
    "default": None,
    "fold": corpus._FOLD_CONFIG,
    "width-lf": "line-length = 40\n[format]\nline-ending = 'lf'\n",
    "crlf-single": "[format]\nline-ending = 'cr-lf'\nquote-style = 'single'\n",
    "quote-double": "[format]\nquote-style = 'double'\n",
    "auto-double": "[format]\nline-ending = 'auto'\nquote-style = 'double'\n",
    "markers": "[format]\ndocument-start = 'add'\ndocument-end = 'add'\n",
    "brace-spacing": "[format]\nbrace-spacing = true\n",
    "preview": "[format]\npreview = true\n",
    "non-defaults": (
        "[format]\nquote-style = 'preserve'\nline-ending = 'cr-lf'\n"
        "document-start = 'add'\ndocument-end = 'add'\nfold-long-lines = true\n"
        "comment-spacing = 1\ncomment-starting-space = 'preserve'\n"
        "max-blank-lines = 0\nsequence-style = 'block'\nmapping-style = 'block'\n"
    ),
    "wide-spacing": "[format]\ncomment-spacing = 3\nmax-blank-lines = 1\n",
    "fold-narrow": (
        "line-length = 12\nindent-width = 2\n[format]\nfold-long-lines = true\n"
    ),
    "fold-1": "line-length = 1\nindent-width = 2\n[format]\nfold-long-lines = true\n",
    "fold-indent-4": (
        "line-length = 12\nindent-width = 4\n[format]\nfold-long-lines = true\n"
    ),
    "fold-indent-1": (
        "line-length = 12\nindent-width = 1\n[format]\nfold-long-lines = true\n"
    ),
    "sequence": (
        "indent-width = 4\n[format]\n"
        "indent-sequences = false\ndash-on-own-line = true\n"
    ),
    "sequence-block": "[format]\nsequence-style = 'block'\n",
    "mapping-block": "indent-width = 4\n[format]\nmapping-style = 'block'\n",
    "block": "[format]\nsequence-style = 'block'\nmapping-style = 'block'\n",
    "flow": (
        "line-length = 30\n[format]\nsequence-style = 'flow'\nmapping-style = 'flow'\n"
    ),
    "mixed": (
        "indent-width = 2\nline-length = 24\n[format]\n"
        "quote-style = 'preserve'\nline-ending = 'auto'\n"
        "document-start = 'add'\nfold-long-lines = true\n"
        "brace-spacing = true\npreview = true\n"
    ),
}

FIXTURES: Final[dict[str, str | bytes]] = {
    "root-marker-followup.yaml": "\"--- 'quoted'\"\n",
    "root-marker-bom.yaml": "\ufeff\"--- 'quoted'\"\n",
    "root-marker-multidoc.yaml": "first: value\n...\n\"--- 'quoted'\"\n",
    "unicode-columns.yaml": "café:  世  # 🦀\nlist: [é, 世]\n",
    "bom.yaml": b"\xef\xbb\xbfkey : value\n",
    "bom-marker-line.yaml": "a: a\n...\n\ufeff\na: 1\n",
    "crlf.yaml": b"a : b\r\nlist: [1,2]\r\n",
    "bare-cr.yaml": b"a : b\rlist: [1,2]\r",
    "tags-directives.yaml": (
        "%YAML 1.2\n%TAG !e! tag:example.com,2026:\n---\nkey: !e!thing value\n"
    ),
    "mixed-directives.yaml": (
        "%YAML 1.1\n---\nflag: yes\n...\n%YAML 1.2\n---\nflag: yes\n"
    ),
    "anchors-aliases.yaml": "base: &base {a: 1,b: 2}\ncopy: *base\n",
    "complex-keys.yaml": "? [a, b]\n: {x: 1,y: 2}\n",
    "empty-scalars.yaml": "empty:\n-\n- null\nmap: {a:,b: null}\n",
    "multiline.yaml": (
        'plain: words that continue\n  on a second line\nquoted: "first\n  second"\n'
    ),
    "block-chomping.yaml": "keep: |+\n  text\n\nstrip: >-\n  folded\n  words\n",
    "blank-block-bodies.yaml": "empty: |\nnext: >-\n  \n",
    "invalid.yaml": "key: [unfinished\n",
    "root-marker-start.yaml": "'---'\n",
    "root-marker-end.yaml": "'...'\n",
    "root-marker-trailing.yaml": "'--- x'\n---\n'... # c'\n",
    "root-marker-unicode.yaml": "'---\u00a0x'\n---\n'...\u2003x'\n",
    "root-marker-bom-content.yaml": "'first\n  ---\ufeffx'\n",
    "nested-markers.yaml": "key: '---'\nlist: ['...', '--- x']\n'---': '...'\n",
    "marker-continuation.yaml": "'first\n  --- x'\n",
    "per-document-refusal.yaml": "a : 1\n---\n: v\n",
    "blank-lf.yaml": "\n\n\n",
    "blank-lf-large.yaml": "\n" * 70_000,
    "blank-whitespace.yaml": " \n\t\n\n",
    "blank-crlf.yaml": b"\r\n\r\n",
    "blank-bom.yaml": "﻿\n\n",
    "markdown-host.md": (
        "# Host\n\n```yaml\na : b\n```\n\nOutside stays.\n"
        "\n```yaml\n'---'\n```\n\n```yaml\n'...'\n```\n"
    ),
}
_MAX_REPORTED: Final = 100
_REFUSAL_NOTICE: Final = b"indentation not fixed: cannot re-indent this document safely"


@dataclass(frozen=True)
class Outcome:
    files: dict[str, bytes]
    status: int
    stdout: bytes
    stderr: bytes


def _run(
    binary: Path, cwd: Path, args: list[str], timeout: float
) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(  # ruff: ignore[subprocess-without-shell-equals-true]
        [str(binary), *args], cwd=cwd, capture_output=True, timeout=timeout, check=False
    )


def _files(binary: Path, clone: Path, config: list[str], timeout: float) -> list[str]:
    listed = _run(binary, clone, ["check", "--list-files", *config, "."], timeout)
    if listed.returncode:
        notice = listed.stderr.decode(errors="replace")
        raise RuntimeError(f"{clone}: --list-files exit {listed.returncode}: {notice}")
    return sorted(
        path
        for line in listed.stdout.decode().splitlines()
        if (path := corpus._relative(line)) and (clone / path).is_file()
    )


def _format(
    binary: Path,
    clone: Path,
    files: list[str],
    original: dict[str, bytes],
    config: list[str],
    timeout: float,
    *,
    markdown: bool,
) -> Outcome:
    for path, data in original.items():
        (clone / path).write_bytes(data)
    args = ["format", "--no-cache", *(["--markdown"] if markdown else []), *config]
    if markdown:
        args.extend(files)
    else:
        args.append(".")
    try:
        proc = _run(binary, clone, args, timeout)
        return Outcome(
            corpus._snapshot(clone, files), proc.returncode, proc.stdout, proc.stderr
        )
    finally:
        for path, data in original.items():
            (clone / path).write_bytes(data)


def _compare(label: str, before: Outcome, after: Outcome) -> list[str]:
    differences = [
        f"{label}: {path}: {_byte_difference(before.files[path], after.files[path])}"
        for path in before.files
        if before.files[path] != after.files[path]
    ]
    if before.status != after.status:
        differences.append(
            f"{label}: status differs: {before.status} vs {after.status}"
        )
    if before.stdout != after.stdout:
        differences.append(
            f"{label}: stdout {_byte_difference(before.stdout, after.stdout)}"
        )
    if before.stderr != after.stderr:
        differences.append(
            f"{label}: stderr {_byte_difference(before.stderr, after.stderr)}"
        )
    return differences


def _byte_difference(before: bytes, after: bytes) -> str:
    common = min(len(before), len(after))
    offset = next(
        (index for index in range(common) if before[index] != after[index]), common
    )
    left = before[offset : offset + 48]
    right = after[offset : offset + 48]
    return f"differs at byte {offset}: {left!r} vs {right!r}"


def _case(
    baseline: Path,
    candidate: Path,
    clone: Path,
    files: list[str],
    config: list[str],
    timeout: float,
    label: str,
    *,
    markdown: bool = False,
    expected_notice: bytes | None = None,
) -> list[str]:
    original = corpus._snapshot(clone, files)
    old = _format(baseline, clone, files, original, config, timeout, markdown=markdown)
    new = _format(candidate, clone, files, original, config, timeout, markdown=markdown)
    differences = _compare(label, old, new)
    if expected_notice is not None and expected_notice not in old.stderr:
        differences.append(f"{label}: expected notice missing: {expected_notice!r}")
    if old.status:
        differences.append(f"{label}: baseline formatter exited {old.status}")
    return differences


@app.command()
def run(
    baseline: Annotated[Path, typer.Argument(help="Immutable baseline ryl binary")],
    candidate: Annotated[Path, typer.Argument(help="Immutable candidate ryl binary")],
    cache_dir: Annotated[
        Path, typer.Option(help="Pinned corpus cache")
    ] = corpus._DEFAULT_CACHE,
    repo: Annotated[
        list[str] | None, typer.Option(help="Repeat to select a bounded batch")
    ] = None,
    profile: Annotated[
        list[str] | None, typer.Option(help="Repeat to select profiles")
    ] = None,
    timeout_s: Annotated[float, typer.Option(help="Per command timeout")] = 900.0,
    markdown: Annotated[
        bool, typer.Option(help="Also compare pinned Markdown hosts")
    ] = True,
    fixtures_only: Annotated[
        bool, typer.Option(help="Run only the named fixtures for a quick probe")
    ] = False,
    markdown_only: Annotated[
        bool, typer.Option(help="Run pinned Markdown hosts without YAML repos")
    ] = False,
) -> None:
    """Compare restored inputs, output bytes, exit status and notices.

    Raises:
        BadParameter: A profile name is unknown.
        Exit: The binaries differ.
    """
    baseline, candidate = baseline.resolve(strict=True), candidate.resolve(strict=True)
    chosen = profile or list(PROFILES)
    if unknown := set(chosen) - PROFILES.keys():
        raise typer.BadParameter(f"unknown profile: {', '.join(sorted(unknown))}")
    known_repos = {row.name for row in corpus._manifest(())}
    known_repos.update(row.name for row in corpus._manifest((), markdown=True))
    if unknown := set(repo or ()) - known_repos:
        raise typer.BadParameter(f"unknown repo: {', '.join(sorted(unknown))}")
    differences: list[str] = []
    cases = 0
    with tempfile.TemporaryDirectory(prefix="ryl-parity-") as temp:
        work = Path(temp)
        configs = {}
        for name in chosen:
            if content := PROFILES[name]:
                config = work / f"{name}.toml"
                config.write_text(content, encoding="utf-8")
                configs[name] = ["-c", str(config)]
            else:
                configs[name] = []
        default_fixture_config = work / "fixture-default.toml"
        default_fixture_config.write_text("[format]\n", encoding="utf-8")
        for row in () if fixtures_only or markdown_only else corpus._manifest(()):
            if repo and row.name not in repo:
                continue
            clone = corpus._fetch(row, cache_dir / "repos")
            for name in chosen:
                if name in ("default", "fold") and corpus.Mode(name) not in row.modes:
                    continue
                config = (
                    configs[name] if name != "default" else corpus._lint_args(clone)
                )
                old_files = _files(baseline, clone, config, timeout_s)
                new_files = _files(candidate, clone, config, timeout_s)
                if old_files != new_files:
                    differences.append(f"{row.name} [{name}]: selected files differ")
                    continue
                if not old_files:
                    differences.append(f"{row.name} [{name}]: no files selected")
                    continue
                differences.extend(
                    _case(
                        baseline,
                        candidate,
                        clone,
                        old_files,
                        config,
                        timeout_s,
                        f"{row.name} [{name}]",
                    )
                )
                cases += 1
                typer.echo(f"{row.name} [{name}]: {len(old_files)} files")
        fixture_dirs = {}
        for name, content in FIXTURES.items():
            fixture_dir = work / "fixtures" / name
            fixture_dir.mkdir(parents=True)
            (fixture_dir / name).write_bytes(
                content.encode() if isinstance(content, str) else content
            )
            fixture_dirs[name] = fixture_dir
        for name in () if markdown_only else chosen:
            fixture_config = (
                configs[name]
                if name != "default"
                else ["-c", str(default_fixture_config)]
            )
            for fixture in FIXTURES:
                if fixture.endswith(".md"):
                    continue
                expects_refusal = (
                    name == "default" and fixture == "per-document-refusal.yaml"
                )
                differences.extend(
                    _case(
                        baseline,
                        candidate,
                        fixture_dirs[fixture],
                        [fixture],
                        fixture_config,
                        timeout_s,
                        f"fixture/{fixture} [{name}]",
                        expected_notice=_REFUSAL_NOTICE if expects_refusal else None,
                    )
                )
                cases += 1
            differences.extend(
                _case(
                    baseline,
                    candidate,
                    fixture_dirs["markdown-host.md"],
                    ["markdown-host.md"],
                    fixture_config,
                    timeout_s,
                    f"fixture/markdown-host.md [{name}]",
                    markdown=True,
                )
            )
            cases += 1
        if markdown and not fixtures_only:
            for row in corpus._manifest((), markdown=True):
                if repo and row.name not in repo:
                    continue
                clone = corpus._fetch(row, cache_dir / "markdown-repos")
                files = corpus._markdown_files(row, clone)
                for name in chosen:
                    if (
                        name in ("default", "fold")
                        and corpus.Mode(name) not in row.modes
                    ):
                        continue
                    config = (
                        configs[name] if name != "default" else corpus._lint_args(clone)
                    )
                    for start in range(0, len(files), 100):
                        batch = files[start : start + 100]
                        differences.extend(
                            _case(
                                baseline,
                                candidate,
                                clone,
                                batch,
                                config,
                                timeout_s,
                                f"{row.name} markdown [{name}] batch {start // 100}",
                                markdown=True,
                            )
                        )
                        cases += 1
        for difference in differences[:_MAX_REPORTED]:
            typer.echo(difference, err=True)
        if len(differences) > _MAX_REPORTED:
            typer.echo(
                f"... {len(differences) - _MAX_REPORTED} more differences", err=True
            )
        typer.echo(f"Compared {cases} runs; {len(differences)} differences")
        if differences:
            raise typer.Exit(1)


if __name__ == "__main__":
    app()
