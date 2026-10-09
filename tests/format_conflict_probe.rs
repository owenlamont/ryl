//! Every option of a formatting rule either has a config here that `format::conflicts`
//! must report, or a reason no value of it can reject what `ryl format` writes.

use std::collections::BTreeSet;

use ryl::config::YamlLintConfig;
use ryl::config_schema::schema_value;
use ryl::format::{FORMAT_RULE_IDS, conflicts};
use serde_json::Value;

const QUOTED: &str = "quote-type = 'single'\nrequired = 'only-when-needed'\n\
    allow-double-quotes-for-escaping = true\nallow-quoted-quotes = true\n";

/// `[format]` prelude, rule, agreeing options, then the option line that must conflict.
const CONFLICTING: &[(&str, &str, &str, &str)] = &[
    (
        "[format]\nmapping-style = 'flow'\n",
        "braces",
        "",
        "forbid = 'non-empty'",
    ),
    ("", "braces", "", "min-spaces-inside = 1"),
    ("", "braces", "", "min-spaces-inside-empty = 1"),
    (
        "[format]\nbrace-spacing = true\n",
        "braces",
        "max-spaces-inside = 1\nmin-spaces-inside-empty = 0\n",
        "max-spaces-inside = 0",
    ),
    (
        "[format]\nmapping-style = 'flow'\n",
        "braces",
        "",
        "forbid = true",
    ),
    (
        "[format]\nsequence-style = 'flow'\n",
        "brackets",
        "",
        "forbid = 'non-empty'",
    ),
    ("", "brackets", "", "min-spaces-inside = 1"),
    ("", "brackets", "", "min-spaces-inside-empty = 1"),
    (
        "[format]\nsequence-style = 'flow'\n",
        "brackets",
        "",
        "forbid = true",
    ),
    ("", "colons", "", "max-spaces-after = 0"),
    ("", "commas", "", "min-spaces-after = 2"),
    ("", "commas", "", "max-spaces-after = 0"),
    ("", "comments", "", "min-spaces-from-content = 3"),
    (
        "",
        "comments",
        "min-spaces-from-content = 1\n",
        "max-spaces-from-content = 1",
    ),
    ("", "comments", "", "ignore-shebangs = false"),
    (
        "",
        "comments",
        "ignore-shebangs = false\nrequire-starting-space = false\n",
        "require-starting-space = true",
    ),
    (
        "[format]\ncomment-spacing = 3\n",
        "comments",
        "min-spaces-from-content = 1\n",
        "max-spaces-from-content = 2",
    ),
    (
        "[format]\ndocument-start = 'add'\n",
        "document-start",
        "",
        "present = false",
    ),
    (
        "[format]\ndocument-end = 'add'\n",
        "document-end",
        "",
        "present = false",
    ),
    ("", "empty-lines", "", "max = 1"),
    (
        "[format]\nmax-blank-lines = 4\n",
        "empty-lines",
        "max = 4\n",
        "max = 3",
    ),
    ("", "hyphens", "", "max-spaces-after = 0"),
    ("", "hyphens", "", "dash-on-own-line = true"),
    (
        "[format]\ndash-on-own-line = true\n",
        "hyphens",
        "dash-on-own-line = true\n",
        "max-spaces-after = 0",
    ),
    ("", "indentation", "", "spaces = 4"),
    ("", "indentation", "", "indent-sequences = false"),
    (
        "[format]\nindent-sequences = false\n",
        "indentation",
        "indent-sequences = false\n",
        "indent-sequences = true",
    ),
    (
        "indent-width = 4\n",
        "indentation",
        "spaces = 4\n",
        "check-multi-line-strings = true",
    ),
    ("", "new-lines", "", "type = 'dos'"),
    (
        "[format]\nline-ending = 'cr-lf'\n",
        "new-lines",
        "type = 'dos'\n",
        "type = 'unix'",
    ),
    ("", "quoted-strings", QUOTED, "quote-type = 'double'"),
    ("", "quoted-strings", QUOTED, "required = true"),
    (
        "",
        "quoted-strings",
        QUOTED,
        "allow-double-quotes-for-escaping = false",
    ),
    ("", "quoted-strings", QUOTED, "allow-quoted-quotes = false"),
    (
        "",
        "quoted-strings",
        QUOTED,
        "extra-required = ['^secret$']",
    ),
    (
        "[format]\nquote-style = 'double'\n",
        "quoted-strings",
        "quote-type = 'double'\nrequired = 'only-when-needed'\nallow-quoted-quotes = true\n",
        "quote-type = 'single'",
    ),
];

