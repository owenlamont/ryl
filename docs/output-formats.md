# Output formats

`ryl` renders diagnostics in several formats, selected with `--format` (`-f`). The
default, `auto`, picks a human-readable format for your environment; the machine-readable
formats are opt-in and are meant to be consumed by editors, CI, and Git forges.

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

The console formats (`standard`, `colored`, `github`, `parsable`) print diagnostics to
**stderr**. The report formats (`junit`, `gitlab`) print to **stdout**, so they can be
redirected into an artifact file:

```console
$ ryl --format gitlab . > gl-code-quality-report.json
```

`--output-file` (`-o`) writes the selected format to a file instead of its default
stream, which is the most robust option in CI (no shell redirection of the wrong stream):

```console
$ ryl --format junit -o report.xml .
$ ryl --format gitlab -o gl-code-quality-report.json .
```

This follows the same model as ruff and eslint: one format goes to one place per run. To
see human output **and** keep a report file in a single run, pipe the console output
through `tee`, or run `ryl` twice.

`--output-file` cannot be combined with `--diff` (which previews fixes and ignores
`--format`), and `--format junit`/`--format gitlab` cannot be combined with `--diff`. An
`--output-file` that points at a file being linted (or at the `--stdin-filename`) is
refused, so a report can never truncate the source it just linted. Otherwise
`--output-file` overwrites its destination, so do not point it at a file you want to keep
(such as a config file). A clean or empty project still produces a valid empty report
(`[]` for GitLab, an empty `<testsuites>` for JUnit), so a CI step that ingests the
artifact never fails on a missing file.

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
JUnit reports consume it the same way. The shape follows the de-facto
[JUnit XML specification](https://llg.cubic.org/docs/junit/).

## GitLab Code Quality

The GitLab report is a single JSON array, one object per diagnostic, matching the
[Code Quality report format](https://docs.gitlab.com/ci/testing/code_quality/#code-quality-report-format):

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
    - ryl --format gitlab -o gl-code-quality-report.json .
  artifacts:
    reports:
      codequality: gl-code-quality-report.json
```
