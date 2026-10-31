# ruff: file-ignore[suspicious-subprocess-import, subprocess-without-shell-equals-true]
"""Stage the allocator benchmark inputs from the pinned corpus repos."""

import gzip
import json
from pathlib import Path
import re
import subprocess
import sys


root = Path(sys.argv[1])
manifest = json.loads(
    gzip.decompress((Path(__file__).parent / "manifest.json.gz").read_bytes())
)
manifest["cases"]["count-1000"] = manifest["cases"]["count-10000"][:1000]
needed = {}
contents = {}
for files in manifest["cases"].values():
    for repo, path in files:
        needed.setdefault(repo, set()).add(path)
for repo, paths in needed.items():
    url, sha = manifest["repos"][repo]
    clone = root / "repos" / repo.replace("/", "__")
    clone.mkdir(parents=True)
    git = ["git", "-C", str(clone)]
    subprocess.run([*git, "init", "-q"], check=True)
    for key, value in (
        ("core.sparseCheckout", "true"),
        ("core.sparseCheckoutCone", "false"),
        ("core.longpaths", "true"),
        ("core.protectNTFS", "false"),
    ):
        subprocess.run([*git, "config", key, value], check=True)
    patterns = "".join(
        "/" + re.sub(r"([*?\[\\])", r"\\\1", p) + "\n" for p in sorted(paths)
    )
    (clone / ".git" / "info" / "sparse-checkout").write_text(patterns)
    subprocess.run(
        [*git, "fetch", "-q", "--depth", "1", "--filter=blob:none", url, sha],
        check=True,
    )
    subprocess.run([*git, "checkout", "-q", "FETCH_HEAD"], check=True)
    # Read blobs from git, not the worktree, which mangles non-ASCII names on Windows.
    ordered = sorted(paths)
    batch = subprocess.run(
        [*git, "cat-file", "--batch"],
        input="".join(f"HEAD:{p}\n" for p in ordered).encode(),
        capture_output=True,
        check=True,
    ).stdout
    for path in ordered:
        header, batch = batch.split(b"\n", 1)
        size = int(header.split()[2])
        contents[repo, path], batch = batch[:size], batch[size + 1 :]
for case, files in manifest["cases"].items():
    target = root / "inputs" / case
    target.mkdir(parents=True)
    for index, (repo, path) in enumerate(files):
        (target / f"file_{index:05d}.yaml").write_bytes(contents[repo, path])
    print(case, len(files))
