# pre-commit and prek

ryl's hooks are published at
[ryl-pre-commit](https://github.com/owenlamont/ryl-pre-commit) and run with both
[pre-commit](https://pre-commit.com/) and [prek](https://prek.j178.dev/). Each hook
installs `ryl` from PyPI at the version matching `rev`.

## Hooks

| Hook id | Runs | Files |
| --- | --- | --- |
| `ryl-check` | `ryl check` | YAML |
| `ryl-format` | `ryl format` | YAML |
| `ryl-check-markdown` | `ryl check --markdown` | `.md`, `.markdown`, `.mdx`, `.qmd`, `.Rmd` |
| `ryl-format-markdown` | `ryl format --markdown` | `.md`, `.markdown`, `.mdx`, `.qmd`, `.Rmd` |

`ryl` and `ryl-markdown` are legacy aliases for `ryl-check` and `ryl-check-markdown`.
The format hooks need `rev: v0.25.0` or later. The Markdown hooks lint and format
[YAML in Markdown](markdown.md) with no `[files]` config.

## Configuration

To apply safe fixes, pass `--fix` to `ryl-check` and list it before `ryl-format`: a
fix can leave layout the formatter then changes. Other CLI options also go in `args`.

=== "pre-commit"

    ```yaml
    # .pre-commit-config.yaml
    repos:
      - repo: https://github.com/owenlamont/ryl-pre-commit
        rev: v0.26.0
        hooks:
          - id: ryl-check
            args: [--fix]
          - id: ryl-format
          - id: ryl-check-markdown
          - id: ryl-format-markdown
    ```

=== "prek"

    ```toml
    # prek.toml
    [[repos]]
    repo = "https://github.com/owenlamont/ryl-pre-commit"
    rev = "v0.26.0"
    hooks = [
      { id = "ryl-check", args = ["--fix"] },
      { id = "ryl-format" },
      { id = "ryl-check-markdown" },
      { id = "ryl-format-markdown" },
    ]
    ```

prek also reads `.pre-commit-config.yaml`, so either file works with prek.

## Seeing warnings after an upgrade

Since v0.26.0, `syntax error: invalid indentation` can fail a check that passed
on an older version. When the diagnostic offers a repair, run `ryl check --fix`
or `ryl format`, then rerun the hook.

pre-commit and prek hide the output of a hook that passes, including ryl's config
deprecation warnings. After upgrading ryl, run the hook verbosely to see them:

```sh
prek run ryl-check --all-files --verbose
pre-commit run ryl-check --all-files --verbose
```

A hook runs ryl once per batch of files, and each run warns once, so the same warning
can appear several times.
