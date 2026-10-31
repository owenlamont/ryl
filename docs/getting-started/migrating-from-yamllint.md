# Migrating from yamllint

ryl implements yamllint's rule set, and its own configuration is TOML.
ryl still reads a yamllint YAML config (`.yamllint`, `.yamllint.yml`,
`.yamllint.yaml`, `-c`/`-d` YAML, `YAMLLINT_CONFIG_FILE` and the yamllint
user-global config) with the same semantics as upstream, but that support is
deprecated: each one prints a warning with migration guidance, and
a later release will stop reading YAML config. Migrate with
[`ryl --migrate-configs`](#automatic-migration); TOML also carries the
ryl-only features, such as
[`lint.fixable`/`lint.unfixable`](#optional-configure-auto-fixes).
Warning commands name the minimum ryl version needed for the migration flags;
run them with that version or later. Project YAML warnings include
`--migrate-delete-old` so a later `ryl check` does not rediscover `.yamllint`.
Paths that need different quoting in POSIX shells and PowerShell receive a
manual instruction without a command.

On the command line the mapping is mechanical: replace `yamllint` with `ryl
check`, keeping every flag and path the same. `ryl check` accepts the same lint
flags as yamllint (`-c`/`-d`/`-f`/`-s`/`--no-warnings`/`--list-files`/`-`), so
`yamllint -d 'extends: default' .` becomes `ryl check -d 'extends: default' .`.
`-d` also takes a whole ryl config as inline TOML, such as
`ryl check -d 'lint.rules.anchors = "enable"' .`, which avoids the YAML
deprecation warning.
For a config-free selection, replace inline YAML rule disables and file ignores
with CLI flags:

```bash
ryl check --enable ALL --disable line-length,truthy,document-start --exclude pnpm-lock.yaml .
```

`ALL` selects every ryl rule, rather than yamllint's `default` preset.
`--disable` subtracts from config or `--enable` selections; `--exclude` replaces
config exclusions with comma-separated globs relative to the current directory.
Both flags may appear once. TOML still has no `extends` or presets.

Bare `ryl <paths>` also lints but is deprecated in favour of `ryl check` and
prints a warning to stderr.

`ryl format` defaults to indent width 2 independently of lint `indentation.spaces`.
To format at another width, migrate to TOML and set top-level `indent-width`.
Without `indent-width`, the lint rule still defaults to `"consistent"`.

## Automatic migration

ryl ships with a built-in converter. From the root of your project:

```bash
# List the configs that would be converted (no files written)
ryl --migrate-configs

# Print the converted TOML (no files written)
ryl --migrate-configs --migrate-stdout

# Write .ryl.toml next to each discovered .yamllint file
ryl --migrate-configs --migrate-write

# Write and remove the original YAML configs
ryl --migrate-configs --migrate-write --migrate-delete-old

# Write and rename the original YAML configs (e.g. .yamllint.bak)
ryl --migrate-configs --migrate-write --migrate-rename-old .bak
```

To migrate your **user-global** yamllint config (the personal defaults at
`<config-dir>/yamllint/config`) to ryl's own location
(`<config-dir>/ryl/ryl.toml`), use `--migrate-user-config`:

```bash
# Preview the user-global conversion
ryl --migrate-user-config

# Write it (creating the ryl/ directory if needed)
ryl --migrate-user-config --migrate-write

# Migrate project configs and the user-global config in one run
ryl --migrate-configs --migrate-user-config --migrate-write
```

Until YAML config is removed, ryl still reads `<config-dir>/yamllint/config`
directly, with a deprecation warning naming
`ryl --migrate-user-config --migrate-write`.

Useful flags:

| Flag | Purpose |
| :--- | :--- |
| `--migrate-configs` | Migrate project-tree YAML configs, move deprecated keys in ryl TOML configs to `[lint]` and `exclude`, and move TOML `yaml-files` to `[files].yaml` |
| `--migrate-user-config` | Migrate the user-global yamllint config, and move deprecated keys in the ryl user-global TOML config to `[lint]` and `exclude` |
| `--migrate-root <PATH>` | Project search root (defaults to `.`), or a single config file |
| `--migrate-stdout` | Print generated TOML to stdout; also writes when combined with `--migrate-write` |
| `--migrate-write` | Write files (otherwise preview only) |
| `--migrate-rename-old <SUFFIX>` | Rename source YAML configs after migration; back up a ryl TOML config before rewriting it in place |
| `--migrate-delete-old` | Delete source YAML configs after migration (a rewritten TOML config is kept) |

The `--migrate-write` / `--migrate-stdout` / `--migrate-rename-old` /
`--migrate-delete-old` flags apply to whichever migration trigger
(`--migrate-configs`, `--migrate-user-config`, or both) is set; `--migrate-root`
applies to project migration only. Migration takes no positional paths: `ryl
--migrate-configs sub/` exits 2, so scope it with `--migrate-root sub/` instead.

A config passed to `-c` under any other name migrates by naming the file:
`--migrate-root ci/lint.yaml` writes `ci/lint.toml` beside it, which `-c` then takes
in its place. Update `-c` to the new path after migration; when migrating a
`YAMLLINT_CONFIG_FILE` source, remove that environment variable too. Only an
existing `ci/lint.toml` blocks that migration. A top-level TOML `yaml-files`
key is moved to `[files].yaml`, preserving its comments. If `[files].yaml` already
exists, its value wins and migration warns that `yaml-files` was discarded.

Migration skips a legacy project YAML config when project discovery already picks
a TOML config, including `.config/` candidates and `pyproject.toml` with `[tool.ryl]`,
in the same directory or an ancestor up to `HOME`. The warning names the active
TOML config; the YAML source and its lower-precedence siblings stay untouched,
even with `--migrate-delete-old` or `--migrate-rename-old`. A `pyproject.toml`
without `[tool.ryl]` does not block migration. User-global migration skips when
a ryl-native user config already exists. Migration also refuses to
follow a symlink for either the source or the target (mirroring `--fix`). It also
refuses to overwrite an existing backup when `--migrate-rename-old` would clobber
one.

Rewriting a ryl TOML config keeps its comments, key order, and formatting
outside the moved keys. If a replacement key is already set, migration warns
and drops the deprecated value and its comments. Migration reports deprecated
keys in `pyproject.toml` without rewriting that file.

Migration writes a temporary file beside each destination and replaces the
destination only after the write succeeds. An interrupted write leaves an
existing config intact. On Linux and macOS, rewriting a TOML config and creating
its backup keep the source file's owner, group, and permissions, and migration
reports an error if it cannot preserve them. On Windows they keep the read-only
attribute; other access-control entries are not copied.

Migration produces a self-contained config. yamllint's top-level `ignore` and
`ignore-from-file` become `exclude` and `exclude-from-file`, since in ryl TOML
per-rule `ignore` suppresses one rule; top-level `ignore` remains a deprecated alias
for whole-file `exclude`. A relative `extends` is flattened
(its rules are inlined), resolved relative to the config's own directory first,
then the current directory. A user-global config with a relative `ignore-from-file`,
at either the top level or rule level, is skipped with a warning: the path resolves
against each linted file's directory, which a single migrated config cannot express.
Make the path absolute or move the setting into a project config, then re-run.
Absolute top-level paths are inlined as `exclude` patterns; absolute rule-level
paths are retained. Project migration keeps a
relative `ignore-from-file` as-is, because the `.ryl.toml` stays in the same
directory.

Migration also keeps [`ryl format`](../formatter.md) from enforcing what the
yamllint config left unchecked, by writing `"preserve"` for these `[format]`
targets:

| Target | When the yamllint config |
| :--- | :--- |
| `quote-style` | leaves `quoted-strings` off, as `extends: default` does |
| `comment-starting-space` | leaves `comments` off, or sets its `require-starting-space: false` |

After migration, run `ryl check .` to confirm diagnostics match what yamllint
produced.

## What is preserved

- Rule names, defaults, and option semantics match yamllint's existing rule
  set.
- Diagnostic message text is kept aligned with yamllint where practical, so
  existing log scrapers continue to work.
- Configuration discovery walks the same locations as yamllint, with TOML
  formats checked in addition: an explicit `--config-file`, then a
  project-local `.ryl.toml`, `ryl.toml`, `.config/.ryl.toml`,
  `.config/ryl.toml`, `pyproject.toml`, `.yamllint`, `.yamllint.yaml`, or
  `.yamllint.yml`, then the `YAMLLINT_CONFIG_FILE` env var, then a
  user-global config. At the user-global step ryl checks its own
  `<config-dir>/ryl/.ryl.toml` (or `ryl.toml`) first, then falls back to
  yamllint's `<config-dir>/yamllint/config` (see
  [ryl-native user-global config](#ryl-native-user-global-config) for how
  `<config-dir>` resolves per platform).
- `YAMLLINT_CONFIG_FILE` honours only a yamllint YAML config; pointing it at a
  `.toml` errors (see
  [`YAMLLINT_CONFIG_FILE` rejects TOML](#yamllint_config_file-rejects-toml)).
- The three built-in presets &mdash; `default`, `relaxed`, and `empty` &mdash;
  match yamllint's behaviour. YAML configs can still use `extends:` to
  reference them; TOML configs must inline the `default`/`relaxed` content (see
  the example below). `empty` has no usable TOML form because ryl rejects a
  config that enables no rules. The full preset content is in
  [Configuration presets](../config-presets.md).
- Inline `# yamllint disable` / `disable-line` / `enable` comments are honoured
  with the same grammar and semantics, so existing in-file suppressions keep
  working. The equivalent `# ryl …` spelling is preferred for new files, and
  can also share a comment with other text; see
  [Inline directives](../directives.md#alongside-other-comments).

## Configuration discovery precedence

ryl resolves the configuration governing **each file** it lints, trying these
sources in order and stopping at the first hit. `-d`/`-c` and
`YAMLLINT_CONFIG_FILE` pin a single config for the whole run; otherwise project
discovery runs per file, so a monorepo can hold many configs, each governing its
own subtree. ryl recognises the same config sources as yamllint (`-d`/`-c`,
project files, `YAMLLINT_CONFIG_FILE`, a user-global config) and adds ryl-native
TOML for `-c`, project discovery, and the user-global config (but not
`YAMLLINT_CONFIG_FILE`, which stays yamllint-YAML-only). It diverges from yamllint
in two ways: yamllint folds `YAMLLINT_CONFIG_FILE` into the user-global slot (the
env var *replaces* it, and a missing target falls through to `extends: default`),
whereas ryl keeps the env var and the user-global config as distinct fall-through
tiers; and ryl requires the resolved config (or `--enable`) to enable at least
one rule or it exits `2` (yamllint instead lints with its `default` preset when nothing is found,
and silently accepts a rule-less config).

See the [configuration precedence
diagram](quickstart.md#configuration-precedence) in the Quick Start for the full
resolution flow, rendered from `-d`/`-c` through project and user-global tiers to
the rule-enabled gate.

## How ryl differs from yamllint

ryl reads yamllint configs and directives, but behaves differently in a small number
of places. Each difference is deliberate: ryl follows the YAML 1.2.2 specification and
its reference parser, reports a problem rather than ignoring it, or avoids repeated
output. The complete list:

### Rules with no yamllint equivalent

`block-scalar-chomping`, `merge-keys`, `schema-dependent-values`, `tags`,
`unicode-line-breaks` and `yaml-directive` are ryl's own rules. They are configurable only in TOML; a YAML config that names one is rejected.

### No implicit default configuration

ryl never enables a rule unless a configuration or `--enable` explicitly turns it
on. yamllint, run with no configuration, lints with its `default` preset; ryl
instead exits `2` with `no configuration found`. It also exits `2` (`configuration enables no
rules`) when a resolved config turns every rule off, where yamllint would
silently lint nothing. `--enable` supplies an explicit selection, but removing
every selected rule with `--disable` still exits `2`.

**Why ryl differs:** linting with rules the user never asked for &mdash; or
silently doing nothing &mdash; is surprising. ryl makes the rule set explicit and
reports loudly when there isn't one. The presets stay available as explicit
opt-ins, so the one-line equivalent of yamllint's out-of-the-box behaviour is a
YAML config containing `extends: default` (or the corresponding TOML preset from
[Configuration presets](../config-presets.md)). The migration converter flattens an
`extends: default` source into the generated TOML automatically, and warns when a
migrated config ends up enabling no rules.

### Directives beside other comments

yamllint honours a directive only when it is the whole comment, so
`# v1.2.3  # yamllint disable-line rule:line-length` disables nothing. ryl
keeps that for the `# yamllint …` spelling, so a migrated file behaves the
same under both tools, but accepts `# ryl …` beside other `#` text in either
order. See [Inline directives](../directives.md#alongside-other-comments).

### Tabs separating explicit keys

ryl accepts the YAML produced by `printf '%b' '{?\ta: b}\n'` and
`printf '%b' '?\tkey\n: value\n'`; yamllint reports a syntax error for both.
Here `\t` becomes a tab and `\n` becomes a newline. YAML 1.2.2
[production 142](https://yaml.org/spec/1.2.2/#rule-ns-flow-map-entry)
places `s-separate(n,c)` after `?`, and separation permits tabs outside indentation.
The [reference parser](https://play.yaml.com/#ez8JYTogYn0=) reads the flow form as
`+MAP`, `=VAL :a`, `=VAL :b`, `-MAP`; [the block form](https://play.yaml.com/#PwlrZXkKOiB2YWx1ZQo=)
emits `+MAP`, `=VAL :key`, `=VAL :value`, `-MAP`. Tabs used as indentation remain invalid.

### A tab-only line inside a flow collection

For `printf -- '- [\n\t\n foo\n ]\n'`, yamllint reports `found character '\t' that
cannot start any token`; ryl parses the file. The
[reference parser](https://play.yaml.com/#LSBbCgkKIGZvbwogXQo=) emits `+SEQ`, `+SEQ []`,
`=VAL :foo`, `-SEQ`, `-SEQ`: inside a flow collection a tab is separation whitespace,
YAML 1.2.2 [production 66](https://yaml.org/spec/1.2.2/#rule-s-separate-in-line).

### `?` directly before a flow indicator

`ryl check` reports a syntax error for `[?,]`, `[?]` and `{?}`; yamllint (via PyYAML)
reads `[?,]` as `[{null: null}]`. YAML 1.2.2
[production 126](https://spec.yaml.io/main/spec/1.2.2/#rule-ns-plain-first) lets `?` start a plain
scalar only when an `ns-plain-safe` character follows, which excludes `, [ ] { }` in flow
context, and an explicit key needs separation after the `?`. The
[reference parser](https://play.yaml.com/#Wz8sXQo=) rejects `[?,]`; with a space,
[`[? ,]`](https://play.yaml.com/#Wz8gLF0K) is a valid empty key and value. `ryl format`
and `ryl check --fix` read the input as PyYAML does and repair it: `[?,]` becomes `[? ]`.

### Anchor and alias names containing a colon

YAML 1.2.2 §6.9.2 defines an anchor/alias name as `ns-anchor-char+`, and
`ns-anchor-char` excludes only the flow indicators `, [ ] { }` &mdash; so a colon
is a **legal name character**. ryl's parser and the
[YAML reference parser](https://play.yaml.com) both read a `:` as part of the
name; yamllint (via PyYAML) stops at the first `:`, which the specification does
not permit. ryl follows the spec, so the `anchors` rule can diverge:

| Input (with `&x`/`&foo…` as noted) | ryl (spec-conformant) | yamllint (PyYAML) |
| :--- | :--- | :--- |
| `b: {*x: 2}` | `*x:` is an **undeclared** alias; `x` is **unused** | reads `{x: 2}` &mdash; no diagnostic |
| `&foo:bar` and `&foo:baz` | two **distinct** anchors | one anchor `foo`, reported **duplicated** |
| `*foo:baz` | a valid alias named `foo:baz` | a **syntax error** (PyYAML cannot scan it) |

The *unused* note in row 1 and the *duplicated* report in row 2 require the matching
non-default `anchors` options (`forbid-unused-anchors`, `forbid-duplicated-anchors`); the
colon-in-name parsing divergence the table illustrates is independent of them.

The `colons` rule follows the same reading. It checks the colons the parser sees as
mapping indicators:

| Input | ryl | yamllint |
| :--- | :--- | :--- |
| `&a : v`, `!t : v` | the space before `:` is required: `&a: v` is the scalar `v` anchored `a:` | too many spaces before colon |
| `*a :  v` | too many spaces after colon | nothing: an alias key exempts both sides |
| `*a:  v` | a syntax error: `:` is part of the alias name | too many spaces after colon |

**Why ryl differs:** the YAML specification and its reference parser are the
authority, and PyYAML's narrowing at `:` is non-conformant (see
[adrienverge/yamllint#686](https://github.com/adrienverge/yamllint/issues/686) and
[adrienverge/yamllint#780](https://github.com/adrienverge/yamllint/issues/780)).
The portable, unambiguous way to use an alias as a mapping key is a **space before
the colon** &mdash; `*foo : bar` &mdash; which every parser reads identically. The
ryl-only [`anchors: forbid-ambiguous-anchor-alias-names`](../rules/anchors.md)
option flags welded colons so you can forbid the construct outright.

The TOML-only [`anchors: forbid-anchors-and-aliases`](../rules/anchors.md)
option prohibits every anchor and alias, including aliases in merge keys. The
option operates independently of the other anchor checks, so several conditions
can produce diagnostics at the same token; yamllint has no equivalent policy option.

### De-duplicated inputs

When a single run names the same file more than once &mdash; listed twice, or reached
by both a directory argument and an explicit path (`ryl check . file.yaml`) &mdash; ryl
processes it **once**. yamllint handles each occurrence, so it would report that
file's diagnostics (or, under `ryl check --diff`, emit its patch) twice.

**Why ryl differs:** duplicate output is never useful, and it is actively harmful for
`--diff`, whose output is meant to be applied as a patch &mdash; a repeated patch
block fails to apply on the second copy. ryl normalizes each input path (lexically,
without resolving symlinks, so a symlink and its target stay distinct) and skips a
file it has already selected.

### Version control directories

A directory walk skips any `.git`, `.hg`, `.svn` or `.jj` directory below the input.
yamllint walks into them and lints any YAML there. Naming one as an input
(`ryl check .git`) or naming a file inside one still lints it.

**Why ryl differs:** a VCS directory holds the tool's own state, never project YAML,
and walking a large `.git` costs time. ruff skips `.git`, `.hg` and `.svn`; ryl also
skips `.jj`. Other hidden directories, such as `.github`, are still walked.

### Bare carriage-return (`\r`) line breaks

YAML 1.2.2 §5.4 defines a line break as `( CR LF ) | CR | LF`, so a **bare `\r`**
(a carriage return not followed by a line feed) is a complete line break on its own.
ryl honours this **everywhere** &mdash; every rule treats `\r`, `\r\n`, and `\n`
identically. yamllint's line layer counts `\n` only (a bare `\r` is ordinary
content), so on a classic-Mac `\r`-only file the two disagree:

| On a bare-`\r` line | ryl (YAML 1.2) | yamllint (`\n`-only) |
| :--- | :--- | :--- |
| spaces before the `\r` | `trailing-spaces` flags them | not flagged |
| a long line ending at `\r` | `line-length` measures to the `\r` | counts across the `\r` |
| a file ending in `\r` | `new-line-at-end-of-file` is satisfied | reports "no new line" |
| the file's first break is `\r` | `new-lines` flags it (not `unix`/`dos`) and `--fix` rewrites it | invisible (no `mac` type) |
| line/column of any diagnostic | counted CR-aware | shifted by uncounted `\r`s |

**On supported LF and CRLF files this is identical to yamllint** &mdash; the two
line-break definitions agree exactly when there is no bare `\r` (one CRLF exception
follows below). The divergence is
visible only on `\r`-only (or mixed-`\r`) files, which yamllint cannot lint
faithfully anyway (and whose `new-lines` `type` has no `mac` value). ryl follows the
specification and its [reference parser](https://play.yaml.com), which rank above
yamllint as the authority.

Two editing-path caveats on such files. First, `ryl check --diff` skips a file whose
content *ends* in a bare `\r` (a unified diff is `\n`-terminated by format, so that
one case has no applicable patch &mdash; use `ryl check --fix`, which rewrites bytes
directly); a bare `\r` elsewhere is diffed as line content and applies via
`git apply`. Second, **Markdown embedding** requires a Markdown file with no bare `\r` anywhere:
the fenced-block and front-matter parser
([`pulldown-cmark`](https://github.com/raphlinus/pulldown-cmark)) does not implement
CommonMark §2.1's bare-`\r` line ending, so a bare `\r` used as a host line ending
hides the YAML blocks from it, and a bare `\r` *inside* an embedded block (which the
parser keeps verbatim) can't be mapped back to a host position by ryl's `\n`-based
remap. Rather than silently check nothing — or report a wrong position — ryl reports
an error (and `--fix`/`--diff` skip with a notice) telling you to convert the file to
LF or CRLF. The YAML *inside* an LF/CRLF Markdown host (itself free of bare `\r`) is
linted CR-aware like any other.

### Strict indentation

ryl rejects under-indented flow collections and scalar continuations that yamllint's
PyYAML parser accepts. For yaml-test-suite `9C9N`, `flow: [a,\nb,\nc]`, the reference
parser reports a parse error; ryl reports a syntax error until the indentation is fixed.
For `ryl check` on YAML files, a repairable diagnostic names `ryl check --fix`
or `ryl format`; a refused repair has no command hint.
YAML 1.2.2's [separation production 81](https://yaml.org/spec/1.2.2/#rule-s-separate-lines),
`s-separate-lines(n) ::= (s-l-comments s-flow-line-prefix(n)) | s-separate-in-line`,
requires `s-flow-line-prefix(n)` inside a flow collection, keeping continuation content
deeper than the enclosing block.
ryl therefore places a flow closing `]` or `}` at least one column past the enclosing
block, where yamllint may request the block's column.

`ryl format`, `check --fix`/`--diff` and editor formatting may repair relaxed-parser
input only when the output parses strictly and preserves the input's relaxed events.
Repairs print a notice; `format --check` exits 1. Otherwise the input stays unchanged
with a syntax skip. See the [formatter guarantees](../formatter.md#guarantees).

### Document-prefix byte order marks

ryl accepts `a: a\n...\n\uFEFF---\na: 1\n` and treats the BOM as an encoding
prefix, not indentation. yamllint rejects the later BOM-prefixed marker as a syntax
error. YAML 1.2.2 [document-prefix production
202](https://yaml.org/spec/1.2.2/#911-document-prefix) permits a BOM before each
document; the reference parser emits two documents, with `+DOC ---` for the second.

### Blank CRLF lines in multi-line scalars

With `indentation: check-multi-line-strings` enabled, yamllint flags a blank line inside
a multi-line scalar in a CRLF file, because it only recognises `\n` as ending a line:

```yaml
key: |
  one

  two
```

Saved with CRLF endings, yamllint reports `3:1 wrong indentation: expected 2 but found 0`
and ryl reports nothing. ryl treats CRLF as a line break, as YAML 1.2 requires, so the
blank line is skipped exactly as it is in an LF file.

### Block scalar at EOF

An unterminated clip or keep block scalar body, such as `a: |\n  a`, gains a
final newline under `ryl check --fix` or `ryl format`. With document ends enabled,
it also gains `...`. The YAML 1.2 reference parser reads `=VAL |a\n` before and
after; PyYAML reads `"a"` before and `"a\n"` after. ryl follows the reference
parser for this value, including when the scalar stands alone.
YAML 1.2.2 [production 165](https://yaml.org/spec/1.2.2/#rule-b-chomped-last)
allows either `b-as-line-feed` or end of input for clip and keep chomping.

For `- |+\n   ` (a spaces-only last line without a line break), `ryl format`
and `ryl check --fix` with `trailing-spaces` enabled write `- |+\n\n`.
The YAML 1.2 reference parser and eemeli `yaml` read `"\n"` before and after;
PyYAML and ruamel read `""` before and `"\n"` after.
After a content line, as in `a: |+\n  h\n `, ryl-yaml-parser also drops that last
line and reads `"h\n"` where the reference parser reads `"h\n\n"`; the formatter
keeps the reference value.

With `[format] document-end = 'add'`, `'#hash': |+\n\n    ` (also
with CRLF line breaks) becomes `'#hash': |+\n\n\n...\n`.
The reference parser reads `"\n\n"` before and after; eemeli `yaml`, PyYAML,
and ruamel read `"\n"` before and `"\n\n"` after. ryl follows the
reference parser for this EOF shape.

**Why ryl differs:** YAML 1.2.2's
[keep-empty production](https://yaml.org/spec/1.2.2/#rule-l-keep-empty)
(`l-chomped-empty(n,KEEP) ::= l-keep-empty(n)`) and the reference parser's
`=VAL |\n` event rank above PyYAML here. In a local measurement of the
2022 yaml-test-suite, eemeli `yaml` rejected 93/94 invalid cases and matched
272/279 valid cases; PyYAML and ruamel rejected 82/94 invalid cases and matched
210–222/279 valid cases.

### Whitespace-only lines inside folded scalars

For `a: >\n  a\n\n\n    \n\n\n\n`, `ryl format` removes the last three
empty lines. The reference parser emits `=VAL >a\n\n\n\n  \n` before and
after; ryl-yaml-parser, eemeli `yaml`, PyYAML and ruamel read `"a\n\n\n  \n"` on
both sides. CRLF and CR line endings have the same meaning.
The formatter follows the reference parser's equal-value verdict.
YAML 1.2.2's [folded content production](https://yaml.org/spec/1.2.2/#rule-l-folded-content)
(`l-folded-content(n,t) ::= (l-nb-diff-lines(n) b-chomped-last(t))?
l-chomped-empty(n,t)`) separates content from chomped trailing empty lines.

### Spaces in a blank-only block scalar

For `a: |+\n  \nb: 1\n`, the reference parser emits `=VAL | \n`: one
space and a line break belong to the scalar. The same space survives with a
bare `|`, folded `>`, or a nested `- |+` body. `ryl format` and
`ryl check --fix` preserve it, and `trailing-spaces` does not report it;
yamllint reports line 2 as trailing spaces. YAML 1.2.2's
[auto-detected indentation](https://yaml.org/spec/1.2.2/#rule-s-indent-less-than)
and [block scalar content](https://yaml.org/spec/1.2.2/#rule-c-l+literal)
productions leave the spaces beyond the content indent in the value.
For `a:\n  b: |-\n    \n\nc: x\n`, the reference parser reads
`=VAL |    \n\nc: x`; ryl leaves the file containing this ambiguous nested scalar unchanged.
That reading detects a negative indentation from the dedented `c`; ryl's value tests
floor it at 1, as the reference parser already does for 0, so a sibling key never
becomes scalar content.
It also refuses a blank-only scalar followed by a sibling key at the same
indent when moving the sequence dash would change the reference value.
ryl also refuses to normalize `? a\n:   -   >\n    \n    - x\n`:
moving the scalar header would turn the blank line's spaces into content.

### Marker-like root strings and Unicode whitespace

Root strings such as `'---'` and `'...'` keep their quotes. Marker prefixes followed
by Unicode whitespace or a BOM also keep quotes, including `---` followed by U+00A0
and `x`. The reference parser emits `=VAL '---\_x` for the quoted form, but only
`+STR -STR` and a parse error for the plain form; ryl-yaml-parser and resolving loaders accept
the plain form as a string. Formatting refuses rewrites that introduce such root lines.

YAML 1.2.2's [production 206, `c-forbidden`](https://yaml.org/spec/1.2.2/#rule-c-forbidden)
checks `b-char | s-white | <end-of-input>` after a column-zero marker; `s-white` is
ASCII space or tab. Keeping the original quotes remains safe under both the grammar
and the reference parser's broader whitespace lookahead.

A BOM inside a scalar retains quotes in every position, including a joined
continuation: [production 27, `nb-char`](https://yaml.org/spec/1.2.2/#rule-nb-char)
excludes `c-byte-order-mark` from plain and block scalar content. The reference
parser rejects a plain scalar containing U+FEFF; the quoted form preserves the character.

### Root block scalar with an explicit indentation indicator

For `|2\n  x\n  `, the reference parser emits `=VAL | x\n \n`:
the second space on each body line is content. The same reading applies to
`|-2`, `>2`, `|2+`, and a root scalar in a multi-document stream.
`ryl format` and `ryl check --fix` preserve the final space, and
`trailing-spaces` does not report it; yamllint reports that last line.
YAML 1.2.2's [bare-document production](https://yaml.org/spec/1.2.2/#rule-l-bare-document)
places the root node at indentation `-1`, so `|2` gives a content indent of one.

### `%YAML` version-directive handling

ryl honours the `%YAML` directive (YAML 1.2.2 §6.8.1) per document: a
directive-less document and `%YAML 1.2` resolve under the 1.2 core schema, an
explicit `%YAML 1.1` resolves under 1.1, a higher major version (`%YAML 2.0`)
is rejected, and a higher minor version (`%YAML 1.3`) is processed as 1.2 with
a warning. yamllint (via PyYAML) resolves implicit types under 1.1 regardless of
the directive, so the two disagree:

| Input | ryl | yamllint |
| :--- | :--- | :--- |
| `'+.5'` with no directive, `quoted-strings: only-when-needed` | kept (1.2 float) | redundant (1.1 string) |
| `'no'` without `%YAML 1.2`, same rule | kept (1.1 boolean) | kept |
| `'no'` under `%YAML 1.2` or `target-version = "1.2"`, same rule | redundant (1.2 string) | kept |
| `'y'` without `%YAML 1.2`, same rule | kept (go-yaml v2 boolean) | redundant (PyYAML string) |
| `'08'` under `%YAML 1.1`, same rule | kept (go-yaml float) | redundant (PyYAML string) |
| plain `yes` under `target-version = "1.2"`, `truthy` | no diagnostic (1.2 string) | flagged |
| `%YAML 1.3` document | warning, processed as 1.2 | no diagnostic |
| `%YAML 2.0` document | rejected (`syntax`) | rejected (`syntax`) |

**Why ryl differs:** the spec is the authority. It assigns a directive-less
document to 1.2, directs a 1.2 processor to honour `%YAML 1.1`, and mandates
rejecting a higher major version. Unless a document declares `%YAML 1.2` or
`target-version = "1.2"` is set, `quoted-strings` keeps PyYAML's 1.1 set, so
`--fix` never strips quotes a 1.1 reader needs, nor quotes a 1.2 reader needs.
ryl adds `y`, `Y`, `n` and `N` to that set: PyYAML loads them as strings, but
go-yaml v2, which Kubernetes, Helm and kubectl use, loads them as booleans, as
the YAML 1.1 spec does. So `'y'` keeps its quotes, and `required: true` leaves a
plain `y` alone as it does a plain `no`. go-yaml's float spellings (`08`, `1e5`,
`-.5`) join the set the same way. `target-version = "1.2"` declares the 1.1
readers out, so `truthy` then flags only the 1.2 booleans; yamllint has no such
setting.

### ytt directives

ryl's `comments` rule skips ytt `#@` directives, including their inline spacing.
yamllint reports missing comment spacing there. Inserting a space after `#` turns
the directive into a plain comment, changing what ytt renders; see
[ytt annotations](https://carvel.dev/ytt/docs/v0.52.x/yaml-primer/).

### Comments after a block scalar header

ryl's `comments` rule checks a comment that follows a block scalar header (`|`, `>`
and their indicators) like any other inline comment, so `--fix` pads it too.
yamllint never sees that comment: PyYAML consumes it as part of the scalar token,
so both `require-starting-space` and `min-spaces-from-content` skip it.

```yaml
d: | # c
  text
e: >- #c
  more
```

| Line | ryl (`comments` defaults) | yamllint |
| :--- | :--- | :--- |
| 1 (`d:`) | too few spaces before comment | no problem |
| 3 (`e:`) | too few spaces before comment; missing starting space | no problem |

**Why ryl differs:** the YAML 1.2.2 spec ends the header with an ordinary inline
comment, `c-b-block-header(t) ::= (indicators) s-b-comment`
([production 162](https://yaml.org/spec/1.2.2/#rule-c-b-block-header)), and its own
[Example 8.1](https://yaml.org/spec/1.2.2/#example-block-scalar-header) writes
`- | # Empty header`. yamllint's exemption is a side effect of reading comments only
between tokens, not a style choice.

### Comments ending a file with no final newline

For `printf 'a: 1\n# note'`, yamllint reports `comment not indented like content` on
the first comment of a block that ends the file without a newline, and accepts one at
the column where the file ends; ryl checks those comments against the content, as
anywhere else. yamllint compares them with the stream end, whose column is the last
line's length.

### A repeated `...` marker

For `printf -- '---\na: 1\n...\n\n...\n'`, yamllint reports `missing document start
"---"` on the second `...`; ryl reports nothing. The
[reference parser](https://play.yaml.com/#LS0tCmE6IDEKLi4uCgouLi4K) reads one document,
and YAML 1.2.2's [stream production](https://yaml.org/spec/1.2.2/#rule-l-yaml-stream)
allows repeated document suffixes with no document between them.

### Per-line ignores

ryl adds a [`per-line-ignores`](../per-line-ignores.md) config table with no
yamllint counterpart: it suppresses chosen rules on lines (and/or files) matching a
regex/glob, the config-level complement to inline `# ryl disable-line`. yamllint
offers only inline directives and per-file path ignores, so a recurring exception
(every `#cloud-config`, every `# renovate:` marker) must be annotated in each file
or handled per-rule. Being ryl-only, it lives in TOML config and is rejected in
yamllint-compatible YAML config.

### Comment alignment to an open block level

yamllint's `comments-indentation` requires a standalone comment to match the
indentation of the surrounding content; at a dedent the preceding block's indentation
is also accepted. ryl adds a ryl-only, off-by-default
[`comments-indentation: allow-any-open-indent`](../rules/comments-indentation.md)
option that *also* accepts a comment aligned to any still-open enclosing block
level &mdash; the pattern requested in
[adrienverge/yamllint#141](https://github.com/adrienverge/yamllint/issues/141),
which the maintainer welcomed but yamllint has not implemented:

```yaml
config:
    entry:
        - things
    # accepted under the option: marks the end of `entry:` at its open level
options:
    - more stuff
```

With the option off (the default) ryl matches yamllint exactly. Being ryl-only, the
option is configured in TOML and rejected in yamllint-compatible YAML config.

### Dash on its own line before a block mapping

yamllint's `hyphens` rule only limits the spaces after a `-`. ryl adds a ryl-only,
off-by-default [`hyphens: dash-on-own-line`](../rules/hyphens.md) option that requires a
block-sequence entry's `-` to be on its own line when the entry is a block mapping
&mdash; the spec-style "sequence of mappings" layout requested in
[adrienverge/yamllint#527](https://github.com/adrienverge/yamllint/issues/527), which
the maintainer welcomed but yamllint has not implemented:

```yaml
items:
  - name: web    # flagged under the option: mapping starts on the dash line
    port: 80
  -
    name: db     # accepted: dash alone, mapping body indented below
    port: 90
```

With the option off (the default) ryl matches yamllint exactly. Being ryl-only, the
option is configured in TOML and rejected in yamllint-compatible YAML config.

### Maximum spaces before an inline comment

yamllint's `comments` rule only sets a minimum gap between content and an inline
comment. ryl adds a ryl-only, off-by-default
[`comments: max-spaces-from-content`](../rules/comments.md) option that also caps it, and
`--fix` trims a wider gap to the cap:

```yaml
first: value        # flagged with max-spaces-from-content = 2
second: value  # accepted
```

With the option off (the default) ryl matches yamllint exactly. Being ryl-only, the
option is configured in TOML and rejected in yamllint-compatible YAML config.

### JUnit and GitLab report formats

yamllint offers `standard`, `parsable`, `colored`, `github`, and `auto` output formats.
ryl keeps those and adds two machine-readable report formats: `--output-format junit`
(JUnit XML) and `--output-format gitlab` (GitLab Code Quality JSON), which write to stdout
(or to a file with `-o`/`--output-file`) so a Git forge can ingest them as a report
artifact. ryl names the flag `--output-format`, like ruff, and keeps yamllint's
`-f`/`--format` as aliases. Going beyond yamllint, `--output-format` is repeatable and
each pairs with its own `--output-file`, so a single
run can emit console diagnostics **and** one or more report files; the same targets can be
set in a ryl-only TOML `[output]` table. See [Output formats](../output-formats.md).

### ryl-native user-global config

yamllint's only user-global config lives at `$XDG_CONFIG_HOME/yamllint/config` (else
`~/.config/yamllint/config` on every platform). ryl still reads that path for migrators,
but checks its own `<config-dir>/ryl/.ryl.toml` (or `ryl.toml`) first, following the
ruff/Biome convention: `<config-dir>` is `$XDG_CONFIG_HOME` if set, else the platform-native
config dir (`~/.config/ryl` on Linux, `~/Library/Application Support/ryl` on macOS,
`%APPDATA%\ryl` on Windows). Being ryl-only, the ryl-native path is TOML; the
yamllint-compatible path stays YAML and, matching yamllint, always resolves under
`$XDG_CONFIG_HOME` or `~/.config` (not the native dir).

### `YAMLLINT_CONFIG_FILE` rejects TOML

`YAMLLINT_CONFIG_FILE` is yamllint's env var, so ryl honours it only as a yamllint
**YAML** config. Pointing it at a `.toml` errors (even when the file is absent),
directing you to `-c`/`--config-file` or project discovery for ryl-native TOML. A
YAML (or extension-less) target keeps full yamllint compatibility, including
yamllint's behaviour of **silently ignoring a missing target** and falling through
to the next config source.

| `YAMLLINT_CONFIG_FILE` target | ryl | yamllint |
| :--- | :--- | :--- |
| present `.toml` | **error** (use `-c`) | parsed as YAML (not TOML) |
| missing `.toml` | **error** (use `-c`) | ignored, falls through |
| missing `.yml`/`config` | ignored, falls through | ignored, falls through |

**Why ryl differs:** TOML is never a valid yamllint config, so there is no
compatibility to preserve for a `.toml` target &mdash; loading one through the
yamllint-named var was an accidental overlap with ryl's own format. ryl fails loudly
on it and keeps ryl-native TOML on its dedicated channels (`-c`/`--config-file`,
project discovery). yamllint's `-c` flag likewise errors on a missing file (only its
env var is lenient), and biome's `BIOME_CONFIG_PATH` errors on a missing target
outright, so failing loudly on the always-wrong `.toml` case is well-precedented.

## Side-by-side example

=== "yamllint (.yamllint)"

    ```yaml
    extends: default

    rules:
      line-length:
        max: 120
        allow-non-breakable-words: true
      quoted-strings:
        quote-type: double
        required: only-when-needed
      truthy: disable
    ```

=== "ryl (.ryl.toml)"

    ```toml
    [files]
    yaml = [
        "*.yaml",
        "*.yml",
        ".yamllint",
    ]

    [lint.rules]
    anchors = "enable"
    braces = "enable"
    brackets = "enable"
    colons = "enable"
    commas = "enable"
    document-end = "disable"
    empty-lines = "enable"
    empty-values = "disable"
    float-values = "disable"
    hyphens = "enable"
    indentation = "enable"
    key-duplicates = "enable"
    key-ordering = "disable"
    new-line-at-end-of-file = "enable"
    new-lines = "enable"
    octal-values = "disable"
    trailing-spaces = "enable"
    truthy = "disable"

    [lint.rules.comments]
    level = "warning"

    [lint.rules.comments-indentation]
    level = "warning"

    [lint.rules.document-start]
    level = "warning"

    [lint.rules.line-length]
    max = 120
    allow-non-breakable-words = true

    [lint.rules.quoted-strings]
    quote-type = "double"
    required = "only-when-needed"
    ```

!!! note "TOML configuration is self-contained"

    TOML configuration does **not** support `extends`. Presets are expanded
    inline so the entire effective rule set is visible in one file. The
    `--migrate-configs` flag handles the expansion for you; if you write a
    TOML config by hand, start from the preset content in
    [Configuration presets](../config-presets.md).

## Optional: configure auto-fixes

TOML configurations can declare which rules are eligible for `ryl check --fix`:

```toml
[lint]
fixable = ["ALL"]
unfixable = ["comments"]
```

This is a ryl-only feature. See the [Rules reference](../rules.md) for the
list of rules that support automatic fixing.

yamllint has no formatter. ryl adds [`ryl format`](../formatter.md), which applies the
layout fixes to every file without enabling the rules. yamllint's `default` preset
conflicts with none of the formatter's default settings, so a migrated config needs no
changes to run alongside it.

## Keeping both files

`.yamllint` and `.ryl.toml` can coexist for separate tools. When both are
present in the same directory, ryl uses TOML exclusively and ignores `.yamllint`.
Keep shared lint settings synchronized: `--migrate-configs` skips `.yamllint` when
a `.ryl.toml` already exists.
