# Shell completions

ryl can generate tab-completion scripts for its flags. `ryl --generate-completions
<SHELL>` prints the script for one shell to stdout, where `<SHELL>` is one of
`bash`, `zsh`, `fish`, `powershell`, or `elvish`:

```bash
ryl --generate-completions zsh
```

The same command is what packagers (conda-forge, a Homebrew tap) run at build
time to ship completions with the binary. To set them up yourself, write the
script where your shell looks for completions.

=== "Bash"

    System-wide (needs the `bash-completion` package):

    ```bash
    ryl --generate-completions bash | sudo tee /etc/bash_completion.d/ryl > /dev/null
    ```

    Or just for your user (bash-completion searches
    `${XDG_DATA_HOME:-$HOME/.local/share}/bash-completion/completions`):

    ```bash
    dir="${XDG_DATA_HOME:-$HOME/.local/share}/bash-completion/completions"
    mkdir -p "$dir"
    ryl --generate-completions bash > "$dir/ryl"
    ```

    Start a new shell to load it.

=== "Zsh"

    Write the script as `_ryl` into a directory on your `$fpath`:

    ```zsh
    mkdir -p ~/.zfunc
    ryl --generate-completions zsh > ~/.zfunc/_ryl
    ```

    Then make sure that directory is on `$fpath` before `compinit` runs in your
    `~/.zshrc`:

    ```zsh
    fpath=(~/.zfunc $fpath)
    autoload -U compinit && compinit
    ```

=== "Fish"

    ```fish
    mkdir -p ~/.config/fish/completions
    ryl --generate-completions fish > ~/.config/fish/completions/ryl.fish
    ```

    fish loads it automatically on the next shell.

=== "PowerShell"

    Add this line to your PowerShell profile (the file at `$PROFILE`) so
    completions load on each session:

    ```powershell
    ryl --generate-completions powershell | Out-String | Invoke-Expression
    ```

=== "Elvish"

    Add to `~/.config/elvish/rc.elv`:

    ```elvish
    eval (ryl --generate-completions elvish | slurp)
    ```
