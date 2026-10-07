//! Every ryl-config example in the docs' Markdown sources must be config the loader
//! actually accepts. Validation goes through ryl's *finalized* config path
//! (`discover_config`, the same one the CLI uses), not the JSON schema (which is
//! generated from the loader) and not a parse-only load: finalizing is what rejects
//! misspelled rule names, while parsing rejects misspelled rule options and bad
//! values, so both classes of typo in a docs example are caught. The "no rules
//! enabled" gate lives above `discover_config`, so `[lint]`/`[files]`-only fragments
//! still validate. Only the `.md` sources are scanned: `docs/llms*.txt` are generated
//! from them and held in lockstep by a separate drift guard, so the sources cover them.
//!
//! Not every fenced block is ryl config (docs also carry rule-input YAML, other
//! tools' TOML, and so on) and a block need not even parse, so each is *classified
//! from its content* by structural markers keyed to ryl's config schema:
//!   - a `toml` block is ryl config when it declares a `[tool.ryl]` table (the
//!     `pyproject.toml` form) or a table whose top-level name is in the TOML config
//!     schema (`[lint.rules]`, `[[lint.per-line-ignores]]`, `[output.gitlab]`, ...);
//!     other TOML (a `prek.toml`, a `Cargo.toml`) is skipped;
//!   - a `yaml` block is ryl config when a top-level mapping key is in the YAML
//!     config schema (`rules:`, `extends:`, ...); rule-input examples are skipped.
//!
//! Detection is deliberately structural rather than a parse, so a *malformed* config
//! example (which would not parse) is still recognised by its headers and routed to
//! the loader, which reports the error, rather than being silently skipped. The
//! unavoidable limit of any such heuristic: a broken block with no recognisable ryl
//! header is indistinguishable from another tool's TOML and is treated as non-config.
//!
//! A `<!-- ryl-config-check: skip -->` comment on the line before a fence overrides
//! detection for an intentional counter-example (e.g. the YAML-1.1 config in
//! `yaml-version.md` whose prose says it "will fail to parse in ryl").
//! `<!-- ryl-config-check: format-clean -->` additionally requires the config to draw no
//! `ryl format` conflict warning and to lint [`FORMAT_SAMPLE`] clean once formatted, and
//! `<!-- ryl-config-check: format-conflict -->` requires at least one warning.

use ryl::config::{Overrides, YamlLintConfig, discover_config};
use ryl::{format, lint_str};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

const MARKERS: [(&str, Marker); 3] = [
    ("<!-- ryl-config-check: skip -->", Marker::Skip),
    (
        "<!-- ryl-config-check: format-clean -->",
        Marker::FormatClean,
    ),
    (
        "<!-- ryl-config-check: format-conflict -->",
        Marker::FormatConflict,
    ),
];

/// Formatter input whose output a `format-clean` config must lint clean: the quoted
/// strings are ones YAML 1.1 would read as booleans.
const FORMAT_SAMPLE: &str = "country: \"NO\"\nenabled: \"yes\"\nswitch: \"on\"\n\
    base: &base {a: 1,b: [ 1,2 ]}\nchild: *base\nplain: \"x\"   #note\n\n\n\nlast: 'it''s'\nsaid: 'it''s: x'\n";

#[derive(Clone, Copy, PartialEq, Debug)]
enum Marker {
    None,
    Skip,
    FormatClean,
    FormatConflict,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    /// Standalone `.ryl.toml`-style config.
    Toml,
    /// `pyproject.toml` with a `[tool.ryl]` table.
    Pyproject,
    /// yamllint-style YAML config.
    Yaml,
    /// Not ryl config (other tool, rule-input example) or explicitly skipped.
    NotConfig,
}

struct Block {
    lang: String,
    content: String,
    marker: Marker,
}

