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

Set up tab-completion with [Shell completions](shell-completions.md), or
continue to the [Quick start](quickstart.md) to run your first lint.
