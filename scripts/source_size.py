#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.10"
# dependencies = [
#   "gitpython>=3.1,<4",
#   "typer>=0.16,<1",
# ]
# ///

from __future__ import annotations

from contextlib import contextmanager, suppress
from dataclasses import asdict, dataclass
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
from typing import Annotated, Any

from git import InvalidGitRepositoryError, NoSuchPathError, Repo
import typer


@dataclass(frozen=True)
class FileMetrics:
    path: str
    root: str
    bytes: int
    lines: int
    code: int
    doc: int
    comment: int


app = typer.Typer(add_completion=False)


def normalize_extensions(raw_extensions: list[str]) -> set[str]:
    """Normalize extension filters so each one starts with a dot.

    Returns:
        A normalized set of extensions.
    """
    extensions = set()
    for extension in raw_extensions:
        normalized = extension if extension.startswith(".") else f".{extension}"
        extensions.add(normalized)
    return extensions


def resolve_roots(raw_roots: list[str] | None) -> list[str]:
    """Resolve the effective root list for the report.

    Returns:
        The root directories to scan.
    """
    return raw_roots or ["src", "tests"]


def iter_files(root: Path, extensions: set[str]) -> list[Path]:
    """Return recursively discovered files under a root, optionally filtered."""
    files = [path for path in root.rglob("*") if path.is_file()]
    if not extensions:
        return sorted(files)
    return sorted(path for path in files if path.suffix in extensions)


def count_lines(data: bytes) -> int:
    """Count logical lines from raw bytes without assuming an encoding.

    Returns:
        The number of logical lines in the file.
    """
    if not data:
        return 0
    return data.count(b"\n") + (0 if data.endswith(b"\n") else 1)


def collect_comment_lines(
    base_dir: Path, roots: list[str]
) -> dict[str, tuple[int, int, int]]:
    """Per-file `(code, doc, comment)` line counts from tokei.

    Doc comments (`///`/`//!`) are filed by tokei under an embedded-Markdown blob,
    while plain `//` and `#` stay under the language's own `comments`; Python
    docstrings are code to tokei (ruff governs their format), so they are not counted.

    Returns:
        A map from POSIX path (relative to `base_dir`) to `(code, doc, comment)`
        lines, empty when tokei is unavailable.
    """
    if shutil.which("tokei") is None:
        return {}
    result = subprocess.run(
        ["tokei", *roots, "--output", "json"],
        cwd=base_dir,
        capture_output=True,
        text=True,
        check=True,
    )
    report = json.loads(result.stdout)
    metrics: dict[str, tuple[int, int, int]] = {}
    for language, summary in report.items():
        if language == "Total":
            continue
        for entry in summary["reports"]:
            stats = entry["stats"]
            markdown = stats.get("blobs", {}).get("Markdown", {})
            doc = markdown.get("comments", 0) + markdown.get("code", 0)
            name = entry["name"].replace("\\", "/").removeprefix("./")
            metrics[name] = (stats["code"], doc, stats["comments"])
    return metrics


def require_tokei() -> None:
    """Exit with guidance when tokei is needed but absent.

    Raises:
        SystemExit: When tokei is not on `PATH`.
    """
    if shutil.which("tokei") is None:
        raise SystemExit(
            "tokei is required for comment metrics; install it (e.g. "
            "`pixi global install tokei`)"
        )


def collect_metrics(
    base_dir: Path,
    root: Path,
    extensions: set[str],
    comment_lines: dict[str, tuple[int, int, int]],
) -> list[FileMetrics]:
    """Collect byte, line, and comment counts for files under a root.

    Returns:
        Per-file metrics for the requested root.
    """
    metrics: list[FileMetrics] = []
    for path in iter_files(root, extensions):
        data = path.read_bytes()
        rel = path.relative_to(base_dir).as_posix()
        code, doc, comment = comment_lines.get(rel, (0, 0, 0))
        metrics.append(
            FileMetrics(
                path=rel,
                root=root.name,
                bytes=len(data),
                lines=count_lines(data),
                code=code,
                doc=doc,
                comment=comment,
            )
        )
    return metrics