/// Options no value of which rejects the formatter's output: rule, agreeing options, the
/// option at its strictest, and the reason.
const UNCONFLICTABLE: &[(&str, &str, &str, &str)] = &[
    (
        "braces",
        "",
        "max-spaces-inside-empty = 0",
        "empty braces are written `{}`",
    ),
    (
        "brackets",
        "",
        "max-spaces-inside = 0",
        "brackets are never padded",
    ),
    (
        "brackets",
        "",
        "max-spaces-inside-empty = 0",
        "empty brackets are written `[]`",
    ),
    (
        "colons",
        "",
        "max-spaces-before = 0",
        "no space is written before `:`",
    ),
    (
        "commas",
        "",
        "max-spaces-before = 0",
        "no space is written before `,`",
    ),
    (
        "comments-indentation",
        "",
        "allow-any-open-indent = false",
        "only admits more placements",
    ),
    (
        "empty-lines",
        "",
        "max-start = 0",
        "leading blank lines are dropped",
    ),
    (
        "empty-lines",
        "",
        "max-end = 0",
        "trailing blank lines are dropped",
    ),
    (
        "line-length",
        "",
        "max = 5",
        "line-length is never reported",
    ),
    (
        "line-length",
        "max = 5\n",
        "allow-non-breakable-words = false",
        "line-length is never reported",
    ),
    (
        "line-length",
        "max = 5\n",
        "allow-non-breakable-inline-mappings = false",
        "line-length is never reported",
    ),
    (
        "quoted-strings",
        QUOTED,
        "check-keys = true",
        "keys are quoted as values are, so a value conflicts first",
    ),
    (
        "quoted-strings",
        QUOTED,
        "extra-allowed = []",
        "only admits more quoting",
    ),
];

fn warned(config: &str) -> Vec<String> {
    let cfg = YamlLintConfig::from_toml_str(config).expect(config);
    conflicts(&cfg)
        .iter()
        .map(|line| line.split(' ').nth(1).unwrap().to_string())
        .collect()
}

fn options_of(schema: &Value, rule: &str) -> BTreeSet<String> {
    let defs = &schema["$defs"];
    let reference = |entry: &Value, prefix: &str| -> String {
        entry["anyOf"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|variant| variant["$ref"].as_str())
            .find_map(|r| r.strip_prefix("#/$defs/").filter(|d| d.starts_with(prefix)))
            .unwrap()
            .to_string()
    };
    let entry = reference(&defs["RulesTable"]["properties"][rule], "RuleEntryFor");
    let options = reference(&defs[entry.as_str()], "RuleOptionsFor");
    defs[options.as_str()]["properties"]
        .as_object()
        .unwrap()
        .keys()
        .filter(|key| !matches!(key.as_str(), "level" | "ignore" | "ignore-from-file"))
        .cloned()
        .collect()
}

#[test]
fn every_formatting_rule_option_is_conflicting_or_unconflictable() {
    let schema = schema_value();
    let expected: BTreeSet<(String, String)> = FORMAT_RULE_IDS
        .into_iter()
        .flat_map(|rule| {
            options_of(&schema, rule)
                .into_iter()
                .map(move |option| (rule.to_string(), option))
        })
        .collect();
    let listed: BTreeSet<(String, String)> = CONFLICTING
        .iter()
        .map(|(_, rule, _, line)| (*rule, line.split(" =").next().unwrap()))
        .chain(
            UNCONFLICTABLE
                .iter()
                .map(|(rule, _, line, _)| (*rule, line.split(" =").next().unwrap())),
        )
        .map(|(rule, option)| (rule.to_string(), option.to_string()))
        .collect();
    assert_eq!(listed, expected);
}

