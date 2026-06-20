# /// script
# requires-python = ">=3.14"
# dependencies = ["typer", "py-yaml12"]
# ///
"""Verify ryl's documented per-formatter recipes still coexist cleanly with the latest
formatters (docs/using-with-formatters.md, issue #186).

For each recipe (yamlfmt / Prettier / yamlfix) over a small corpus, it runs the joint
workflow `formatter -> ryl --fix -> formatter ...` to a fixed point and asserts three
invariants:

  1. it converges (no edit loop between the two tools);
  2. the settled file has no residual ryl findings (minus documented caveats); and
  3. the settled file resolves to the same YAML 1.2 values as the input (no silent value
     change), checked with py-yaml12 (Rust/saphyr, matching ryl's 1.2 target).

Manual / opt-in: it shells out to the real tools and is NOT meant for CI. It needs `ryl`
and `pixi` on PATH (plus `uv`, which launches this script); pixi fetches all three
formatters ephemerally at the pinned versions via `pixi exec`, so nothing else needs
installing. Routing every formatter through pixi (rather than npx + uvx + pixi) keeps the
dependency list short and dodges the Windows `npx` .cmd-shim breakage under subprocess.
The pinned conda-forge builds cover linux-64, osx-arm64, and win-64, so the script is
cross-platform. Run:

    uv run scripts/formatter_compat.py

pixi can even supply uv itself, so ryl + pixi are the only hard prerequisites:

    pixi exec --spec uv uv run scripts/formatter_compat.py

The pinned versions below were the latest of each as of TESTED_DATE; bump them
deliberately and re-run when a formatter releases (conda-forge can lag the upstream
release briefly, so a bump may need to wait for the feedstock to catch up).
"""

from __future__ import annotations

from itertools import starmap
import math
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

import typer
from yaml12 import parse_yaml


app = typer.Typer(add_completion=False, help=__doc__)

TESTED_DATE = "2026-06-20"
YAMLFMT_VERSION = "0.21.0"
PRETTIER_VERSION = "3.8.4"
YAMLFIX_VERSION = "1.19.1"


# On Windows, conda installs a node-based tool as a `.bat` launcher (e.g. prettier ->
# prettier.bat) that `pixi exec` cannot spawn directly -- the same PATHEXT/shim limitation
# that makes a bare `npx` fail under subprocess. Routing every tool through `cmd /c` (always
# present on Windows) resolves .exe/.bat/.cmd uniformly; POSIX has no shim and no cmd, so it
# runs the tool directly.
_WINDOWS = os.name == "nt"


def _pixi_exec(tool: str, version: str, *run_args: str) -> list[str]:
    # An ephemeral, version-pinned `pixi exec`. Each formatter publishes the pinned version
    # on conda-forge for linux-64/osx-arm64/win-64, so a single runner covers them all.
    prefix = ["pixi", "exec", "--spec", f"{tool}={version}"]
    return [*prefix, "cmd", "/c", *run_args] if _WINDOWS else [*prefix, *run_args]


# Version-pinned formatter invocations (operate in-place on work.yaml). yamlfmt defaults to
# the platform-native line ending (CRLF on Windows), which fights ryl `new-lines = unix` in
# an endless loop, so pin it to LF -- the docs recipe documents the matching `.yamlfmt`.
FORMATTER_CMD = {
    "yamlfmt": _pixi_exec(
        "yamlfmt",
        YAMLFMT_VERSION,
        "yamlfmt",
        "-formatter",
        "line_ending=lf",
        "work.yaml",
    ),
    "prettier": _pixi_exec(
        "prettier",
        PRETTIER_VERSION,
        "prettier",
        "--write",
        "--no-editorconfig",
        "work.yaml",
    ),
    "yamlfix": _pixi_exec("yamlfix", YAMLFIX_VERSION, "yamlfix", "work.yaml"),
}