def collect_metrics_by_root(
    base_dir: Path, roots: list[str], extensions: set[str]
) -> dict[str, list[FileMetrics]]:
    """Collect metrics for each requested root relative to a base directory.

    Returns:
        Per-root metrics for the requested directory tree.

    Raises:
        SystemExit: If any requested root is missing or not a directory.
    """
    comment_lines = collect_comment_lines(base_dir, roots)
    metrics_by_root: dict[str, list[FileMetrics]] = {}
    for raw_root in roots:
        root = base_dir / raw_root
        if not root.exists():
            raise SystemExit(f"root does not exist: {raw_root}")
        if not root.is_dir():
            raise SystemExit(f"root is not a directory: {raw_root}")
        metrics_by_root[raw_root] = collect_metrics(
            base_dir, root, extensions, comment_lines
        )
    return metrics_by_root


def format_kib(size_bytes: int) -> str:
    """Format a byte count in kibibytes for table output.

    Returns:
        The size rendered in KiB with one decimal place.
    """
    return f"{size_bytes / 1024:.1f}"


def comment_ratio(comment: int, doc: int, code: int) -> float:
    """Comment-to-code line ratio (doc + plain comments over code).

    Returns:
        The ratio, or 0.0 when there is no code.
    """
    return (comment + doc) / code if code else 0.0


def format_ratio(comment: int, doc: int, code: int) -> str:
    """Format a comment-to-code ratio as a percentage.

    Returns:
        The ratio rendered as a percentage with one decimal place.
    """
    return f"{comment_ratio(comment, doc, code) * 100:.1f}%"


def format_delta(value: int) -> str:
    """Format a signed delta for text output.

    Returns:
        The signed integer as a string.
    """
    return f"{value:+d}"


def format_delta_kib(value: int) -> str:
    """Format a signed byte delta in kibibytes for text output.

    Returns:
        The signed size rendered in KiB with one decimal place.
    """
    return f"{value / 1024:+.1f}"


def build_report(metrics_by_root: dict[str, list[FileMetrics]]) -> dict[str, Any]:
    """Build the aggregate report structure used by both output modes.

    Returns:
        Aggregate totals and per-file metrics for rendering.
    """
    per_root = []
    all_metrics: list[FileMetrics] = []
    for root, metrics in metrics_by_root.items():
        all_metrics.extend(metrics)
        per_root.append(
            {
                "root": root,
                "files": len(metrics),
                "bytes": sum(metric.bytes for metric in metrics),
                "lines": sum(metric.lines for metric in metrics),
                "code": sum(metric.code for metric in metrics),
                "doc": sum(metric.doc for metric in metrics),
                "comment": sum(metric.comment for metric in metrics),
            }
        )

    totals = {
        key: sum(entry[key] for entry in per_root)
        for key in ("files", "bytes", "lines", "code", "doc", "comment")
    }
    largest_files = sorted(
        (asdict(metric) for metric in all_metrics),
        key=lambda metric: (-metric["bytes"], metric["path"]),
    )

    return {"roots": per_root, "totals": totals, "largest_files": largest_files}


def build_diff_report(
    current_report: dict[str, Any], baseline_report: dict[str, Any]
) -> dict[str, Any]:
    """Build a diff report between current and baseline size reports.

    Returns:
        Aggregate and per-file deltas between two reports.
    """
    empty_root = {"files": 0, "lines": 0, "bytes": 0, "code": 0, "doc": 0, "comment": 0}
    empty_file = {"lines": 0, "bytes": 0, "code": 0, "doc": 0, "comment": 0}
    count_keys = ("files", "lines", "bytes", "code", "doc", "comment")
    current_roots = {entry["root"]: entry for entry in current_report["roots"]}
    baseline_roots = {entry["root"]: entry for entry in baseline_report["roots"]}
    all_roots = sorted(set(current_roots) | set(baseline_roots))

    root_deltas = []
    for root in all_roots:
        current = current_roots.get(root, empty_root)
        baseline = baseline_roots.get(root, empty_root)
        root_deltas.append(
            {"root": root, **{key: current[key] - baseline[key] for key in count_keys}}
        )

    current_files = {entry["path"]: entry for entry in current_report["largest_files"]}
    baseline_files = {
        entry["path"]: entry for entry in baseline_report["largest_files"]
    }
    file_deltas = []
    for path in sorted(set(current_files) | set(baseline_files)):
        current = current_files.get(path, {"root": path.split("/", 1)[0], **empty_file})
        baseline = baseline_files.get(
            path, {"root": path.split("/", 1)[0], **empty_file}
        )
        deltas = {key: current[key] - baseline[key] for key in count_keys[1:]}
        if all(value == 0 for value in deltas.values()):
            continue
        file_deltas.append(
            {"path": path, "root": current.get("root", baseline.get("root")), **deltas}
        )

    file_deltas.sort(key=lambda entry: (-abs(entry["bytes"]), entry["path"]))

    return {
        "roots": root_deltas,
        "totals": {
            key: current_report["totals"][key] - baseline_report["totals"][key]
            for key in count_keys
        },
        "files": file_deltas,
    }


