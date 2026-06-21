//! Every ryl-config example in the docs' Markdown sources must be config the loader
//! actually accepts. Validation goes through ryl's *finalized* config path
//! (`discover_config`, the same one the CLI uses), not the JSON schema (which is
//! generated from the loader) and not a parse-only load: finalizing is what rejects
//! misspelled rule names, while parsing rejects misspelled rule options and bad
//! values, so both classes of typo in a docs example are caught. The "no rules
//! enabled" gate lives above `discover_config`, so `[fix]`/`[files]`-only fragments
//! still validate. Only the `.md` sources are scanned: `docs/llms*.txt` are generated
//! from them and held in lockstep by a separate drift guard, so the sources cover them.
//!
//! Not every fenced block is ryl config — docs also carry rule-input YAML, other
//! tools' TOML, and so on — and a block need not even parse, so each is *classified
//! from its content* by structural markers keyed to ryl's config schema:
//!   - a `toml` block is ryl config when it declares a `[tool.ryl]` table (the
//!     `pyproject.toml` form) or a table whose top-level name is in the TOML config
//!     schema (`[rules]`, `[[per-line-ignores]]`, `[output.gitlab]`, ...); other TOML
//!     (a `prek.toml`, a `Cargo.toml`) is skipped;
//!   - a `yaml` block is ryl config when a top-level mapping key is in the YAML
//!     config schema (`rules:`, `extends:`, ...); rule-input examples are skipped.
//!
//! Detection is deliberately structural rather than a parse, so a *malformed* config
//! example (which would not parse) is still recognised by its headers and routed to
//! the loader, which reports the error — rather than being silently skipped. The
//! unavoidable limit of any such heuristic: a broken block with no recognisable ryl
//! header is indistinguishable from another tool's TOML and is treated as non-config.
//!
//! A `<!-- ryl-config-check: skip -->` comment on the line before a fence overrides
//! detection for an intentional counter-example (e.g. the YAML-1.1 config in
//! `yaml-version.md` whose prose says it "will fail to parse in ryl").

use ryl::config::{Overrides, discover_config};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

const SKIP_MARKER: &str = "<!-- ryl-config-check: skip -->";

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
    skip: bool,
}

/// Top-level property names of a config schema produced by
/// [`ryl::config_schema`] — the source of truth for which tables/keys mark a
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
        let skip = i
            .checked_sub(1)
            .is_some_and(|prev| lines[prev].trim() == SKIP_MARKER);
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
            skip,
        });
        // Resume past the closing fence (or at end-of-input when unterminated).
        i = j + 1;
    }
    blocks
}

