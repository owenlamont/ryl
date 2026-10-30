# Installation

ryl ships as a single binary. Pick the install method that matches your
toolchain.

=== "Cargo"

    ```bash
    cargo install ryl
    ```

=== "pip"

    ```bash
    pip install ryl
    ```

=== "npm"

    ```bash
    npm install --global @owenlamont/ryl
    ```

=== "conda"

    ```bash
    pixi global install ryl
    # or: conda install -c conda-forge ryl
    ```

=== "winget (Windows)"

    ```powershell
    winget install owenlamont.ryl
    ```

=== "Prebuilt binary"

    Download a release artifact from the
    [GitHub releases page](https://github.com/owenlamont/ryl/releases) and
    place the binary on your `PATH`.

Verify the install:

```bash
ryl --version
```

## GitHub Actions

[ryl-action](https://github.com/marketplace/actions/ryl-action) installs the
release binary on the runner, verifies its checksum and runs `ryl check .`:

```yaml
- uses: actions/checkout@v7
- uses: owenlamont/ryl-action@v0.27.0
- uses: owenlamont/ryl-action@v0.27.0
  with:
    args: format --check .
```

Each action tag installs the matching ryl release. ryl enables no rules without
a config, so commit a `.ryl.toml` first. The other inputs are `version`, `config`
(passed as `-c`), `config-data` (inline config, passed whole as `-d`) and
`install-only`, which puts `ryl` on `PATH` for later steps. `args` splits on
whitespace without quote handling, so pass inline config through `config-data`.
See the [ryl-action README](https://github.com/owenlamont/ryl-action) for more
examples.

Set up tab-completion with [Shell completions](shell-completions.md), or
continue to the [Quick start](quickstart.md) to run your first lint.