def find_repo(start_dir: Path) -> Repo:
    """Resolve the Git repository for the current working tree.

    Returns:
        The repository object for the current working tree.

    Raises:
        SystemExit: If the current directory is not inside a Git repository.
    """
    try:
        return Repo(start_dir, search_parent_directories=True)
    except (InvalidGitRepositoryError, NoSuchPathError) as exc:
        raise SystemExit("failed to resolve git repository root") from exc


@contextmanager
def temporary_worktree(repo: Repo, git_ref: str):
    """Create and clean up a temporary detached worktree for a Git ref.

    Yields:
        The path to the temporary baseline worktree.
    """
    with tempfile.TemporaryDirectory(prefix="source-size-") as temp_dir:
        worktree_dir = Path(temp_dir) / "baseline"
        try:
            repo.git.worktree("add", "--detach", str(worktree_dir), git_ref)
            yield worktree_dir
        finally:
            with suppress(Exception):
                repo.git.worktree("remove", "--force", str(worktree_dir))


def print_text(report: dict[str, Any], top_n: int) -> None:
    """Print a human-readable table for the collected size report."""
    print("Root           Files      Lines      Bytes      KiB")
    for root in report["roots"]:
        print(
            f"{root['root']:<12} "
            f"{root['files']:>5} "
            f"{root['lines']:>10} "
            f"{root['bytes']:>10} "
            f"{format_kib(root['bytes']):>8}"
        )

    totals = report["totals"]
    print(
        f"{'TOTAL':<12} "
        f"{totals['files']:>5} "
        f"{totals['lines']:>10} "
        f"{totals['bytes']:>10} "
        f"{format_kib(totals['bytes']):>8}"
    )

    print_comments(report)

    if top_n <= 0:
        return

    print()
    print(f"Top {min(top_n, len(report['largest_files']))} Largest Files")
    print("Path                                             Lines      Bytes      KiB")
    for metric in report["largest_files"][:top_n]:
        print(
            f"{metric['path']:<48.48} "
            f"{metric['lines']:>10} "
            f"{metric['bytes']:>10} "
            f"{format_kib(metric['bytes']):>8}"
        )


def print_comments(report: dict[str, Any]) -> None:
    """Print the per-root code/comment breakdown when tokei data is present."""
    totals = report["totals"]
    if totals["code"] == 0 and totals["doc"] == 0 and totals["comment"] == 0:
        return
    print()
    print("Comments       Code        Doc    Comment   Cmt%")
    for root in report["roots"]:
        print(
            f"{root['root']:<12} "
            f"{root['code']:>8} "
            f"{root['doc']:>8} "
            f"{root['comment']:>8} "
            f"{format_ratio(root['comment'], root['doc'], root['code']):>6}"
        )
    print(
        f"{'TOTAL':<12} "
        f"{totals['code']:>8} "
        f"{totals['doc']:>8} "
        f"{totals['comment']:>8} "
        f"{format_ratio(totals['comment'], totals['doc'], totals['code']):>6}"
    )


def print_diff_text(diff_report: dict[str, Any], git_ref: str, top_n: int) -> None:
    """Print a human-readable diff table against a Git baseline."""
    print(f"Delta Vs {git_ref}")
    print("Root           Files      Lines      Bytes    KiB Δ")
    for root in diff_report["roots"]:
        print(
            f"{root['root']:<12} "
            f"{format_delta(root['files']):>5} "
            f"{format_delta(root['lines']):>10} "
            f"{format_delta(root['bytes']):>10} "
            f"{format_delta_kib(root['bytes']):>8}"
        )

    totals = diff_report["totals"]
    print(
        f"{'TOTAL':<12} "
        f"{format_delta(totals['files']):>5} "
        f"{format_delta(totals['lines']):>10} "
        f"{format_delta(totals['bytes']):>10} "
        f"{format_delta_kib(totals['bytes']):>8}"
    )

    print_comments_diff(diff_report)

    if top_n <= 0:
        return

    if not diff_report["files"]:
        print()
        print("No file deltas.")
        return

    print()
    print(f"Top {min(top_n, len(diff_report['files']))} File Deltas")
    print(
        "Path                                             Lines Δ    Bytes Δ    KiB Δ"
    )
    for metric in diff_report["files"][:top_n]:
        print(
            f"{metric['path']:<48.48} "
            f"{format_delta(metric['lines']):>10} "
            f"{format_delta(metric['bytes']):>10} "
            f"{format_delta_kib(metric['bytes']):>8}"
        )


