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
  - **Review the dev skills** in `.agents/skills/` for any procedure that changed
    in this release (a moved command, renamed test, new wiring step), and update
    the affected `SKILL.md`.
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
  winget PR (no existence guard, unlike the crates/PyPI/NPM steps).
