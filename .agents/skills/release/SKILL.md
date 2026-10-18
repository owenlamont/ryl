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
  - If the release first ships a config deprecation, set its `deprecated_since` in
    `config_schema::DEPRECATED_TOML_KEYS` (and the test pinning that table) to this
    version. The repo's own `.ryl.toml` keeps the old shape until the `ryl-pre-commit`
    pin in `prek.toml` reaches that release (the pinned binary rejects the new keys);
    move it in the PR that bumps the pin.
- Refresh both lockfiles and validate (five version-bearing files in total):
  - Run `cargo generate-lockfile` to refresh `Cargo.lock`. This deliberately sweeps in
    semver-compatible transitive bumps — the maintainer prefers staying current, so do
    **not** revert that churn as "separate-PR noise" (a recurring past mistake). Reach
    for a frozen `cargo check` only when keeping the lockfile pinned is explicitly wanted.
  - Run `uv lock` to refresh `uv.lock` (it carries the project version too).
  - Stage: `git add Cargo.toml Cargo.lock pyproject.toml package.json uv.lock`.
  - Run `prek run --all-files` (re-run if files were auto-fixed).
- Docs and notes:
  - Draft the next entry with `uv run --script scripts/changelog.py draft vX.Y.Z`.
    It groups merged PR subjects by conventional-commit prefix; edit the draft into
    one-line user-visible changes, omitting internal refactors and CI work. Move the
    reviewed `Unreleased` items into a `## vX.Y.Z` section in `CHANGELOG.md` and
    leave an empty `## Unreleased` section above it. Verify the entry with
    `uv run --script scripts/changelog.py extract vX.Y.Z` before tagging. A missing
    or empty version section stops `release.yml` before it creates the draft release.
  - Update README/AGENTS for behavior changes.
  - **Refresh the benchmark image** when the release changes timed code: any minor or
    major bump, or a patch that touches performance-relevant paths. Skip it for a
    docs/packaging/bugfix patch with no perf delta, leaving the committed
    `img/benchmark-5x5-5runs.svg` in place. Do this **in the version-bump PR, before
    squash-merge** — not as a post-merge afterthought. Bump the tool pins deliberately
    in `scripts/benchmark_tool_versions.toml`, then use the benchmark's
    `--install-pinned-tools` option to install them with uv, pixi, and npm. The README
    table reports these pins; it is not the source of truth. Run `cargo build --release`,
    then run this complete pre-flight smoke test (its load override is for smoke tests
    only):

    ```sh
    uv run scripts/benchmark_yaml_tools.py --ryl-bin target/release/ryl \
      --install-pinned-tools --file-counts 1,10 --file-sizes-kib 1,9 \
      --runs 2 --warmup 1 --warm --ignore-load
    ```

    Check its `summary.csv`, `warm-summary.csv`, `meta.json`, and both SVGs before
    the full run. Then check
    `uptime` and `nproc`. Wait in five-minute steps, up to one hour, while the
    one-minute load exceeds half the CPU count. Once idle, run
    `uv run scripts/benchmark_yaml_tools.py --ryl-bin target/release/ryl
    --install-pinned-tools --warm`. The defaults reproduce the committed 5x5/5-run
    matrix. The script checks load again after corpus validation; discard a run if
    other builds push load over the threshold during timing. If the hour yields no
    stable run, leave the old chart and name that limit in the PR. The README chart
    shows the real-world dataset only. Re-plot the full run without
    re-timing, then copy its SVG over the tracked image:

    ```sh
    uv run scripts/benchmark_yaml_tools.py --replot manual_outputs/benchmarks/<ts> --datasets real-world
    cp manual_outputs/benchmarks/<ts>/benchmark.svg img/benchmark-5x5-5runs.svg
    cp manual_outputs/benchmarks/<ts>/benchmark-warm.svg img/benchmark-warm-5x5-5runs.svg
    ```

    The generated corpus stays in a temporary directory;
    `manual_outputs/benchmarks/` is gitignored. Check `meta.json` for binary versions,
    configs, corpus hash, seed, strategy, and rejection rate. Add or update the README
    tool table from the deliberate pins; the chart itself has no version labels.
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
    `gen-llms-txt` prek hook keeps `docs/llms.txt` and `docs/llms-full.txt` current
    automatically.)
  - Summarize notable changes in the PR description; the changelog is the release
    notes source.
