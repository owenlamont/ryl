# ruff: file-ignore[suspicious-subprocess-import, subprocess-without-shell-equals-true, line-too-long]
"""Time each allocator variant with hyperfine and, on Unix, sample peak RSS."""

import json
import os
from pathlib import Path
import shutil
import statistics
import subprocess
import sys


if sys.argv[1] == "--restore":
    shutil.rmtree(sys.argv[3], ignore_errors=True)
    shutil.copytree(sys.argv[2], sys.argv[3])
    sys.exit()

root, out = Path(sys.argv[1]).resolve(), Path(sys.argv[3]).resolve()
variants = sys.argv[2].split(",")
out.mkdir(parents=True, exist_ok=True)
here = Path(__file__).resolve().parent
ext = ".exe" if os.name == "nt" else ""
modes = {
    "check": [
        "check",
        "--no-cache",
        "-f",
        "parsable",
        "-c",
        (here / "yamllint.toml").as_posix(),
    ],
    "format": ["format", "--no-cache", "-c", (here / "max-work.toml").as_posix()],
}
rss = {}
for case in ("size-1k", "size-10240k", "count-1000", "count-10000"):
    src, work = root / "inputs" / case, root / "work" / case
    for mode, args in modes.items():
        restore = [
            Path(sys.executable).as_posix(),
            Path(__file__).resolve().as_posix(),
            "--restore",
            src.as_posix(),
            work.as_posix(),
        ]
        subprocess.run(restore, check=True)
        runs = "100" if case == "size-1k" else "15"
        cmd = [
            "hyperfine",
            "-N",
            "-i",
            "-w",
            "3",
            "-r",
            runs,
            "--export-json",
            (out / f"{case}-{mode}.json").as_posix(),
        ]
        if mode == "format":
            cmd += ["--prepare", " ".join(restore)]
        argv = {
            v: [(root / "bin" / f"ryl-{v}{ext}").as_posix(), *args, work.as_posix()]
            for v in variants
        }
        for v in variants:
            cmd += ["-n", v, " ".join(argv[v])]
        subprocess.run(cmd, check=True)
        if os.name == "nt":
            continue

        for v in variants:
            samples = []
            for _ in range(3):
                subprocess.run(restore, check=True)
                probe = "import resource,subprocess,sys; subprocess.run(sys.argv[1:], stdout=subprocess.DEVNULL); print(resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss)"
                peak = int(
                    subprocess.run(
                        [sys.executable, "-c", probe, *argv[v]],
                        capture_output=True,
                        text=True,
                        check=True,
                    ).stdout
                )
                samples.append(peak / 1024 if sys.platform == "darwin" else peak)
            rss[f"{case} {mode} {v}"] = statistics.median(samples) / 1024
(out / "rss-mib.json").write_text(json.dumps(rss, indent=1))