/// Top-level property names of a config schema produced by
/// [`ryl::config_schema`]: the source of truth for which tables/keys mark a
/// block as ryl config.
fn schema_top_level_keys(schema: serde_json::Value) -> BTreeSet<String> {
    schema["properties"]
        .as_object()
        .expect("a config schema should expose top-level properties")
        .keys()
        .cloned()
        .collect()
}

/// Strip up to `indent` leading space/tab characters (the fence's own
/// indentation), leaving more-indented and blank lines intact. The stripped
/// characters are ASCII whitespace, so the byte count equals the char count.
fn dedent(line: &str, indent: usize) -> &str {
    let strip = line
        .chars()
        .take(indent)
        .take_while(|c| *c == ' ' || *c == '\t')
        .count();
    &line[strip..]
}

/// The number of leading backticks on a line (after indentation): the fence
/// length. A run of three or more opens or closes a fence.
fn backtick_run(line: &str) -> usize {
    line.trim_start().chars().take_while(|c| *c == '`').count()
}

/// A line that closes a fence opened with `open_len` backticks: at least as many
/// backticks, with nothing but optional whitespace after them. An info string
/// only ever appears on an opener, so a longer fence (e.g. a nested ```` block)
/// is not closed by an inner ``` line.
fn is_closing_fence(line: &str, open_len: usize) -> bool {
    let len = backtick_run(line);
    len >= open_len && line.trim_start()[len..].trim().is_empty()
}

/// Extract fenced code blocks, dedenting each by its fence indentation and
/// recording whether the immediately-preceding line carries the skip marker.
/// Fence length is tracked so 4-backtick blocks (which wrap nested ``` examples
/// in the docs) are bounded correctly rather than closing at the first inner
/// fence.
fn extract_blocks(markdown: &str) -> Vec<Block> {
    let lines: Vec<&str> = markdown.lines().collect();
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let open_len = backtick_run(lines[i]);
        if open_len < 3 {
            i += 1;
            continue;
        }
        let trimmed = lines[i].trim_start();
        let indent = lines[i].len() - trimmed.len();
        let marker = i
            .checked_sub(1)
            .and_then(|prev| {
                MARKERS.iter().find(|(text, _)| lines[prev].trim() == *text)
            })
            .map_or(Marker::None, |(_, marker)| *marker);
        let mut content = String::new();
        let mut j = i + 1;
        while j < lines.len() && !is_closing_fence(lines[j], open_len) {
            content.push_str(dedent(lines[j], indent));
            content.push('\n');
            j += 1;
        }
        blocks.push(Block {
            lang: trimmed[open_len..].trim().to_string(),
            content,
            marker,
        });
        // Resume past the closing fence (or at end-of-input when unterminated).
        i = j + 1;
    }
    blocks
}

/// The top-level name of a TOML table header (`[lint.rules.commas]` -> `lint`,
/// `[[output.junit]]` -> `output`), or `None` for a non-header line.
fn toml_table_header_name(line: &str) -> Option<&str> {
    let header = line.strip_prefix('[')?;
    let name = header
        .trim_start_matches('[')
        .split(['.', ']'])
        .next()
        .expect("split always yields at least one segment");
    Some(name.trim())
}

/// Classify a `toml` block: the `pyproject.toml` form (a `[tool.ryl]` table), a
/// standalone config (a table or a pre-header key named in the schema), or not ryl
/// config. Detection scans lines rather than parsing, so a malformed config example is
/// still recognised and handed to the loader to report.
fn classify_toml(content: &str, toml_keys: &BTreeSet<String>) -> Kind {
    let mut kind = Kind::NotConfig;
    let mut top_level = true;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("[tool.ryl]") || trimmed.starts_with("[tool.ryl.") {
            return Kind::Pyproject;
        }
        let header = toml_table_header_name(trimmed);
        top_level &= header.is_none();
        let key = trimmed.split_once('=').map(|(key, _)| key.trim());
        if header
            .or(key.filter(|_| top_level))
            .is_some_and(|name| toml_keys.contains(name))
        {
            kind = Kind::Toml;
        }
    }
    kind
}

