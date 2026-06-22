---
name: release
description: >-
  Use when cutting a ryl release. Covers the lockstep version bump across the
  five version-bearing files, the lockfile refresh, the tag/push gate, and the
  post-release SchemaStore + Trusted-Publishing flow.
---

# Release Checklist

- Bump versions in lockstep, in the release feature branch (never a separate post-merge
  bump PR — a forgotten bump has forced one before):
  - Cargo: update `Cargo.toml` `version`.
  - Python: update `pyproject.toml` `[project].version`.
  - NPM: update `package.json` `version`.
- Refresh both lockfiles and validate (five version-bearing files in total):
  - Run `cargo generate-lockfile` to refresh `Cargo.lock`. This deliberately sweeps in
    semver-compatible transitive bumps — the maintainer prefers staying current, so do
    **not** revert that churn as "separate-PR noise" (a recurring past mistake). Reach
    for a frozen `cargo check` only when keeping the lockfile pinned is explicitly wanted.
  - Run `uv lock` to refresh `uv.lock` (it carries the project version too).
  - Stage: `git add Cargo.toml Cargo.lock pyproject.toml package.json uv.lock`.
  - Run `prek run --all-files` (re-run if files were auto-fixed).
- Docs and notes:
  - Update README/AGENTS for behavior changes.
  - **Refresh the benchmark image** when the release changes timed code: any minor or
    major bump, or a patch that touches performance-relevant paths. Skip it for a
    docs/packaging/bugfix patch with no perf delta, leaving the committed
    `img/benchmark-5x5-5runs.svg` in place (a patch-digit-stale version label on the chart
    is acceptable). Do this **in the version-bump PR, before squash-merge** — not as a
    post-merge afterthought (a forgotten refresh has forced a separate follow-up PR, and
    main is never pushed to directly). The chart carries both the `ryl <ver>` and
    `yamllint <ver>` labels. The script's `yamllint` PEP723 dep is unpinned, but uv reuses
    a cached resolution, so a plain `uv run` can time against a stale yamllint; pass `-U`
    (`uv run -U scripts/...`, which forces a fresh resolution) to pick up the newest
    release, then confirm the chart's `yamllint <ver>` label equals the current PyPI latest
    (`curl -s https://pypi.org/pypi/yamllint/json`). To refresh: build the release binary
    and point the script at it with
    `--ryl-bin` (the label is read from that binary's `--version`, so it always matches
    what was timed; without `--ryl-bin` the script benchmarks whatever `ryl` is on
    `PATH`): `cargo build --release && uv run scripts/benchmark_perf_vs_yamllint.py
    --ryl-bin target/release/ryl`. The no-arg defaults reproduce the committed 5x5/5-run
    matrix; copy the run's `manual_outputs/benchmarks/<ts>/benchmark.svg` over the tracked
    `img/benchmark-5x5-5runs.svg` (only that file is committed; `manual_outputs/` is
    gitignored) and confirm the `ryl <version>` label reads the release version.
  - **Review the dev skills** in `.agents/skills/` for any procedure that changed
    in this release (a moved command, renamed test, new wiring step), and update
    the affected `SKILL.md`. If any `.agents/skills/` file changed, refresh the
    gitignored local copies Claude Code loads (`.claude/skills/`) **from GitHub
    pinned to the release tag, after the tag is pushed** — never `--from-local`
    (that pins to unverified local state):
    `gh skill install owenlamont/ryl --all --allow-hidden-dirs --agent claude-code
    --scope project --pin vX.Y.Z --force` (then `gh skill update` for later
    refreshes). Confirm the install pulls only the `.agents/skills/` dev skills and
    not the published `skills/ryl`; scope by skill path if it does not.
  - Review the downstream agent skill `skills/ryl/SKILL.md` and the
    `docs/using-ryl-with-ai-agents.md` page for any behaviour, flag, format, or
    rule changes in this release. The `agent_skill_drift_guard` test catches a
    removed/renamed flag, but new flags or behaviour need a manual update. (The
    `gen-llms-txt` prek hook keeps `docs/llms.txt` current automatically.)
  - Summarize notable changes in the PR description or changelog (if present).
