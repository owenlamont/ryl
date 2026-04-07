# ryl Rules

`ryl` aims for parity with [yamllint](https://yamllint.readthedocs.io/en/stable/rules.html).

## Supported Rules

The following rules are implemented and can be configured in your `.ryl.toml` (or YAML equivalent).

| Rule | Description | Fixable |
| :--- | :--- | :---: |
| `anchors` | Checks for anchor and alias definitions. | |
| `braces` | Checks for spaces inside braces. | |
| `brackets` | Checks for spaces inside brackets. | |
| `colons` | Checks for spaces around colons. | |
| `commas` | Checks for spaces around commas. | |
| `comments` | Checks for spaces after `#` and before `#`. | ✅ |
| `comments-indentation` | Checks for indentation of comments. | |
| `document-end` | Checks for the document end marker `...`. | |
| `document-start` | Checks for the document start marker `---`. | |
| `empty-lines` | Checks for the number of empty lines. | |
| `empty-values` | Checks for empty values in mappings. | |
| `float-values` | Checks for float value formats. | |
| `hyphens` | Checks for spaces around hyphens in sequences. | |
| `indentation` | Checks for consistent indentation. | |
| `key-duplicates` | Checks for duplicate keys in mappings. | |
| `key-ordering` | Checks for alphabetically ordered keys. | |
| `line-length` | Checks for maximum line length. | |
| `new-line-at-end-of-file` | Checks for a single newline at the end of the file. | ✅ |
| `new-lines` | Checks for consistent line endings (LF vs CRLF). | ✅ |
| `octal-values` | Checks for octal value formats. | |
| `quoted-strings` | Checks for quoted string styles. | |
| `trailing-spaces` | Checks for trailing whitespace. | |
| `truthy` | Checks for truthy values (e.g., `true`, `false`). | |

## Rule Configuration

Most rules follow the standard `yamllint` configuration. For detailed options,
refer to the [yamllint documentation](https://yamllint.readthedocs.io/en/stable/rules.html).

### Example: `line-length`

```toml
[rules.line-length]
max = 120
allow-non-breakable-words = true
```

## Automatic Fixing

`ryl` supports automatic fixing for specific rules when the `--fix` flag is used.

### Fixable Rules

- `comments`
- `new-lines`
- `new-line-at-end-of-file`

### Configuring Fixes (TOML only)

You can control which rules are automatically fixed in your `.ryl.toml`:

```toml
[fix]
fixable = ["ALL"]    # Fix all supported rules
unfixable = ["comments"] # Except for comments
```
