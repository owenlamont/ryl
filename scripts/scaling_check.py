#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.15"
# dependencies = [
#   "typer>=0.16,<1",
# ]
# ///

"""Flag ryl paths whose run time grows faster than linearly with input size.

Each series times one command over growing inputs (min of `--runs` per point) and
fits the log-log slope of time against size; a slope above `--max-slope` is flagged.
Series cover every rule alone on an input dense with its violations, each rule
option at a non-default value, `check`, `check --fix`, `format` and
`format --markdown` on clean and structural stress inputs, and file count.
"""

from collections.abc import Callable, Iterator
from dataclasses import dataclass
import json
import math
from pathlib import Path
import shutil
import statistics
import subprocess
import tempfile
import time
from typing import Annotated, Any, Final, TypedDict

import typer


ROOT: Final = Path(__file__).resolve().parent.parent
MIB: Final = 1024 * 1024
_MARKDOWN_BLOCK_BYTES: Final = 2048
_FILES_PER_DIRECTORY: Final = 500
_COUNT_FILE_BYTES: Final = 1500
app = typer.Typer(
    add_completion=False, context_settings={"help_option_names": ["-h", "--help"]}
)


class CommandFailedError(Exception):
    """A timed command exited with a status it never returns normally."""


# A snippet per rule that violates it, formatted with the repeat index `i`.
DENSE: Final[dict[str, str]] = {
    "anchors": "a{i}: &dup 1\nb{i}: &u{i} 2\nc{i}: *dup\n",
    "block-scalar-chomping": "k{i}: |\n  text\n\n",
    "braces": "k{i}: {{  a: 1  }}\n",
    "brackets": "k{i}: [  1, 2  ]\n",
    "colons": "k{i}  :   v\n",
    "commas": "k{i}: [1 ,2,3]\n",
    "comments": "k{i}: v #c\n#c\n",
    "comments-indentation": "k{i}:\n  a: 1\n    # c\n  b: 2\n",
    "document-end": "---\nk: {i}\n",
    "document-start": "k: {i}\n...\n",
    "empty-lines": "k{i}: v\n\n\n\n",
    "empty-values": "k{i}:\n",
    "float-values": "a{i}: .5\nb{i}: .nan\nc{i}: 1e3\n",
    "hyphens": "k{i}:\n  -   a\n",
    "indentation": "k{i}:\n    - a\nj{i}:\n   b: 1\n",
    "key-duplicates": "k: {i}\n",
    "key-ordering": "b{i}: 1\na{i}: 2\n",
    "line-length": "k{i}: " + "word " * 30 + "\n",
    "merge-keys": "b{i}: &m{i} {{x: 1}}\nc{i}:\n  <<: *m{i}\n  x: 2\n",
    "new-line-at-end-of-file": "k{i}: v\n",
    "new-lines": "k{i}: v\r\n",
    "octal-values": "a{i}: 010\nb{i}: 0o10\n",
    "quoted-strings": "a{i}: 'v'\nb{i}: v\n",
    "tags": "k{i}: !custom v\n",
    "trailing-spaces": "k{i}: v   \n",
    "truthy": "k{i}: yes\nyes: {i}\n",
    "unicode-line-breaks": "k{i}: 'a\u2028b'\n",
}

CLEAN: Final = "key_{i}:\n  name: value {i}\n  list:\n    - a\n    - b\n  flag: true\n"
UNFORMATTED: Final = "key_{i}:   [ a,b ]\nmap_{i}:\n    x:   1\n"