/// Whether a `yaml` block declares a top-level mapping key named in the YAML
/// config schema (so a rule-input example is not mistaken for config).
fn is_yaml_config(content: &str, yaml_keys: &BTreeSet<String>) -> bool {
    content.lines().any(|line| {
        line.starts_with(|c: char| !c.is_whitespace())
            && line
                .split_once(':')
                .is_some_and(|(key, _)| yaml_keys.contains(key.trim()))
    })
}

fn classify(
    block: &Block,
    toml_keys: &BTreeSet<String>,
    yaml_keys: &BTreeSet<String>,
) -> Kind {
    if block.marker == Marker::Skip {
        return Kind::NotConfig;
    }
    match block.lang.as_str() {
        "toml" => classify_toml(&block.content, toml_keys),
        "yaml" | "yml" if is_yaml_config(&block.content, yaml_keys) => Kind::Yaml,
        _ => Kind::NotConfig,
    }
}

/// Validate a classified block through ryl's finalized config path: write it to a
/// temp config file (named so the `pyproject.toml` form is recognised) and run the
/// `discover_config` `-c` path the CLI uses, which parses *and* finalizes (so
/// misspelled rule names are caught) without the "no rules enabled" gate (so
/// fragments pass). A config notice (a deprecated key) is a failure too, so the docs
/// teach only the current shape. `-c` bypasses project/env/user-global discovery, so no
/// `HOME` isolation is needed. A format marker then checks the formatter's verdict too.
fn validate(kind: Kind, marker: Marker, content: &str) -> Result<(), String> {
    let name = match (kind, marker) {
        (Kind::Toml, _) => "config.toml",
        (Kind::Pyproject, _) => "pyproject.toml",
        (Kind::Yaml, _) => "config.yaml",
        (Kind::NotConfig, Marker::FormatClean | Marker::FormatConflict) => {
            return Err("a format marker sits on a block that is not ryl config".into());
        }
        (Kind::NotConfig, _) => return Ok(()),
    };
    let dir = tempdir().expect("create temp dir for config validation");
    let cfg = dir.path().join(name);
    fs::write(&cfg, content).expect("write temp config file");
    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
    )?;
    if !ctx.notices.is_empty() {
        return Err(ctx.notices.join("; "));
    }
    let conflicts = format::conflicts(&ctx.config);
    match marker {
        Marker::FormatClean if !conflicts.is_empty() => Err(conflicts.join("; ")),
        Marker::FormatClean => lint_formatted_sample(&ctx.config, dir.path()),
        Marker::FormatConflict if conflicts.is_empty() => {
            Err("marked format-conflict, but `ryl format` warns about nothing".into())
        }
        _ => Ok(()),
    }
}

fn lint_formatted_sample(cfg: &YamlLintConfig, dir: &Path) -> Result<(), String> {
    let path = dir.join("sample.yaml");
    let formatted = format::format_str(FORMAT_SAMPLE, cfg, &path, &[]);
    let problems: Vec<String> = lint_str(&formatted, &path, cfg, dir)
        .into_iter()
        .map(|p| format!("{}:{} {}", p.line, p.column, p.message))
        .collect();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "ryl check rejects ryl format's output: {}",
            problems.join("; ")
        ))
    }
}

