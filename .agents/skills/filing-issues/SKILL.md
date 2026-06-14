---
name: filing-issues
description: >-
  Use when filing or editing a GitHub issue/PR for ryl or, especially, for another
  project (yamllint, granit, SchemaStore). Covers the cross-repo reference footgun,
  the draft-locally-then-file gate for other people's repos, the never-on-an-assumption
  rule, and the verified-Sources requirement — so a published issue does not need a
  verify-then-re-push cycle.
---

# Filing Issues and PRs

## Cross-repo references (silent footgun)

- ryl's own issues/PRs: a bare `#123` is correct.
- **Any other project** (yamllint, granit, SchemaStore, …): use the fully-qualified
  `owner/repo#123` form, e.g. `adrienverge/yamllint#123`. A bare `#123` silently
  auto-links to *this* repo (`owenlamont/ryl#123`) and points at the wrong issue — a past
  batch of 11 issues had to be re-edited and re-pushed over this.

## Filing on another project's repo

These go out under the maintainer's name, so they are higher-stakes:

1. **Draft locally first** for proof-read; do not open it directly. Ask where drafts
   live (do not hardcode a contributor's local path); the convention is a
   `<repo>-<topic>` Markdown file one directory up from the ryl clone.
2. **Never report on an assumption.** Verify every behavioural claim by *running the
   actual target* at the version in use — never inferred from its lineage, a sibling
   tool, its docs, the spec, or memory. (A granit issue was filed on an unverified
   assumption and had to be fully retracted.) Treat memory notes about third-party
   behaviour as hypotheses to re-verify.
3. **Ship a one-command reproduction**, pinned to the dependency's **latest** version
   (so the report can't be for a bug already fixed upstream), printing
   observed-vs-expected. No repro, no report.
4. **Include verified, clickable Sources** (a "Sources" section: spec quote /
   play.yaml.com event stream / upstream issue links) on the *first* pass — not bare
   prose mentions — to avoid a verify-then-re-push cycle on an already-published issue.

Keep it succinct: concrete ask, runnable repro, then authoritative evidence.

## Triage hygiene

- To assert a verdict on a reported bug, reproduce the reporter's posted output through
  the **same code path** they ran (e.g. run `--fix` and diff byte-for-byte against the
  pasted output, not just `--diff`), and read the cited source/docs directly rather than
  inferring from a grep.
- Reading a PR's inline review comments: list `.../pulls/<n>/comments` and filter by id
  (`--jq 'select(.id==<id>)'`); the per-id single-comment endpoint 404s in this workflow.

## Codifying a lesson into always-on docs

When a one-off incident becomes an AGENTS.md rule, **generalize** it: never hardcode a
local filesystem path, and state the durable principle (reproduce against the current
version once) rather than transcribing the one-off steps.