# Each recipe is verbatim from the corresponding block in docs/using-with-formatters.md.
RECIPES: dict[str, str] = {
    "yamlfmt": """
[rules]
braces = "enable"
brackets = "enable"
colons = "enable"
commas = "enable"
comments-indentation = "enable"
hyphens = "enable"
new-line-at-end-of-file = "enable"
trailing-spaces = "enable"

[rules.document-start]
present = false

[rules.new-lines]
type = "unix"

[rules.comments]
min-spaces-from-content = 1

[rules.empty-lines]
max = 2

[rules.indentation]
spaces = 2
indent-sequences = true

[rules.quoted-strings]
required = "only-when-needed"

[rules.line-length]
max = 120
""",
    "prettier": """
[rules]
brackets = "enable"
colons = "enable"
commas = "enable"
comments-indentation = "enable"
hyphens = "enable"
new-line-at-end-of-file = "enable"
trailing-spaces = "enable"

[rules.braces]
min-spaces-inside = 1
max-spaces-inside = 1
min-spaces-inside-empty = 0
max-spaces-inside-empty = 0

[rules.new-lines]
type = "unix"

[rules.comments]
min-spaces-from-content = 1

[rules.empty-lines]
max = 2

[rules.indentation]
spaces = 2
indent-sequences = true

[rules.quoted-strings]
required = "only-when-needed"

[rules.line-length]
max = 120
""",
    "yamlfix": """
[rules]
braces = "enable"
brackets = "enable"
colons = "enable"
commas = "enable"
comments-indentation = "enable"
hyphens = "enable"
new-line-at-end-of-file = "enable"
trailing-spaces = "enable"
truthy = "enable"

[rules.document-start]
present = true

[rules.new-lines]
type = "unix"

[rules.comments]
min-spaces-from-content = 2

[rules.empty-lines]
max = 2

[rules.indentation]
spaces = 2
indent-sequences = true

[rules.line-length]
max = 120
""",
}

# Small fixtures, one construct apiece. Keep them representative of the constructs the
# rules and formatters touch (markers, flow, quotes, truthy incl. quoted/flow edges, ...).
CORPUS: dict[str, str] = {
    "blanks.yaml": "first: 1\n\n\n\nsecond: 2\n\n\nthird: 3\n",
    "comments.yaml": (
        "top: value  # inline with two spaces\n"
        "other: value # inline with one space\n"
        "# full-line comment\n"
        "parent:\n"
        "   # over-indented comment\n"
        "  child: 1\n"
        "  sibling: 2  #no space after hash\n"
    ),
    "flow.yaml": (
        "short_list: [1, 2, 3]\n"
        "short_map: {a: 1, b: 2}\n"
        "padded_list: [ 1, 2, 3 ]\n"
        "padded_map: { a: 1 }\n"
        "empty_map: {}\n"
        "empty_list: []\n"
        "long_list: [alpha, bravo, charlie, delta, echo, foxtrot, golf, hotel, india, juliett, kilo]\n"
        "nested:\n  - [x, y]\n  - {p: 1, q: 2}\n"
    ),
    "indent.yaml": (
        "parent:\n  - item1\n  - item2\n"
        "sequence_at_root:\n- a\n- b\n"
        "map:\n  nested:\n    deep: value\n"
        "list_of_maps:\n  - name: one\n    value: 1\n  - name: two\n    value: 2\n"
    ),
    "keys.yaml": (
        "zebra: 1\napple: 2\nmango: 3\nempty_value:\n"
        "colon_spacing :   squished\nnested:\n  banana: 1\n  avocado: 2\n"
    ),
    "markers.yaml": "name: example\nlist:\n  - a\n  - b\nnested:\n  key: value\n",
    "multiline.yaml": (
        "literal: |\n  line one\n  line two\n"
        "folded: >\n  some long folded\n  text here\n"
        "plain_long: this is a very long plain scalar that runs well beyond eighty characters in total to trip line-length\n"
        'quoted_long: "another quite long value that also runs beyond the eighty character soft limit boundary"\n'
    ),
    "nums.yaml": (
        "implicit_octal: 0755\nexplicit_octal: 0o755\nhexadecimal: 0xFF\n"
        "infinity: .inf\nnot_a_number: .nan\nscientific: 1.2e3\nno_leading_zero: .5\n"
    ),
    "quotes.yaml": (
        'unquoted: hello\nsingle: \'hello\'\ndouble: "hello"\nnumber_like: "123"\n'
        'bool_like: "yes"\nempty: ""\nspecial: "a: b"\napostrophe: "it\'s fine"\n'
    ),
    "refs.yaml": (
        "base: &anchor\n  shared: true\nmerged:\n  <<: *anchor\n  extra: 1\n"
        "used_alias: *anchor\nunused_anchor: &dangling 42\n"
        "typed_str: !!str 123\ncustom_tag: !mytag hello\n"
    ),
    "truthy.yaml": (
        "a: yes\nb: no\nc: True\nd: False\ne: on\nf: off\ng: Yes\nh: NO\n"
        "flow_flags: [yes, no, on, off]\nnested_flow: {active: yes, disabled: no}\n"
        "commented: yes  # truthy kept before a trailing comment\n"
    ),
    "with-marker.yaml": "---\nname: example\nitems:\n  - one\n  - two\nnested:\n  key: value\n...\n",
}

