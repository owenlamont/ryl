# Output formats

`ryl` renders diagnostics in several formats, selected with `--output-format` (`-f`, or
yamllint's `--format`). The default, `auto`, picks a human-readable format for your
environment; the machine-readable formats are opt-in and are meant to be consumed by
editors, CI, and Git forges.

| Format | Purpose | Default destination |
| --- | --- | --- |
| `auto` (default) | GitHub annotations in GitHub Actions, otherwise colored or plain | stderr |
| `standard` | Plain text, grouped per file | stderr |
| `colored` | Plain text with ANSI colors | stderr |
| `github` | GitHub Actions workflow commands (`::error ...`) | stderr |
| `parsable` | One `path:line:col: [level] message (rule)` line per diagnostic | stderr |
| `junit` | JUnit XML test report | stdout |
| `gitlab` | GitLab Code Quality JSON report | stdout |

The exit code is the same for every format: `0` when clean, `1` when any error-level
diagnostic is found (or `2` with `--strict` and only warnings), `2` for a usage or config
error.

## Choosing where output goes

Each format has a default stream: the console formats (`standard`, `colored`, `github`,
`parsable`) go to **stderr**, and the report formats (`junit`, `gitlab`) go to **stdout**,
so a report can be redirected into an artifact file:

```console
$ ryl check --output-format gitlab . > gl-code-quality-report.json
```

### Multiple outputs in one run

`--output-format` (`-f`) is repeatable, and each `--output-file` (`-o`) binds to the
**most recent** `--output-format`, so a single run can produce several outputs at once
(the RuboCop / Biome model). `-o` takes a file path, or `-` for stdout:

```console
# console diagnostics on stderr AND a GitLab report written to a file
$ ryl check --output-format auto --output-format gitlab -o gl-code-quality-report.json .

# a JUnit file and a GitLab file together, with no console output
$ ryl check --output-format junit -o report.xml --output-format gitlab -o gl.json .
```

An `--output-format` with no `--output-file` uses its default stream, so the way to get
your usual console output **and** a report file is to add the report `--output-format`
with its own `-o`, as in the first example above.

The rules that keep the outputs unambiguous (each a usage error, exit code 2):

- an `--output-file` must follow an `--output-format`, since it binds to that format;
- an `--output-format` takes at most one `--output-file`;
- at most one output may go to stdout and at most one to the console (stderr); two
  documents sharing a stream would interleave, so give the others a file destination;
- two outputs may not resolve to the same file (the second would clobber the first);
- an output file that is also a linted input (or the `--stdin-filename`) is refused, so a
  report can never truncate the source it just linted.

Otherwise an `--output-file` overwrites its destination, so do not point it at a file you
want to keep. `--diff` previews fixes and ignores `--output-format`, so it combines with
neither `--output-file` nor `--output-format junit`/`--output-format gitlab`. A clean or
empty project still produces a valid empty report for each report target (`[]` for
GitLab, an empty `<testsuites>` for JUnit), so a CI step that ingests the artifact never
fails on a missing file.

### `ryl format --check`

`ryl format --check` reports the lines it would change through the same formats.
An empty or wholly excluded run still writes an empty configured report, including
when `--stdin-filename` is excluded.
`ryl format --output-format` takes one format, written to its default stream, and
overrides `[output]`. Like ruff's, `ryl format` honours it only under `--check`.
The same output-target guards apply to files and stdin: two configured reporters
sharing stdout or stderr exit 2 before emitting reports. `ryl format --diff` ignores
`[output]` and `--output-format`, keeping stdout for the patch.

### Configuring outputs in TOML

The same outputs can be set once in a project's TOML config under `[output]`, so `ryl check .`
in CI produces the artifacts without repeating the flags. Each format is a sub-table; an
absent `path` uses that format's default stream, `path = "-"` is stdout, and any other
value is a file:

```toml
# keep the normal console output, and also write a GitLab report file
[output.auto]

[output.gitlab]
path = "gl-code-quality-report.json"
```

`[output]` is ryl-only and TOML-only (it is rejected in a YAML config). It is read once
from the configuration governing the run: the `-c`/`-d` config, or the project config
discovered for the inputs (so a project's `.ryl.toml` applies to `ryl check .`). A CLI
`--output-format` overrides the entire `[output]` table, so the command line always wins
over the config. The same unambiguous-output rules above apply to a config that declares
several targets.

`[output]` produces a single, run-level set of artifacts, so it is read from one config.
That is unambiguous for a single project (one root, or subdirectories that share the
project config). If you point one run at *separate* projects that each declare their own
differing `[output]`, the first project config discovered along the inputs wins; pass
`-c`/`-d` to choose the output config explicitly in that case.

## JUnit XML

The JUnit report is one `<testsuite>` per file and one `<testcase>` per diagnostic. A
failing diagnostic is a `<failure>` carrying the message and the rule id as its `type`; a
clean file is a single passing testcase; a file that could not be read or parsed is an
`<error>` testcase.

```xml
<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="ryl" tests="2" failures="1" errors="0">
  <testsuite name="config.yaml" tests="1" failures="1" errors="0">
    <testcase name="colons:3:8" classname="config.yaml">
      <failure message="too many spaces after colon" type="colons">3:8 too many spaces after colon</failure>
    </testcase>
  </testsuite>
  <testsuite name="ok.yaml" tests="1" failures="0" errors="0">
    <testcase name="ok.yaml" classname="ok.yaml"/>
  </testsuite>
</testsuites>
```

In GitLab CI it is published with `artifacts:reports:junit`; other forges that render
JUnit reports consume it the same way. JUnit XML has no single official standard; `ryl`
follows the [`junit-10.xsd`](https://github.com/jenkinsci/xunit-plugin/blob/master/src/main/resources/org/jenkinsci/plugins/xunit/types/model/xsd/junit-10.xsd)
schema maintained by the Jenkins xUnit plugin (the most authoritative published schema),
cross-checked against the widely cited
[JUnit XML reference](https://llg.cubic.org/docs/junit/).

## GitLab Code Quality

The GitLab report is a single JSON array, one object per diagnostic, matching the
[Code Quality report format](https://docs.gitlab.com/ci/testing/code_quality/#code-quality-report-format),
a documented subset of the Code Climate engine's
[`spec/analyzers/SPEC.md`](https://github.com/codeclimate/platform/blob/master/spec/analyzers/SPEC.md)
(the underlying standard):

```json
[
  {
    "description": "too many spaces after colon",
    "check_name": "colons",
    "severity": "major",
    "fingerprint": "2f8a...e1",
    "location": { "path": "config.yaml", "lines": { "begin": 3 } }
  }
]
```

- `severity` maps from the rule level: an error is `major`, a warning is `minor`, and a
  file that could not be processed is a `blocker`.
- `location.path` is relative to `CI_PROJECT_DIR` (the repository root in GitLab CI) when
  that variable is set, otherwise to the working directory, with no `./` prefix, as GitLab
  requires. A file outside that root is expressed with `..` segments (like ruff).
- `fingerprint` is a stable SHA-256 of the diagnostic's identity (path, rule, and message,
  deliberately not the line or column), so GitLab keeps tracking the same issue across
  pipeline runs even when an edit elsewhere shifts its line.

Publish it with `artifacts:reports:codequality` pointing at the file you wrote with
`-o` (conventionally `gl-code-quality-report.json`):

```yaml
lint:
  script:
    - ryl check --output-format gitlab -o gl-code-quality-report.json .
  artifacts:
    reports:
      codequality: gl-code-quality-report.json
```