#[test]
fn each_conflicting_option_is_reported_and_its_baseline_is_not() {
    let mut missed = Vec::new();
    for (prelude, rule, agreeing, line) in CONFLICTING {
        let key = line.split(" =").next().unwrap();
        let kept: String = agreeing
            .lines()
            .filter(|l| l.split(" =").next() != Some(key))
            .map(|l| format!("{l}\n"))
            .collect();
        let table = format!("{prelude}[lint.rules.{rule}]\nlevel = 'error'\n");
        let baseline = format!("{table}{agreeing}");
        let conflicting = format!("{table}{kept}{line}\n");
        let outcome = (warned(&baseline), warned(&conflicting));
        if outcome != (Vec::new(), vec![(*rule).to_string()]) {
            missed.push(format!("{conflicting}=> {outcome:?}"));
        }
    }
    assert!(missed.is_empty(), "{}", missed.join("\n"));
}

#[test]
fn each_unconflictable_option_is_silent_at_its_strictest() {
    for (rule, agreeing, line, reason) in UNCONFLICTABLE {
        let config =
            format!("[lint.rules.{rule}]\nlevel = 'error'\n{agreeing}{line}\n");
        assert_eq!(warned(&config), Vec::<String>::new(), "{reason}: {config}");
    }
}

#[test]
fn options_without_a_format_key_name_the_built_in_style() {
    for (config, style) in [
        (
            "[lint.rules.comments]\nignore-shebangs = false\n",
            "built-in comments style",
        ),
        (
            "indent-width = 4\n[lint.rules.indentation]\nspaces = 4\n\
             check-multi-line-strings = true\n",
            "built-in indentation style",
        ),
    ] {
        let cfg = YamlLintConfig::from_toml_str(config).unwrap();
        let warnings = conflicts(&cfg);
        assert_eq!(warnings.len(), 1, "{config}: {warnings:#?}");
        assert!(warnings[0].contains(style), "{}", warnings[0]);
    }
}

#[test]
fn a_preserved_starting_space_waives_the_shebang_but_not_the_spacing() {
    let preserve = "[format]\ncomment-starting-space = 'preserve'\n\
                    [lint.rules.comments]\nignore-shebangs = false\n\
                    require-starting-space = true\n";
    assert_eq!(warned(preserve), Vec::<String>::new());
    let cfg = YamlLintConfig::from_toml_str(&format!(
        "{preserve}min-spaces-from-content = 3\n"
    ))
    .unwrap();
    let warnings = conflicts(&cfg);
    assert_eq!(warnings.len(), 1, "{warnings:#?}");
    assert!(
        warnings[0].contains("`[format] comment-spacing = 2`"),
        "{}",
        warnings[0]
    );
}

#[test]
fn a_preserved_collection_style_waives_forbid_but_not_the_spacing() {
    for (style, rule, spacing) in [
        ("mapping-style", "braces", "brace-spacing"),
        ("sequence-style", "brackets", "built-in brackets style"),
    ] {
        let forbid = format!(
            "[format]\n{style} = 'preserve'\n[lint.rules.{rule}]\nforbid = 'non-empty'\n"
        );
        assert_eq!(warned(&forbid), Vec::<String>::new(), "{forbid}");
        let config = format!("{forbid}min-spaces-inside = 1\n");
        let warnings = conflicts(&YamlLintConfig::from_toml_str(&config).unwrap());
        assert_eq!(warnings.len(), 1, "{config}: {warnings:#?}");
        assert!(warnings[0].contains(spacing), "{}", warnings[0]);
    }
}
