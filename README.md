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