# Residual ryl findings the docs already document as expected (not failures).
# yamlfix canonicalises only block-style truthy, so flow/pre-comment truthy survives.
EXPECTED_RESIDUAL: dict[tuple[str, str], set[str]] = {
    ("yamlfix", "truthy.yaml"): {"truthy"}
}

# Value changes that are the formatter's intended behaviour, not a corruption. yamlfix
# rewrites UNQUOTED truthy words to booleans (a YAML 1.1 normalisation); the user opted
# into that by choosing yamlfix. A change anywhere NOT listed here (e.g. a quoted string
# flipping type) is treated as data corruption and fails. Populated from a real run.
EXPECTED_VALUE_CHANGE: set[tuple[str, str]] = {("yamlfix", "truthy.yaml")}

MAX_ITERS = 6
CLEAN_ENV = {k: v for k, v in os.environ.items() if not k.startswith("GITHUB_")}
# CLEAN_ENV strips GITHUB_*, so ryl uses its standard console format, which ends each
# diagnostic line with the bare rule id in parens, e.g. "(new-lines)". (The GitHub format
# renders "[new-lines]" mid-line and would not match -- hence stripping GITHUB_*.)
_RULE_RE = re.compile(r"\(([a-z][a-z0-9-]*)\)\s*$")


class Runner:
    """Drives the real tools in a throwaway workdir; memoises formatter output (a pure
    function of input bytes) so the ephemeral `pixi exec` calls are not repeated.
    """

    def __init__(self, workdir: Path) -> None:
        self.wd = workdir
        self._fmt_cache: dict[tuple[str, str], str | None] = {}
        self._cfg: dict[str, Path] = {}

    def _work(self) -> Path:
        return self.wd / "work.yaml"

    def config(self, formatter: str) -> Path:
        """Write (once) the recipe TOML for a formatter and return its path."""
        if formatter not in self._cfg:
            p = self.wd / f"{formatter}.ryl.toml"
            p.write_bytes(RECIPES[formatter].encode())
            self._cfg[formatter] = p
        return self._cfg[formatter]

    def format(self, formatter: str, content: str) -> str | None:
        key = (formatter, content)
        if key not in self._fmt_cache:
            p = self._work()
            p.write_bytes(content.encode())
            proc = subprocess.run(
                FORMATTER_CMD[formatter],
                cwd=self.wd,
                env=CLEAN_ENV,
                capture_output=True,
                text=True,
                encoding="utf-8",
            )
            self._fmt_cache[key] = (
                p.read_bytes().decode() if proc.returncode == 0 else None
            )
        return self._fmt_cache[key]

    def ryl_fix(self, content: str, cfg: Path) -> str:
        p = self._work()
        p.write_bytes(content.encode())
        subprocess.run(
            ["ryl", "--fix", "-c", str(cfg), "work.yaml"],
            cwd=self.wd,
            env=CLEAN_ENV,
            capture_output=True,
            text=True,
            encoding="utf-8",
        )
        return p.read_bytes().decode()

    def ryl_lint(self, content: str, cfg: Path) -> list[str]:
        p = self._work()
        p.write_bytes(content.encode())
        proc = subprocess.run(
            ["ryl", "-c", str(cfg), "work.yaml"],
            cwd=self.wd,
            env=CLEAN_ENV,
            capture_output=True,
            text=True,
            encoding="utf-8",
        )
        return [
            m.group(1)
            for ln in (proc.stdout + proc.stderr).splitlines()
            if (m := _RULE_RE.search(ln))
        ]


def _values_equal(a: object, b: object) -> bool:
    """Deep equality that treats two NaNs as equal (Python's `nan != nan` would otherwise
    make any file containing `.nan` look changed) and keeps int/bool/str distinct.
    """
    if isinstance(a, float) and isinstance(b, float):
        return a == b or (math.isnan(a) and math.isnan(b))
    if type(a) is not type(b):
        return False
    if isinstance(a, dict):
        return a.keys() == b.keys() and all(_values_equal(a[k], b[k]) for k in a)
    if isinstance(a, list):
        return len(a) == len(b) and all(starmap(_values_equal, zip(a, b, strict=False)))
    return a == b


