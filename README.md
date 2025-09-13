# ryl

ryl - the Rust Yaml Linter is intended to ultimately be a drop in replacement for
[yamllint](https://github.com/adrienverge/yamllint). It's only just begun and isn't
ready for use yet though. I'll update and post info as it becomes ready.

## Usage

ryl accepts one or more paths: files and/or directories.

Basic:

```text
ryl <PATH_OR_FILE> [PATH_OR_FILE...]
```

Behavior:

- Files: parsed as YAML even if the extension is not `.yml`/`.yaml`.
- Directories: recursively lints `.yml` and `.yaml` files.
  - Respects `.gitignore`, global git ignores, and git excludes.
  - Does not follow symlinks.

Exit codes:

- `0` when all parsed files are valid (or no files found).
- `1` when any invalid YAML is found.
- `2` for CLI usage errors (for example, no paths provided).

Examples:

```text
# Single file
ryl myfile.yml

# Multiple inputs (mix files and directories)
ryl config/ another.yml

# Multiple directories
ryl dir1 dir2

# Explicit non-YAML extension (parsed as YAML)
ryl notes.txt
```

Help and version:

- `ryl -h` or `ryl --help` shows auto-generated help.
- `ryl -V` or `ryl --version` prints the version.

The CLI is built with `clap`, which auto-generates `--help` and `--version`.

## Configuration

- Flags:
  - `-c, --config-file <FILE>`: path to a YAML config file.
  - `-d, --config-data <YAML>`: inline YAML config (highest precedence).
  - `--list-files`: print files that would be linted after applying ignores and exit.
  - `-f, --format`, `-s, --strict`, `--no-warnings`: reserved for compatibility.
- Discovery precedence:
  inline `--config-data` > `--config-file` > env `YAMLLINT_CONFIG_FILE`
  (global) > nearest project config up the tree (`.yamllint`, `.yamllint.yml`,
  `.yamllint.yaml`) > user-global (`$XDG_CONFIG_HOME/yamllint/config` or
  `~/.config/yamllint/config`) > built-in defaults.
- Per-file behavior: unless a global config is set via `--config-data`,
  `--config-file`, or `YAMLLINT_CONFIG_FILE`, each file discovers its nearest
  project config. Ignores apply to directory scans and explicit files (parity).
- Presets and extends: supports yamllint’s built-in `default`, `relaxed`, and
  `empty` via `extends`. Rule maps are deep-merged; scalars/sequences overwrite.