def print_comments_diff(diff_report: dict[str, Any]) -> None:
    """Print per-root code/comment-line deltas when tokei data is present."""
    totals = diff_report["totals"]
    if totals["code"] == 0 and totals["doc"] == 0 and totals["comment"] == 0:
        return
    print()
    print("Comments Δ     Code        Doc    Comment")
    for root in diff_report["roots"]:
        print(
            f"{root['root']:<12} "
            f"{format_delta(root['code']):>8} "
            f"{format_delta(root['doc']):>8} "
            f"{format_delta(root['comment']):>8}"
        )
    print(
        f"{'TOTAL':<12} "
        f"{format_delta(totals['code']):>8} "
        f"{format_delta(totals['doc']):>8} "
        f"{format_delta(totals['comment']):>8}"
    )


def enforce_comment_ratio(report: dict[str, Any], max_ratio: float) -> None:
    """Fail when the aggregate comment-to-code ratio exceeds `max_ratio`.

    Raises:
        SystemExit: When the ratio over all scanned roots exceeds the limit.
    """
    totals = report["totals"]
    ratio = comment_ratio(totals["comment"], totals["doc"], totals["code"])
    if ratio > max_ratio:
        raise SystemExit(
            f"comment ratio {ratio * 100:.1f}% exceeds the {max_ratio * 100:.1f}% "
            f"limit ({totals['comment'] + totals['doc']} comment lines over "
            f"{totals['code']} code lines)"
        )


@app.command()
def main(
    roots: Annotated[
        list[str] | None,
        typer.Argument(help="Directories to scan recursively. Defaults to: src tests."),
    ] = None,
    ext: Annotated[
        list[str] | None,
        typer.Option(help="Limit results to file extensions such as .rs or .py."),
    ] = None,
    top: Annotated[
        int,
        typer.Option(help="How many largest files to show. Use 0 to hide the table."),
    ] = 15,
    compare_to: Annotated[
        str | None,
        typer.Option(
            "--compare-to", help="Git branch, tag, or commit SHA to compare against."
        ),
    ] = None,
    max_comment_ratio: Annotated[
        float | None,
        typer.Option(
            "--max-comment-ratio",
            help="Exit non-zero if comment lines exceed this fraction of code lines "
            "(e.g. 0.12). Requires tokei.",
        ),
    ] = None,
    as_json: Annotated[
        bool,
        typer.Option("--json", help="Print the report as JSON instead of plain text."),
    ] = False,
) -> int:
    """Run the size report CLI.

    Returns:
        Zero on success.
    """
    if max_comment_ratio is not None:
        require_tokei()
    current_dir = Path.cwd()
    selected_roots = resolve_roots(roots)
    extensions = normalize_extensions(ext or [])
    report = build_report(
        collect_metrics_by_root(current_dir, selected_roots, extensions)
    )

    if compare_to is None:
        if as_json:
            print(json.dumps(report, indent=2, sort_keys=True))
        else:
            print_text(report, top)
        if max_comment_ratio is not None:
            enforce_comment_ratio(report, max_comment_ratio)
        return 0

    repo = find_repo(current_dir)
    with temporary_worktree(repo, compare_to) as baseline_dir:
        baseline_report = build_report(
            collect_metrics_by_root(baseline_dir, selected_roots, extensions)
        )

    diff_report = build_diff_report(report, baseline_report)
    comparison = {
        "baseline_ref": compare_to,
        "baseline": baseline_report,
        "current": report,
        "delta": diff_report,
    }
    if as_json:
        print(json.dumps(comparison, indent=2, sort_keys=True))
    else:
        print_text(report, top)
        print()
        print_diff_text(diff_report, compare_to, top)
    if max_comment_ratio is not None:
        enforce_comment_ratio(report, max_comment_ratio)
    return 0


if __name__ == "__main__":
    app()
