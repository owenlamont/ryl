# unicode-line-breaks

## What this rule does

Reports raw occurrences of three Unicode characters that YAML 1.1 treated as
line breaks but YAML 1.2 does not:

| Character | Name | YAML escape |
| :--- | :--- | :--- |
| `U+0085` | next line (NEL) | `\N` |
| `U+2028` | line separator | `\L` |
| `U+2029` | paragraph separator | `\P` |

The rule scans the decoded source text and flags every raw occurrence, wherever
it appears — inside a scalar, a key, or a comment. It is **off by default**.

## Why this matters

YAML 1.1 treated a broad Unicode set as line breaks, including NEL, LS, and PS.
YAML 1.2 narrowed line breaks to just line feed (`\n`) and carriage return
(`\r`), and the 1.2.2 changes page records that these three characters "are no
longer considered line-break characters." ryl targets YAML 1.2.

This makes a raw occurrence a portability trap: a YAML 1.1 parser splits the
line where a 1.2 parser keeps the character as ordinary scalar content, so the
same bytes can produce a different parsed structure depending on the tool. The
characters are also invisible in virtually every editor, so a stray one — pasted
in from a word processor, a PDF, or a web page — is almost impossible to spot by
eye.

If you genuinely need one of these characters, YAML 1.2 gives each a dedicated
escape (`\N`, `\L`, `\P`) that includes it visibly and portably inside a
double-quoted scalar.

Sources: YAML 1.2.2 changes page; YAML 1.2.2 spec §5.1 (character set), §5.4
(line-break characters), §5.7 (escaped characters).

## Configuration

`unicode-line-breaks` is a ryl-only rule (yamllint has no equivalent), so it is
configured **only in TOML** &mdash; `[rules.unicode-line-breaks]` in
`.ryl.toml`/`ryl.toml` or `[tool.ryl.rules.unicode-line-breaks]` in
`pyproject.toml`. It is rejected in yamllint-compatible YAML config (including
`-d` data) so the YAML namespace stays reserved for any future yamllint rule.

```toml
[rules.unicode-line-breaks]
level = "error"
```

The rule has no options: when enabled it flags all three characters everywhere.

## Examples

### :x: Reported

A double-quoted scalar containing a raw `U+2028` (shown here as `<LS>`; in a real
file the character is invisible):

```yaml
title: "first line<LS>second line"
```

```text
1:19  error  forbidden raw line separator U+2028; escape as "\L" in a double-quoted scalar  (unicode-line-breaks)
```

The rule also fires on a raw `U+0085`/`U+2029` in a plain scalar or a comment.

### :white_check_mark: Allowed

Use the dedicated escape inside a double-quoted scalar:

```yaml
title: "first line\Lsecond line"
```

## Automatic fixing

This rule does not auto-fix. The escape (`\N`/`\L`/`\P`) is only valid inside a
double-quoted scalar, so rewriting a plain or single-quoted scalar, a comment,
or a block scalar would require changing the quoting style or guessing intent;
no single rewrite is universally safe.

## Related rules

- [`new-lines`](new-lines.md) &mdash; enforces a consistent *line ending*
  (LF vs CRLF) for the real line breaks.
- [`quoted-strings`](quoted-strings.md) &mdash; governs the quoting style you
  need in order to write a `\L`/`\N`/`\P` escape.
