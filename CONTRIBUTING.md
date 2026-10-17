# Contributing to ryl

Human and autonomous-agent contributors are welcome. Every contribution is held to
the same standard: understand the existing behaviour, check the project's history,
and keep the change consistent with [ryl's scope](docs/scope.md) and
[design decisions](docs/design/decisions.md).

Open an issue before starting substantial work so the approach can be agreed.
Small bug fixes and documentation fixes may go straight to a pull request. About
500 changed lines is a guide, not a hard limit; a larger pull request needs prior
discussion and a track record with the contributor.

Before proposing a change, search open and closed issues and pull requests,
including their comments. For a feature, check the relevant rules, formatter,
and configuration decisions. Verify claims by running ryl and, where
compatibility matters, the reference tool or parser. The
[coding agent instructions](AGENTS.md) describe the project's development and
test gates; human contributors can use the same guidance.

In the pull request, link the issue unless the change is a small bug or
documentation fix, and state which prior issues and decisions you checked.
Explain the behaviour you verified and run the applicable checks in
`AGENTS.md`.

## Development setup

Install [rustup](https://rustup.rs/),
[uv](https://docs.astral.sh/uv/getting-started/installation/), and
[pixi](https://pixi.sh/latest/installation/).
Rustup uses the toolchain pinned in [rust-toolchain.toml](rust-toolchain.toml);
`uv` runs the project's Python scripts. Put the global tools listed in
[AGENTS.md](AGENTS.md#development-environment--terminal) on `PATH`:

```text
uv tool install prek
uv tool install ripgrep
uv tool install rumdl
uv tool install typos
uv tool install yamllint
uv tool install zizmor
cargo install cargo-audit --locked
cargo install cargo-nextest --locked
cargo install cargo-llvm-cov --locked
pixi global install tokei
```

Install the Git hooks, then run the lint gate and the tests your change touches:

```text
prek install --install-hooks
prek run --all-files
cargo nextest run --test <suite>
uv run --group docs zensical build --clean
```

CI runs the full suite on Linux and Windows plus coverage; `cargo test --release` and
`uv run .agents/skills/coverage/coverage-missing.py` reproduce them locally. CI skips
random property cases, so a rule or formatter change also runs the
[property-tests skill](.agents/skills/property-tests/SKILL.md)'s suites. See [AGENTS.md](AGENTS.md#code-change-requirements) for when to run
each gate and the [dev skills](.agents/skills/) for task-specific procedures.
The layout is `src/` (application and language server), `tests/` (checks),
`docs/` (site), and `scripts/` (development helpers).
