---
name: release
description: >-
  Use when cutting a ryl release. Covers the lockstep version bump across the
  five version-bearing files, the lockfile refresh, the tag/push gate, and the
  post-release SchemaStore + Trusted-Publishing flow.
---

# Release Checklist

- Bump versions in lockstep:
  - Cargo: update `Cargo.toml` `version`.
  - Python: update `pyproject.toml` `[project].version`.
  - NPM: update `package.json` `version`.
- Refresh lockfile and validate:
  - Run `cargo generate-lockfile` (or `cargo check`) to refresh `Cargo.lock`.
  - Stage: `git add Cargo.toml Cargo.lock pyproject.toml package.json`.
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
- Publishing uses Trusted Publishing on all registries (crates.io via GitHub OIDC, PyPI
  via `pypa/gh-action-pypi-publish`, NPM via `actions/setup-node` OIDC). GitHub release
  creation is deferred until after crates.io/PyPI/NPM publishing succeeds, kept as a
  draft until assets upload, with auto-generated notes; reruns skip publish steps for a
  version that already exists.