- Tag and push (when releasing):
  - `git tag -a vX.Y.Z -m "vX.Y.Z"`
  - `git push && git push --tags`
  - `.github/workflows/release.yml` validates that the pushed tag version
    matches `Cargo.toml`, `pyproject.toml`, and `package.json` versions
    before release jobs run.
  - **Transient network/download flakes recur (~every 2nd-3rd release), in no fixed
    step.** Release runs have many download-heavy steps (macOS/Linux build jobs, the
    cross-compile `cargo install cross`, crates.io fetches); one intermittently dies
    on a network/SSL blip (e.g. `curl ... OpenSSL SSL_read: unexpected eof`, exit
    101) — sometimes a macOS build, sometimes the aarch64-linux cross install. It is
    not a real failure: a failed build/upload job gates and *skips* the
    publish/finalize jobs, so nothing publishes and the tag stays intact. Re-run the
    failed jobs with `gh run rerun <run-id> --failed` (also re-runs the skipped
    downstream publish jobs); repeat if a different step flakes next time. A partial
    durable fix for the most frequent culprit: pre-install `cross` from a prebuilt
    binary (`taiki-e/install-action@cross`) so the flaky `cargo install cross` step
    never runs.
- After a successful release, `.github/workflows/sync-schemastore.yml` projects
  `ryl.toml.schema.json` into SchemaStore's draft-07 format, updates the user's
  SchemaStore fork, and prints a manual upstream PR handoff for
  `owenlamont/schemastore:ryl-schema-update`.
  - Known failure: the sync branch is built directly on `upstream/master`
    (`git checkout -B … upstream/master`), so it carries upstream's `.github/workflows/`
    files; pushing them needs the App token's **workflows: write** scope. If the job
    errors with "refusing to allow an OAuth App to create or update workflow", that scope
    is missing — confirm the `actions/create-github-app-token` step requests
    `permission-workflows: write` (added in PR #265) and that the GitHub App installation
    actually grants Workflows: Read and write. The fork's `master` state is *not*
    involved (the branch never derives from it).
- Publishing uses Trusted Publishing on all registries (crates.io via GitHub OIDC, PyPI
  via `pypa/gh-action-pypi-publish`, NPM via `actions/setup-node` OIDC). GitHub release
  creation is deferred until after crates.io/PyPI/NPM publishing succeeds, kept as a
  draft until assets upload, with auto-generated notes; reruns skip publish steps for a
  version that already exists.
- After the release is un-drafted, the `publish-winget` job submits a winget-pkgs PR via
  `wingetcreate update owenlamont.ryl` (token: the classic `public_repo` `WINGET_PAT`
  secret in the `automation` environment). It requires the package to already exist in
  winget-pkgs from the one-time `wingetcreate new` bootstrap; `update` preserves the
  nested-portable config and `Microsoft.VCRedist.2015+` dependencies, swapping only
  version, URLs, and SHA256. Re-running a published version would attempt a duplicate
  winget PR (no existence guard, unlike the crates/PyPI/NPM steps). If that winget PR is
  blocked by a Microsoft Defender false positive (`Validation-Defender-Error` / "Installer
  failed security check"), see the `winget-defender-fp` skill.
- Social posts are optional — skip them for a routine correctness/packaging release; draft
  them only for a notable feature. When drafting, write the `.txt` files one directory up
  from the ryl clone (where drafts live) but run any post-length/status check from **inside
  the repo** (a tool run from the drafts dir fails with `not a git repo`). Respect the
  per-platform limits: BlueSky 300 chars, Mastodon 500, ASCII only. See the social-post
  drafting convention for the full per-platform link rules.