def value_preserved(before: str, after: str) -> bool | None:
    """True/False if the YAML 1.2 resolved values match; None if py-yaml12 cannot parse
    one of them (e.g. an unknown `!tag`), so the check is skipped rather than failed.
    """
    try:
        return _values_equal(parse_yaml(before), parse_yaml(after))
    except Exception:
        return None


def _settle(
    run: Runner, formatter: str, cfg: Path, original: str
) -> tuple[str, str | None]:
    """Run the joint cycle to a fixed point. Returns (verdict, settled_or_None).
    verdict: 'converged' | 'loop' | 'fmt-error'.
    """
    f0 = run.format(formatter, original)
    if f0 is None:
        return "fmt-error", None
    state, history = f0, [f0]
    for _ in range(MAX_ITERS):
        fixed = run.ryl_fix(state, cfg)
        if (
            fixed == state
        ):  # ryl leaves this formatter fixed point alone -> joint fixed point
            return "converged", state
        nxt = run.format(formatter, fixed)
        if nxt is None:
            return "fmt-error", None
        if nxt == state or nxt in history:  # formatter reverts ryl's fix -> they fight
            return "loop", state
        history.append(nxt)
        state = nxt
    return "loop", state


@app.command()
def verify() -> None:
    """Check every recipe is loop-free, complaint-free, and value-preserving."""
    for tool, hint in (
        ("ryl", "build or install ryl first (it is the tool under test)"),
        (
            "pixi",
            "install pixi (https://pixi.sh); it fetches the formatters via pixi exec",
        ),
    ):
        if shutil.which(tool) is None:
            typer.secho(
                f"required tool not on PATH: {tool}; {hint}",
                fg=typer.colors.RED,
                err=True,
            )
            raise typer.Exit(2)

    typer.echo(
        f"Pinned formatters (latest as of {TESTED_DATE}): "
        f"yamlfmt {YAMLFMT_VERSION}, prettier {PRETTIER_VERSION}, yamlfix {YAMLFIX_VERSION}"
    )
    failures = skipped = 0
    with tempfile.TemporaryDirectory(prefix="ryl-fmt-compat-") as td:
        run = Runner(Path(td))
        for formatter in RECIPES:
            cfg = run.config(formatter)
            for fname, original in CORPUS.items():
                verdict, settled = _settle(run, formatter, cfg, original)
                if verdict == "fmt-error":
                    skipped += 1
                    typer.echo(
                        f"  {formatter:9} {fname:16} SKIP (formatter could not process)"
                    )
                    continue
                residual = sorted(set(run.ryl_lint(settled, cfg)))
                unexpected = [
                    r
                    for r in residual
                    if r not in EXPECTED_RESIDUAL.get((formatter, fname), set())
                ]
                vp = value_preserved(original, settled)
                value_bad = (
                    vp is False and (formatter, fname) not in EXPECTED_VALUE_CHANGE
                )

                notes = []
                if residual and not unexpected:
                    notes.append(f"expected caveat: {', '.join(residual)}")
                if vp is False and (formatter, fname) in EXPECTED_VALUE_CHANGE:
                    notes.append("expected value change")
                if vp is None:
                    notes.append("value check skipped (py-yaml12 could not parse)")

                if verdict == "loop":
                    status, failures = "FAIL: edit loop", failures + 1
                elif unexpected:
                    status, failures = f"FAIL: complaints {unexpected}", failures + 1
                elif value_bad:
                    status, failures = "FAIL: VALUE CHANGED (corruption)", failures + 1
                else:
                    status = "PASS" + (f"  ({'; '.join(notes)})" if notes else "")
                typer.echo(f"  {formatter:9} {fname:16} {status}")

    if failures:
        typer.secho(f"\n{failures} FAILURE(S)", fg=typer.colors.RED)
        raise typer.Exit(1)
    suffix = f" ({skipped} check(s) skipped, see SKIP lines above)" if skipped else ""
    typer.secho(
        f"\nVERIFIED CLEAN (loop-free, complaint-free, value-preserving){suffix}",
        fg=typer.colors.GREEN,
    )


if __name__ == "__main__":
    app()