- Release-candidate gates, **after the version bump and before tagging**:
  - Build the release candidate with `cargo build --release`.
  - Run the performance gate against the newest published stable `v*` release:

    ```sh
    uv run --script scripts/release_perf_gate.py --ryl-bin target/release/ryl --release vX.Y.Z
    ```

    On a shared benchmark host, acquire the host's benchmark lock around the entire
    command (for example `flock /path/to/bench.lock uv run --script ...`). The gate
    reuses the README harness's pinned real-world corpus, 5×5 cases, relaxed lint and
    default formatter config. Each case alternates A/B order, with 3 warmups and 20
    samples per binary; caches are disabled. The baseline is the exact release's PyPI
    wheel installed into an isolated uv tool directory, keeping package-launch overhead
    outside timing. Historical releases without a cache need no cache flag.
    `--previous-version vX.Y.Z` selects a published baseline explicitly;
    `--previous-bin /path/to/ryl` supports local release-binary comparisons.
    Preserve the per-case mean ± standard deviation and delta table, plus
    `manual_outputs/benchmarks/release-perf.json`, in the release PR. The host-load
    guard applies before and throughout timing; an overloaded host invalidates the run.
    Any mean slowdown **strictly above 10%** blocks the tag. `--tolerance` changes the
    threshold only with the maintainer's agreement. Acceptance requires the maintainer
    to approve a bullet in the exact release's CHANGELOG section (dated headings are
    supported):

    ```text
    - Performance: format/100x9KiB: accepted because <reason>.
    ```

    Case names must exactly match the gate table (`lint/20x1KiB`, for example).
    Every failing case needs a separate top-level bullet with a nonempty reason;
    quoted, nested or historical markers cannot waive a regression. Re-run the gate
    after recording acceptance.
  - Run the downstream report on at least two of **the maintainer's downstream repos**:

    ```sh
    uv run --script scripts/downstream_check.py --ryl-bin /absolute/path/to/target/release/ryl /path/to/downstream-a /path/to/downstream-b
    ```

    The helper installs each repo's ryl-pre-commit release pin from `prek.toml` or
    `.pre-commit-config.yaml`/`.yml`, then compares read-only whole-repo checks with the
    candidate. Frozen SHA pins select the newest upstream release tag matching the
    SHA (requiring `git` and network access); untagged commits fail rather than
    guessing a PyPI version.
    Format hooks trigger `format --check`; Markdown hooks add Markdown
    checks. Repo configuration applies, but hook file filters and hook arguments do not:
    the report intentionally checks the whole repo. Raw stdout/stderr retains every
    warning, notice and formatter filename alongside new/removed diagnostics. Exit 1
    means changes or errors need review; an exit 0 does not replace the guidance trial.
  - Read the downstream report, then upgrade at least two downstream repos using **only
    ryl's printed warnings, `--help` and published docs** as guidance. Exercise the
    upgraded hooks too, so hidden warnings or an incomplete migration appear. Record
    each repo's starting pin, warnings followed, commands, hook result and guidance
    defects in the release PR body. Any misleading or missing guidance blocks the tag:
    fix the guidance, rebuild the candidate, and repeat the affected upgrade trial.
- Tag and push (when releasing):
  - `git tag -a vX.Y.Z -m "vX.Y.Z"`
  - `git push && git push --tags`
  - `.github/workflows/release.yml` validates that the pushed tag version
    matches `Cargo.toml`, `pyproject.toml`, and `package.json` versions
    before release jobs run.
  - **Transient network/download flakes are now auto-retried in `release.yml`** (#372):
    `CARGO_NET_RETRY` retries crates.io fetches, `cross` is pre-installed (no
    `cargo install cross` crates.io hop), the QEMU manylinux/musllinux builds retry
    once, and the smoke tests loop up to three attempts. A flake that survives all
    those is rare. When one does, it is not a real failure: a failed build/upload job
    gates and *skips* the publish/finalize jobs, so nothing publishes and the tag
    stays intact. Re-run the failed jobs with `gh run rerun <run-id> --failed` (also
    re-runs the skipped downstream publish jobs); repeat if a different step flakes.
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
- After a successful release, `release.yml`'s `notify-ryl-vscode` job dispatches a
  `ryl_release` event to `owenlamont/ryl-vscode`, whose `bump-ryl` workflow opens a PR
  moving its bundled-binary pin to this release. That repo also polls weekly, so a
  failed dispatch delays the bump rather than losing it; nothing here needs re-running.
- Publishing uses Trusted Publishing on all registries (crates.io via GitHub OIDC, PyPI
  via `pypa/gh-action-pypi-publish`, NPM via `actions/setup-node` OIDC). GitHub release
  creation is deferred until after the binaries build, kept as a draft until
  crates.io/PyPI/NPM publishing succeeds, with its body copied from that version's
  `CHANGELOG.md` section. Reruns update an existing draft's body and skip publish steps
  for a version that already exists.
- The `publish-winget` job syncs the winget-pkgs fork with upstream before submitting,
  because `wingetcreate` cannot fast-forward it and `--submit` fails with "The forked
  repository could not be synced with the upstream commits" once it falls behind. If that
  error appears anyway, sync by hand
  (`gh api repos/<owner>/winget-pkgs/merge-upstream -f branch=master`) and re-run the job.
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
