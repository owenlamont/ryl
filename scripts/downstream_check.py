# /// script
# requires-python = ">=3.15"
# dependencies = ["typer", "py-yaml12", "packaging"]
# ///

from collections.abc import Sequence
from dataclasses import dataclass
import os
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib
from typing import Annotated

from packaging.version import Version
import typer
from yaml12 import parse_yaml


app = typer.Typer(
    add_completion=False, context_settings={"help_option_names": ["-h", "--help"]}
)


@dataclass(frozen=True, slots=True, kw_only=True)
class Pin:
    version: str
    hooks: tuple[str, ...]


def _release_version(*, revision: str, repo: str) -> str:
    if not re.fullmatch(r"[0-9a-fA-F]{40}", revision):
        return str(Version(revision.removeprefix("v")))
    refs = subprocess.run(
        ["git", "ls-remote", "--tags", repo, "v[0-9]*"],
        capture_output=True,
        text=True,
        check=True,
        timeout=60,
    ).stdout
    versions = [
        Version(ref.removeprefix("refs/tags/v").removesuffix("^{}"))
        for sha, ref in map(str.split, refs.splitlines())
        if sha.lower() == revision.lower()
    ]
    if not versions:
        raise ValueError(f"{repo}: frozen revision {revision} has no release tag")
    return str(max(versions))


def read_pin(config: Path) -> Pin:
    """Return the ryl-pre-commit release pin and hook IDs.

    Frozen revisions are resolved against upstream release tags.

    Raises:
        ValueError: The pin is missing, ambiguous or not a version.
    """
    raw = config.read_text()
    data = tomllib.loads(raw) if config.suffix == ".toml" else parse_yaml(raw)
    matches = [
        repo
        for repo in data["repos"]
        if repo["repo"]
        .removesuffix(".git")
        .rstrip("/")
        .endswith("/owenlamont/ryl-pre-commit")
    ]
    if len(matches) != 1:
        raise ValueError(f"{config}: expected exactly one ryl-pre-commit pin")
    repo = matches[0]
    version = _release_version(revision=repo["rev"], repo=repo["repo"])
    return Pin(version=version, hooks=tuple(hook["id"] for hook in repo["hooks"]))


def published_binary(*, version: str, root: Path) -> Path:
    """Return the exact PyPI release binary from an isolated uv tool install."""
    version = str(Version(version.removeprefix("v")))
    destination = root / version
    executable = destination / "bin" / ("ryl.exe" if os.name == "nt" else "ryl")
    environment = os.environ | {
        "UV_TOOL_DIR": str(destination / "tools"),
        "UV_TOOL_BIN_DIR": str(destination / "bin"),
    }
    subprocess.run(
        ["uv", "tool", "install", f"ryl=={version}"],
        env=environment,
        capture_output=True,
        text=True,
        check=True,
        timeout=300,
    )
    return executable.resolve(strict=True)


def _run(
    *, binary: Path, arguments: Sequence[str], repo: Path
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(binary), *arguments],
        cwd=repo,
        env=os.environ | {"RYL_NO_CACHE": "1", "NO_COLOR": "1"},
        capture_output=True,
        text=True,
        check=False,
        timeout=300,
    )


def _diagnostics(result: subprocess.CompletedProcess[str]) -> dict[str, str]:
    return {
        line: match[1]
        for line in (result.stdout + result.stderr).splitlines()
        if (match := re.match(r"^(.+):\d+:\d+: ", line))
    }


@app.command()
def main(
    repos: Annotated[
        list[Path], typer.Argument(help="Downstream checkout paths (read only).")
    ],
    *,
    ryl_bin: Annotated[Path, typer.Option(exists=True, dir_okay=False)],
) -> None:
    """Compare whole-repo checks using each repo's config, without hook file filters.

    Raw output includes every warning/notice and formatter filename; exit 1 means
    new diagnostics, formatter changes, or a check/config failure needs review.

    Raises:
        typer.BadParameter: A repo has no hook configuration.
        typer.Exit: The report is complete (status 1 requires review).
    """
    candidate = ryl_bin.resolve()
    failed = False
    with tempfile.TemporaryDirectory(prefix="ryl-downstream-") as temporary:
        for repo in repos:
            repo = repo.resolve(strict=True)
            config = next(
                (
                    repo / name
                    for name in (
                        "prek.toml",
                        ".pre-commit-config.yaml",
                        ".pre-commit-config.yml",
                    )
                    if (repo / name).is_file()
                ),
                None,
            )
            if config is None:
                raise typer.BadParameter(f"{repo}: no prek/pre-commit config")
            pin = read_pin(config)
            previous = published_binary(version=pin.version, root=Path(temporary))
            typer.echo(f"\n## {repo} (pinned {pin.version})")
            typer.echo(
                _run(
                    binary=candidate, arguments=["--version"], repo=repo
                ).stdout.strip()
            )
            commands = [["check", "--output-format", "parsable", "."]]
            if any(
                hook in pin.hooks for hook in ("ryl-markdown", "ryl-check-markdown")
            ):
                commands.append(
                    ["check", "--markdown", "--output-format", "parsable", "."]
                )
            commands.extend(
                [
                    "format",
                    "--check",
                    "--output-format",
                    "parsable",
                    *(["--markdown"] if hook.endswith("-markdown") else []),
                    ".",
                ]
                for hook in ("ryl-format", "ryl-format-markdown")
                if hook in pin.hooks
            )
            for arguments in commands:
                before = _run(binary=previous, arguments=arguments, repo=repo)
                after = _run(binary=candidate, arguments=arguments, repo=repo)
                typer.echo(f"\n### {' '.join(arguments)}")
                for label, result in (("Pinned", before), ("Candidate", after)):
                    typer.echo(
                        f"{label}: exit {result.returncode}\n"
                        f"stdout:\n{result.stdout}stderr:\n{result.stderr}"
                    )
                before_diagnostics = _diagnostics(before)
                after_diagnostics = _diagnostics(after)
                new = after_diagnostics.keys() - before_diagnostics.keys()
                removed = before_diagnostics.keys() - after_diagnostics.keys()
                if arguments[0] == "format":
                    for label, diagnostics in (
                        ("Pinned", before_diagnostics),
                        ("Candidate", after_diagnostics),
                    ):
                        files = "\n".join(sorted(set(diagnostics.values())))
                        typer.echo(f"{label} formatter files:\n{files or '(none)'}")
                typer.echo("New diagnostics:\n" + ("\n".join(sorted(new)) or "(none)"))
                typer.echo(
                    "Removed diagnostics:\n" + ("\n".join(sorted(removed)) or "(none)")
                )
                failed |= (
                    bool(new)
                    or after.returncode not in (0, 1)
                    or before.returncode not in (0, 1)
                    or (arguments[0] == "format" and after.returncode != 0)
                )
    raise typer.Exit(int(failed))


if __name__ == "__main__":
    app()