# Structural stress inputs, each a generator of `size` bytes.
STRUCTURE: Final[dict[str, Callable[[int], str]]] = {
    "deep-nesting": lambda size: repeat(
        "r{i}:\n"
        + "".join("  " * depth + f"k{depth}:\n" for depth in range(1, 200))
        + "  " * 200
        + "v: 1\n",
        size,
    ),
    "long-lines": lambda size: "".join(
        f"k{i}: {'x' * 50_000}\n" for i in range(max(1, size // 50_000))
    ),
    "many-documents": lambda size: repeat("---\nk: {i}\n", size),
    "anchors-aliases": lambda size: repeat("a{i}: [&a{i} x, *a{i}, *a{i}]\n", size),
    "big-flow": lambda size: (
        "k: [" + ", ".join(f"{{a: {i}, b: [x, y]}}" for i in range(size // 20)) + "]\n"
    ),
    "long-block-scalar": lambda size: (
        "k: |\n" + repeat("  line {i} of a long block scalar\n", size)
    ),
}


def repeat(snippet: str, size: int) -> str:
    """Repeat `snippet`, numbered, until it reaches `size` bytes.

    Returns:
        The repeated text.
    """
    parts, total, index = [], 0, 0
    while total < size:
        part = snippet.format(i=index)
        parts.append(part)
        total += len(part.encode())
        index += 1
    return "".join(parts)


def markdown(yaml: str) -> str:
    """Wrap `yaml` in fenced blocks of about 2 KiB each.

    Returns:
        A Markdown document.
    """
    blocks, chunk, length = [], [], 0
    for line in yaml.splitlines(keepends=True):
        chunk.append(line)
        length += len(line)
        if length > _MARKDOWN_BLOCK_BYTES and not line.startswith(" "):
            blocks.append("".join(chunk))
            chunk, length = [], 0
    blocks.append("".join(chunk))
    return "".join(f"# Part\n\n```yaml\n{block}```\n\n" for block in blocks if block)


@dataclass(frozen=True, slots=True, kw_only=True)
class Series:
    name: str
    command: tuple[str, ...]
    inputs: Callable[[float], dict[str, str]]
    config: str = ""


class SeriesResult(TypedDict):
    series: str
    slope: float
    timings: list[tuple[float, float]]
    timed_out: float | None


def option_values(*, schema: dict[str, Any], node: dict[str, Any]) -> list[Any]:
    """Leaf values worth trying for a schema option: booleans, enum members, 1.

    Returns:
        The candidate values.
    """
    if "$ref" in node:
        return option_values(
            schema=schema, node=schema["$defs"][node["$ref"].split("/")[-1]]
        )
    if "anyOf" in node:
        return [
            v for alt in node["anyOf"] for v in option_values(schema=schema, node=alt)
        ]
    if "enum" in node:
        return list(node["enum"])
    kinds = node.get("type", [])
    kinds = [kinds] if isinstance(kinds, str) else kinds
    return [True, False] * ("boolean" in kinds) + [1] * ("integer" in kinds)


def rule_options(rule: str) -> Iterator[tuple[str, Any]]:
    """Every `(option, value)` the TOML schema allows for `rule`, one at a time.

    Yields:
        An option name and a value for it.
    """
    schema = json.loads((ROOT / "ryl.toml.schema.json").read_text())
    defs = schema["$defs"]
    entry = schema["$defs"]["RulesTable"]["properties"][rule]["anyOf"][0]["$ref"]
    for alt in defs[entry.split("/")[-1]]["anyOf"]:
        name = alt.get("$ref", "").split("/")[-1]
        for option, node in defs.get(name, {}).get("properties", {}).items():
            if option in {"level", "ignore", "ignore-from-file"}:
                continue
            for value in dict.fromkeys(option_values(schema=schema, node=node)):
                yield option, value


def toml_value(value: Any) -> str:
    """`value` spelled as a TOML literal.

    Returns:
        The TOML text.
    """
    return json.dumps(value) if not isinstance(value, bool) else str(value).lower()


def series(binary: str) -> Iterator[Series]:
    """Every series the sweep times.

    Yields:
        One `Series` per command and input family.
    """

    def sized(
        text_for: Callable[[int], str], name: str = "in.yaml"
    ) -> Callable[[float], dict[str, str]]:
        return lambda mib: {name: text_for(int(mib * MIB))}

    check = (binary, "check", "--no-cache")
    for rule, snippet in DENSE.items():
        dense = sized(lambda size, s=snippet: repeat(s, size))
        yield Series(
            name=f"rule {rule}", command=(*check, "--enable", rule), inputs=dense
        )
        for option, value in rule_options(rule):
            yield Series(
                name=f"rule {rule} {option}={toml_value(value)}",
                command=(*check, "--enable", rule),
                inputs=dense,
                config=f"[lint.rules.{rule}]\n{option} = {toml_value(value)}\n",
            )
    every_rule = "\n".join(DENSE.values())
    families = {
        "clean": sized(lambda size: repeat(CLEAN, size)),
        "unformatted": sized(lambda size: repeat(UNFORMATTED, size)),
        "all-rules-dense": sized(lambda size: repeat(every_rule, size)),
        **{name: sized(make) for name, make in STRUCTURE.items()},
    }
    for family, inputs in families.items():
        yield Series(
            name=f"check ALL {family}",
            command=(*check, "--enable", "ALL"),
            inputs=inputs,
        )
        yield Series(
            name=f"fix ALL {family}",
            command=(*check, "--fix", "--enable", "ALL"),
            inputs=inputs,
        )
        yield Series(
            name=f"format {family}",
            command=(binary, "format", "--no-cache"),
            inputs=inputs,
        )
        yield Series(
            name=f"format --markdown {family}",
            command=(binary, "format", "--no-cache", "--markdown"),
            inputs=lambda mib, inputs=inputs: {
                "in.md": markdown(next(iter(inputs(mib).values())))
            },
        )


def count_series(binary: str) -> Iterator[Series]:
    """Series over file count rather than size, on files of mixed rule snippets.

    Yields:
        One `Series` per command.
    """
    body = repeat("\n".join(DENSE.values()).replace("k: {i}\n", ""), _COUNT_FILE_BYTES)

    def files(count: float) -> dict[str, str]:
        return {
            f"d{n // _FILES_PER_DIRECTORY}/f{n}.yaml": body for n in range(int(count))
        }

    check = (binary, "check", "--no-cache")
    yield Series(
        name="count check ALL", command=(*check, "--enable", "ALL"), inputs=files
    )
    yield Series(
        name="count fix ALL", command=(*check, "--fix", "--enable", "ALL"), inputs=files
    )
    yield Series(
        name="count format", command=(binary, "format", "--no-cache"), inputs=files
    )


def timed(
    *,
    command: list[str],
    work: Path,
    inputs: dict[str, str],
    runs: int,
    limit_seconds: float,
) -> float:
    """Minimum wall time of `command` over fresh copies of `inputs`.

    Returns:
        Seconds; `inf` once a run reaches `limit_seconds`; `nan` when the command
        rejects its configuration.

    Raises:
        CommandFailedError: when the command exits with a code it never returns
            normally.
    """
    allowed = {0} if "format" in command else {0, 1}
    best = math.inf
    for _ in range(runs):
        shutil.rmtree(work, ignore_errors=True)
        for name, text in inputs.items():
            (work / name).parent.mkdir(parents=True, exist_ok=True)
            (work / name).write_text(text, newline="")
        start = time.perf_counter()
        try:
            result = subprocess.run(
                [*command, "."],
                cwd=work,
                capture_output=True,
                check=False,
                timeout=limit_seconds,
            )
        except subprocess.TimeoutExpired:
            return math.inf
        if result.returncode == 2:
            return math.nan
        if result.returncode not in allowed:
            message = f"{command} exited {result.returncode}: {result.stderr[-500:]!r}"
            raise CommandFailedError(message)
        best = min(best, time.perf_counter() - start)
    return best


def slope(points: list[tuple[float, float]]) -> float:
    """Least-squares slope of log time against log size.

    Returns:
        The fitted exponent.
    """
    return statistics.linear_regression(
        [math.log(size) for size, _ in points],
        [math.log(seconds) for _, seconds in points],
    ).slope


@app.command()
def main(
    *,
    binary: Annotated[Path, typer.Option()] = ROOT / "target/release/ryl",
    sizes: Annotated[str, typer.Option(help="MiB per point")] = "0.5,1,2,4,8",
    counts: Annotated[str, typer.Option()] = "100,300,1000,3000,10000",
    runs: Annotated[int, typer.Option()] = 3,
    max_slope: Annotated[float, typer.Option()] = 1.2,
    max_seconds: Annotated[float, typer.Option(help="stop a series past this")] = 20.0,
    only: Annotated[str, typer.Option(help="substring of series names")] = "",
    report: Annotated[Path | None, typer.Option(help="JSON results")] = None,
) -> None:
    """Time each series and flag a log-log slope above `max_slope`.

    Raises:
        Exit: with status 1 when any series is flagged.
    """
    grid = [
        *(
            (s, [float(x) for x in sizes.split(",")])
            for s in series(str(binary.resolve()))
        ),
        *(
            (s, [float(x) for x in counts.split(",")])
            for s in count_series(str(binary.resolve()))
        ),
    ]
    results: list[SeriesResult] = []
    flagged: list[str] = []
    with tempfile.TemporaryDirectory() as scratch:
        for spec, points in grid:
            if only not in spec.name:
                continue
            work = Path(scratch) / "work"
            config = Path(scratch) / "ryl.toml"
            config.write_text(spec.config or "[lint]\n")
            command = [*spec.command[:2], "-c", str(config), *spec.command[2:]]
            timings, timed_out = [], None
            for point in points:
                seconds = timed(
                    command=command,
                    work=work,
                    inputs=spec.inputs(point),
                    runs=runs,
                    limit_seconds=max_seconds,
                )
                if math.isinf(seconds):
                    timed_out = point
                if not math.isfinite(seconds):
                    break
                timings.append((point, seconds))
            if not timings and timed_out is None:
                typer.echo(f"skip  {spec.name}: config rejected")
                continue
            fitted = slope(timings) if len(timings) > 1 else math.nan
            results.append(
                {
                    "series": spec.name,
                    "slope": fitted,
                    "timings": timings,
                    "timed_out": timed_out,
                }
            )
            failed = timed_out is not None or fitted > max_slope
            if failed:
                flagged.append(spec.name)
            cells = " ".join(f"{p:g}:{s * 1000:.0f}ms" for p, s in timings)
            note = "" if timed_out is None else f" timed out at {timed_out:g}"
            mark = "FLAG" if failed else "ok  "
            typer.echo(f"{mark} {fitted:5.2f} {spec.name}  [{cells}]{note}")
    if report:
        report.write_text(json.dumps(results, indent=2))
    typer.echo(f"{len(flagged)} flagged of {len(results)} series")
    raise typer.Exit(1 if flagged else 0)


if __name__ == "__main__":
    app()