fn collect_markdown(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("docs directory should be readable") {
        let path = entry.expect("a readable directory entry").path();
        if path.is_dir() {
            collect_markdown(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "md") {
            out.push(path);
        }
    }
}

#[test]
fn docs_config_examples_are_valid() {
    let docs = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs");
    // docs/ is excluded from the packaged crate; skip there rather than fail.
    if !docs.is_dir() {
        return;
    }

    let toml_keys = schema_top_level_keys(ryl::config_schema::schema_value());
    let yaml_keys = schema_top_level_keys(ryl::config_schema::yaml_schema_value());

    let mut files = Vec::new();
    collect_markdown(&docs, &mut files);
    files.sort();

    let failures: Vec<String> = files
        .iter()
        .flat_map(|file| {
            let text = fs::read_to_string(file).expect("doc file should be readable");
            extract_blocks(&text)
                .into_iter()
                .enumerate()
                .filter_map(|(idx, block)| {
                    let kind = classify(&block, &toml_keys, &yaml_keys);
                    validate(kind, block.marker, &block.content)
                        .err()
                        .map(|err| {
                            format!(
                                "{}: block {} ({:?}): {}",
                                file.display(),
                                idx + 1,
                                kind,
                                err.replace('\n', " ")
                            )
                        })
                })
                .collect::<Vec<_>>()
        })
        .collect();

    assert!(
        failures.is_empty(),
        "docs contain config examples the loader rejects:\n{}",
        failures.join("\n")
    );
}

/// Classification covers each config form and both non-config cases, independent
/// of what the docs currently contain.
#[test]
fn classify_routes_each_block_kind() {
    let toml_keys = schema_top_level_keys(ryl::config_schema::schema_value());
    let yaml_keys = schema_top_level_keys(ryl::config_schema::yaml_schema_value());

    let block = |lang: &str, content: &str, marker: Marker| Block {
        lang: lang.to_string(),
        content: content.to_string(),
        marker,
    };
    let kind = |b: &Block| classify(b, &toml_keys, &yaml_keys);

    assert_eq!(
        kind(&block(
            "toml",
            "[lint.rules.commas]\nlevel = \"error\"\n",
            Marker::None
        )),
        Kind::Toml,
    );
    assert_eq!(
        kind(&block(
            "toml",
            "[tool.ryl.lint.rules.commas]\nlevel = \"error\"\n",
            Marker::None
        )),
        Kind::Pyproject,
    );
    assert_eq!(
        kind(&block(
            "toml",
            "[[lint.per-line-ignores]]\nregex = 'x'\n",
            Marker::None
        )),
        Kind::Toml,
        "an array-of-tables header is recognised by its top-level name",
    );
    assert_eq!(
        kind(&block("toml", "line-length = 100\n", Marker::None)),
        Kind::Toml,
        "a top-level key before any header is recognised",
    );
    assert_eq!(
        kind(&block("toml", "[package]\nname = \"demo\"\n", Marker::None)),
        Kind::NotConfig,
        "another tool's TOML is not ryl config",
    );
    assert_eq!(
        kind(&block(
            "toml",
            "[tool.ruff]\nline-length = 100\n",
            Marker::None
        )),
        Kind::NotConfig,
        "a schema key under another table is not ryl config",
    );
    assert_eq!(
        kind(&block("toml", "this = is = not = toml", Marker::None)),
        Kind::NotConfig,
        "a block with no ryl table header is not config",
    );
    assert_eq!(
        kind(&block(
            "toml",
            "[lint.rules.commas]\nlevel =\n",
            Marker::None
        )),
        Kind::Toml,
        "a malformed config example is recognised by its header, not skipped",
    );
    assert_eq!(
        kind(&block("yaml", "rules:\n  commas: enable\n", Marker::None)),
        Kind::Yaml,
    );
    assert_eq!(
        kind(&block(
            "yaml",
            "build:\n  steps:\n    - run: make\n",
            Marker::None
        )),
        Kind::NotConfig,
        "a rule-input example is not config",
    );
    assert_eq!(
        kind(&block("yml", "extends: default\n", Marker::None)),
        Kind::Yaml,
        "the .yml language tag is recognised",
    );
    assert_eq!(
        kind(&block("bash", "echo hi\n", Marker::None)),
        Kind::NotConfig,
        "a non-config language is skipped",
    );
    assert_eq!(
        kind(&block(
            "toml",
            "[lint.rules.commas]\nlevel = \"error\"\n",
            Marker::Skip
        )),
        Kind::NotConfig,
        "the skip marker overrides detection",
    );
}

/// The finalized loader accepts valid config (including rule-less fragments) and
/// rejects every class of typo a docs example might carry: bad values, misspelled
/// rule options (caught at parse), and misspelled rule names (caught at finalize),
/// in both the standalone and `pyproject.toml` forms.
#[test]
fn validate_reports_loader_verdict() {
    let accepted = [
        (Kind::Toml, "[lint.rules.commas]\nlevel = \"error\"\n"),
        (
            Kind::Pyproject,
            "[tool.ryl.lint.rules.commas]\nlevel = \"error\"\n",
        ),
        (Kind::Yaml, "rules:\n  commas: enable\n"),
        (Kind::NotConfig, "anything goes here"),
        // A fragment that enables no rules still validates (no "no rules" gate here).
        (Kind::Toml, "[lint]\nfixable = [\"ALL\"]\n"),
    ];
    for (kind, content) in accepted {
        assert!(
            validate(kind, Marker::None, content).is_ok(),
            "{kind:?} should be accepted: {content:?}",
        );
    }

    let rejected = [
        (Kind::Toml, "[lint.rules.commas]\nlevel = \"bogus\"\n"),
        (
            Kind::Toml,
            "[lint.rules.tariling-spaces]\nlevel = \"error\"\n",
        ),
        (Kind::Toml, "[lint.rules.commas]\nunknown-option = 0\n"),
        (
            Kind::Pyproject,
            "[tool.ryl.lint.rules.tariling-spaces]\nlevel = \"error\"\n",
        ),
        (Kind::Yaml, "rules:\n  not-a-real-rule: enable\n"),
        (Kind::Toml, "[rules.commas]\nlevel = \"error\"\n"),
    ];
    for (kind, content) in rejected {
        assert!(
            validate(kind, Marker::None, content).is_err(),
            "{kind:?} should be rejected: {content:?}",
        );
    }
}

/// A malformed ryl config example must fail the guard rather than slip through as
/// "not config": its header is recognised, so it reaches the loader, which reports
/// the syntax error.
#[test]
fn malformed_toml_config_example_is_caught() {
    let toml_keys = schema_top_level_keys(ryl::config_schema::schema_value());
    let yaml_keys = schema_top_level_keys(ryl::config_schema::yaml_schema_value());
    let block = Block {
        lang: "toml".to_string(),
        content: "[lint.rules.commas]\nlevel =\n".to_string(),
        marker: Marker::None,
    };
    let kind = classify(&block, &toml_keys, &yaml_keys);
    assert_eq!(kind, Kind::Toml, "the header marks it as ryl config");
    assert!(
        validate(kind, block.marker, &block.content).is_err(),
        "the loader must reject the malformed example",
    );
}

/// Exercises the fenced-block extractor: a leading fence (no preceding line), the
/// skip marker, indentation dedent, and a 4-backtick block that wraps a nested
/// ``` example without closing early.
#[test]
fn extract_blocks_handles_skip_markers_indentation_and_nested_fences() {
    let markdown = "\
```toml
top = 1
```

<!-- ryl-config-check: skip -->
```toml
skipped = true
```

Indented inside a list:

    ```toml
    [files]
    yaml = [\"*.yaml\"]
    ```

A 4-backtick wrapper around a nested fence:

````markdown
```yaml
nested: true
```
````
";
    let blocks = extract_blocks(markdown);
    assert_eq!(blocks.len(), 4, "four fenced blocks should be found");

    assert_eq!(
        blocks[0].marker,
        Marker::None,
        "first block has no preceding marker line"
    );
    assert_eq!(blocks[0].content, "top = 1\n");

    assert_eq!(
        blocks[1].marker,
        Marker::Skip,
        "the skip marker on the prior line is recorded"
    );

    assert_eq!(
        blocks[2].content, "[files]\nyaml = [\"*.yaml\"]\n",
        "an indented fence is dedented to its fence column",
    );

    assert_eq!(
        blocks[3].lang, "markdown",
        "the 4-backtick fence is not closed by the inner ``` line",
    );
    assert_eq!(
        blocks[3].content, "```yaml\nnested: true\n```\n",
        "the nested fence is captured as content of the outer block",
    );
}

/// Each format marker rejects the config that contradicts it, and a format marker on a
/// block that is not ryl config fails rather than passing unchecked.
#[test]
fn format_markers_check_the_formatter_verdict() {
    let conflicting = "[lint.rules]\nquoted-strings = \"enable\"\n";
    let compatible =
        "[lint.rules.quoted-strings]\nquote-type = \"any\"\nrequired = false\n";
    let rejects_output = "[format]\ndocument-end = \"preserve\"\n\n\
        [lint.rules.document-end]\npresent = true\n";
    let cases = [
        (Kind::Toml, Marker::FormatConflict, conflicting, true),
        (Kind::Toml, Marker::FormatClean, conflicting, false),
        (Kind::Toml, Marker::FormatClean, compatible, true),
        (Kind::Toml, Marker::FormatConflict, compatible, false),
        (Kind::Toml, Marker::FormatClean, rejects_output, false),
        (Kind::NotConfig, Marker::FormatClean, "echo hi\n", false),
    ];
    for (kind, marker, content, accepted) in cases {
        assert_eq!(
            validate(kind, marker, content).is_ok(),
            accepted,
            "{marker:?} on {content:?}",
        );
    }
}

/// The formatter page and the `[format]` keys of the TOML config schema, or `None` where
/// docs/ is absent, as in the packaged crate.
fn formatter_page_and_keys() -> Option<(String, Vec<String>)> {
    let page = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/formatter.md");
    let text = fs::read_to_string(page).ok()?;
    let schema = ryl::config_schema::schema_value();
    let table = schema["properties"]["format"]["anyOf"]
        .as_array()
        .expect("`format` is an optional table")
        .iter()
        .find_map(|branch| branch["$ref"].as_str()?.strip_prefix("#/$defs/"))
        .expect("`format` refers to its table's definition");
    let keys = schema["$defs"][table]["properties"]
        .as_object()
        .expect("the `[format]` table has properties")
        .keys()
        .cloned()
        .collect();
    Some((text, keys))
}

/// Every `[format]` key in the TOML config schema has a row in the formatter page's key
/// table, so a new key cannot ship undocumented.
#[test]
fn every_format_key_has_a_formatter_page_row() {
    let Some((text, keys)) = formatter_page_and_keys() else {
        return;
    };
    let missing: Vec<&String> = keys
        .iter()
        .filter(|key| {
            !text
                .lines()
                .any(|line| line.starts_with(&format!("| `{key}` |")))
        })
        .collect();
    assert!(
        missing.is_empty(),
        "docs/formatter.md has no key-table row for: {missing:?}"
    );
}

/// Every default and value the formatter page's key table lists for a `[format]` key
/// loads, since prose values escape the fenced-example check.
#[test]
fn formatter_page_key_values_load() {
    let Some((text, keys)) = formatter_page_and_keys() else {
        return;
    };
    let rejected: Vec<String> = keys
        .iter()
        .flat_map(|key| {
            let prefix = format!("| `{key}` |");
            let row = text
                .lines()
                .find(|line| line.starts_with(&prefix))
                .unwrap_or_default();
            let cells: Vec<&str> = row.split('|').skip(2).take(2).collect();
            cells
                .join(" ")
                .split('`')
                .skip(1)
                .step_by(2)
                .map(|value| format!("[format]\n{key} = {value}\n"))
                .collect::<Vec<_>>()
        })
        .filter(|config| validate(Kind::Toml, Marker::None, config).is_err())
        .collect();
    assert!(
        rejected.is_empty(),
        "docs/formatter.md lists values the loader rejects: {rejected:?}"
    );
}