/// The top-level name of a TOML table header (`[rules.commas]` -> `rules`,
/// `[[per-line-ignores]]` -> `per-line-ignores`), or `None` for a non-header line.
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
/// standalone config (a table named in the schema), or not ryl config. Detection
/// scans table headers rather than parsing, so a malformed config example is still
/// recognised and handed to the loader to report.
fn classify_toml(content: &str, toml_keys: &BTreeSet<String>) -> Kind {
    let mut kind = Kind::NotConfig;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("[tool.ryl]") || trimmed.starts_with("[tool.ryl.") {
            return Kind::Pyproject;
        }
        if toml_table_header_name(trimmed).is_some_and(|name| toml_keys.contains(name))
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
    if block.skip {
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
/// fragments pass). `-c` bypasses project/env/user-global discovery, so no `HOME`
/// isolation is needed.
fn validate(kind: Kind, content: &str) -> Result<(), String> {
    let name = match kind {
        Kind::Toml => "config.toml",
        Kind::Pyproject => "pyproject.toml",
        Kind::Yaml => "config.yaml",
        Kind::NotConfig => return Ok(()),
    };
    let dir = tempdir().expect("create temp dir for config validation");
    let cfg = dir.path().join(name);
    fs::write(&cfg, content).expect("write temp config file");
    discover_config(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
    )
    .map(drop)
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
                    validate(kind, &block.content).err().map(|err| {
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

    let block = |lang: &str, content: &str, skip: bool| Block {
        lang: lang.to_string(),
        content: content.to_string(),
        skip,
    };
    let kind = |b: &Block| classify(b, &toml_keys, &yaml_keys);

    assert_eq!(
        kind(&block("toml", "[rules.commas]\nlevel = \"error\"\n", false)),
        Kind::Toml,
    );
    assert_eq!(
        kind(&block(
            "toml",
            "[tool.ryl.rules.commas]\nlevel = \"error\"\n",
            false
        )),
        Kind::Pyproject,
    );
    assert_eq!(
        kind(&block("toml", "[[per-line-ignores]]\nregex = 'x'\n", false)),
        Kind::Toml,
        "an array-of-tables header is recognised by its top-level name",
    );
    assert_eq!(
        kind(&block("toml", "[package]\nname = \"demo\"\n", false)),
        Kind::NotConfig,
        "another tool's TOML is not ryl config",
    );
    assert_eq!(
        kind(&block("toml", "this = is = not = toml", false)),
        Kind::NotConfig,
        "a block with no ryl table header is not config",
    );
    assert_eq!(
        kind(&block("toml", "[rules.commas]\nlevel =\n", false)),
        Kind::Toml,
        "a malformed config example is recognised by its header, not skipped",
    );
    assert_eq!(
        kind(&block("yaml", "rules:\n  commas: enable\n", false)),
        Kind::Yaml,
    );
    assert_eq!(
        kind(&block("yaml", "build:\n  steps:\n    - run: make\n", false)),
        Kind::NotConfig,
        "a rule-input example is not config",
    );
    assert_eq!(
        kind(&block("yml", "extends: default\n", false)),
        Kind::Yaml,
        "the .yml language tag is recognised",
    );
    assert_eq!(
        kind(&block("bash", "echo hi\n", false)),
        Kind::NotConfig,
        "a non-config language is skipped",
    );
    assert_eq!(
        kind(&block("toml", "[rules.commas]\nlevel = \"error\"\n", true)),
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
        (Kind::Toml, "[rules.commas]\nlevel = \"error\"\n"),
        (
            Kind::Pyproject,
            "[tool.ryl.rules.commas]\nlevel = \"error\"\n",
        ),
        (Kind::Yaml, "rules:\n  commas: enable\n"),
        (Kind::NotConfig, "anything goes here"),
        // A fragment that enables no rules still validates (no "no rules" gate here).
        (Kind::Toml, "[fix]\nfixable = [\"ALL\"]\n"),
    ];
    for (kind, content) in accepted {
        assert!(
            validate(kind, content).is_ok(),
            "{kind:?} should be accepted: {content:?}",
        );
    }

    let rejected = [
        (Kind::Toml, "[rules.commas]\nlevel = \"bogus\"\n"),
        (Kind::Toml, "[rules.tariling-spaces]\nlevel = \"error\"\n"),
        (Kind::Toml, "[rules.commas]\nunknown-option = 0\n"),
        (
            Kind::Pyproject,
            "[tool.ryl.rules.tariling-spaces]\nlevel = \"error\"\n",
        ),
        (Kind::Yaml, "rules:\n  not-a-real-rule: enable\n"),
    ];
    for (kind, content) in rejected {
        assert!(
            validate(kind, content).is_err(),
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
        content: "[rules.commas]\nlevel =\n".to_string(),
        skip: false,
    };
    let kind = classify(&block, &toml_keys, &yaml_keys);
    assert_eq!(kind, Kind::Toml, "the header marks it as ryl config");
    assert!(
        validate(kind, &block.content).is_err(),
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

    assert!(!blocks[0].skip, "first block has no preceding marker line");
    assert_eq!(blocks[0].content, "top = 1\n");

    assert!(
        blocks[1].skip,
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
