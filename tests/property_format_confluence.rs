use std::collections::BTreeSet;

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use ryl::config::YamlLintConfig;
use ryl::config_schema::{CollectionStyleTarget, ExpandTarget};

use crate::property::{config::synthetic_path, reference, representation};

const SCRAMBLERS: [&str; 16] = [
    "braces",
    "brackets",
    "colons",
    "commas",
    "comments",
    "comments-indentation",
    "document-end",
    "document-start",
    "empty-lines",
    "hyphens",
    "indentation",
    "line-length",
    "new-line-at-end-of-file",
    "new-lines",
    "quoted-strings",
    "trailing-spaces",
];

#[derive(Debug)]
struct Document {
    words: Vec<String>,
    amount: usize,
}

impl Document {
    fn render(&self, scramblers: &[&str], cfg: &YamlLintConfig) -> String {
        let on = |rule| scramblers.contains(&rule);
        let spaces = " ".repeat(self.amount);
        let indent = if on("indentation") {
            self.amount + 2
        } else {
            2
        };
        let pad = " ".repeat(indent);
        let bracket = if on("brackets") { &spaces } else { "" };
        let brace = if on("braces") { &spaces } else { "" };
        let colon = if on("colons") { "  :   " } else { ": " };
        let comma = if on("commas") { " ,   " } else { ", " };
        let dash = if on("hyphens") { "-    " } else { "- " };
        let mapping_pad = " ".repeat(indent * 2 + dash.len());
        let quote = if on("quoted-strings") { '"' } else { '\'' };
        let comment = if on("comments") {
            "     #note"
        } else {
            "  # note"
        };
        let comment_pad = if on("comments-indentation") {
            &spaces
        } else {
            ""
        };
        let tail = if on("trailing-spaces") { &spaces } else { "" };
        let blanks = if on("empty-lines") { "\n\n\n" } else { "" };
        let targets = cfg.format().targets();
        let prose = self.words.join(" ");
        let prose = if on("line-length")
            && (!targets.keep_quotes
                || matches!(
                    targets.prose_wrap,
                    Some(
                        ryl::config_schema::ProseWrap::Always
                            | ryl::config_schema::ProseWrap::Never
                    )
                )) {
            self.words.join(&format!("\n{pad}{pad}"))
        } else {
            prose
        };
        let word = &self.words[0];
        let folded = if matches!(
            targets.prose_wrap,
            Some(
                ryl::config_schema::ProseWrap::Always
                    | ryl::config_schema::ProseWrap::Never
            )
        ) {
            format!(
                "{pad}prose: >-\n{pad}{pad}{prose}\n{pad}# ryl disable rule:line-length\n{pad}held: |\n{pad}{pad}fixed\n{pad}# ryl enable rule:line-length\n"
            )
        } else {
            String::new()
        };
        let mut text = String::new();
        if !on("document-start") {
            text.push_str("---\n");
        }
        text.push_str(&format!(
            "# lead\n{blanks}root{colon}&root\n\
             {pad}{comment_pad}# child\n\
             {pad}flow{colon}{{{brace}left{colon}[{bracket}one{comma}two{bracket}]{comma}right{colon}{quote}{word}{quote}{brace}}}{comment}{tail}\n\
             {pad}items:\n{pad}{pad}{dash}{quote}{word}{quote}\n{pad}{pad}{dash}three\n\
             {pad}records:\n{pad}{pad}{dash}name{colon}{quote}{word}{quote}\n{mapping_pad}other: ok\n\
             {pad}text{colon}\"{prose}\"\n{folded}\
             {pad}literal: |-\n{pad}{pad}{word}\n\
             copy{colon}*root\n{blanks}"
        ));
        // Under `always`, a `flow` target leaves a block collection block.
        let keeps_block = |style, expand| {
            style == CollectionStyleTarget::Flow && expand == ExpandTarget::Always
        };
        if on("brackets")
            && targets.sequence_style != CollectionStyleTarget::Preserve
            && !keeps_block(targets.sequence_style, targets.sequence_expand)
        {
            text.push_str(&format!("{quote}q -  s{quote}{colon}\n{pad}{dash}z\n"));
        } else if on("brackets") && targets.sequence_expand != ExpandTarget::Preserve {
            text.push_str(&format!(
                "{quote}q -  s{quote}{colon}[\n{pad}z{comma}\n{pad}]\n"
            ));
        } else {
            text.push_str(&format!(
                "{quote}q -  s{quote}{colon}[{bracket}z{bracket}]\n"
            ));
        }
        if on("braces") && targets.mapping_style == CollectionStyleTarget::Block {
            text.push_str(&format!("{quote}q -  m{quote}{colon}\n{pad}a{colon}z\n"));
        } else if on("braces") && targets.mapping_expand != ExpandTarget::Preserve {
            text.push_str(&format!(
                "{quote}q -  m{quote}{colon}{{\n{pad}a{colon}z\n{pad}}}\n"
            ));
        } else {
            text.push_str(&format!(
                "{quote}q -  m{quote}{colon}{{{brace}a{colon}z{brace}}}\n"
            ));
        }
        if !on("document-end") {
            text.push_str("...\n");
        }
        if on("new-line-at-end-of-file") {
            text.pop();
        }
        if on("new-lines") {
            text = text.replace('\n', "\r\n");
        }
        text
    }
}

fn arb_document() -> impl Strategy<Value = Document> {
    (
        prop::collection::vec(
            prop::sample::select(vec!["alpha", "café", "世界", "delta", "🦀", "echo"])
                .prop_map(str::to_owned),
            3..=15,
        ),
        1usize..=5,
    )
        .prop_map(|(words, amount)| Document { words, amount })
}

fn arb_config() -> impl Strategy<Value = String> {
    (
        1u8..=4,
        12u16..=80,
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        1u8..=4,
        prop::sample::select(vec!["preserve", "block", "flow"]),
        prop::sample::select(vec!["preserve", "block", "flow"]),
        (
            prop::sample::select(vec!["preserve", "auto", "always", "never"]),
            prop::sample::select(vec!["preserve", "auto", "always", "never"]),
            any::<bool>(),
            prop::sample::select(vec!["fold-long-lines = true", "prose-wrap = 'preserve'", "prose-wrap = 'always'", "prose-wrap = 'never'"]),
        ),
    )
        .prop_map(|(indent, width, double, crlf, brace, sequences, dash, gap, sequence, mapping, (sequence_expand, mapping_expand, keep_quotes, prose))| {
            format!(
                "indent-width = {indent}\nline-length = {width}\n[format]\n\
                 quote-style = '{}'\nkeep-quotes = {keep_quotes}\nline-ending = '{}'\nbrace-spacing = {brace}\n\
                 indent-sequences = {sequences}\ndash-on-own-line = {dash}\n\
                 comment-spacing = {gap}\ncomment-starting-space = 'add'\n\
                 document-start = 'add'\ndocument-end = 'add'\nmax-blank-lines = 0\n\
                 {prose}\nsequence-style = '{sequence}'\nmapping-style = '{mapping}'\n\
                 sequence-expand = '{sequence_expand}'\nmapping-expand = '{mapping_expand}'\n",
                if double { "double" } else { "single" },
                if crlf { "cr-lf" } else { "lf" },
            )
        })
}

fn check_preserved(input: &str, output: &str) -> Result<(), String> {
    reference::check_strict_events(input, output)?;
    reference::check_values(input, output)?;
    representation::check_annotations(input, output)
}

fn check_confluence(
    input: &str,
    scrambled: &str,
    format: &dyn Fn(&str) -> String,
) -> Result<(), String> {
    check_preserved(input, scrambled)?;
    let expected = format(input);
    let actual = format(scrambled);
    check_preserved(input, &expected)?;
    check_preserved(scrambled, &actual)?;
    if actual != expected {
        return Err(format!(
            "confluence: input {input:?}; scrambled {scrambled:?}; expected {expected:?}; actual {actual:?}"
        ));
    }
    Ok(())
}

fn format(input: &str, cfg: &YamlLintConfig) -> String {
    ryl::format::format_str(input, cfg, synthetic_path(), &[])
}

#[test]
fn retained_quote_styles_converge_with_identical_physical_breaks() {
    for style in ["single", "double"] {
        let cfg = YamlLintConfig::from_toml_str(&format!(
            "[format]\nkeep-quotes = true\nquote-style = '{style}'\n",
        ))
        .unwrap();
        check_confluence(
            "text: 'hello\n  world'\n",
            "text: \"hello\n  world\"\n",
            &|text| format(text, &cfg),
        )
        .unwrap();
        assert_ne!(
            format("text: hello\n", &cfg),
            format("text: 'hello'\n", &cfg)
        );
    }
}

#[test]
fn every_formatter_rule_has_a_scrambler() {
    assert_eq!(
        BTreeSet::from(SCRAMBLERS),
        BTreeSet::from(ryl::format::FORMAT_RULE_IDS)
    );
    assert_eq!(BTreeSet::from(SCRAMBLERS).len(), SCRAMBLERS.len());
}

#[test]
fn every_scrambler_changes_presentation_and_converges() {
    let document = Document {
        words: vec!["café".into(), "alpha".into(), "beta".into(), "delta".into()],
        amount: 3,
    };
    let cfg = YamlLintConfig::from_toml_str(
        "line-length = 16\n[format]\nfold-long-lines = true\n\
         document-start = 'add'\ndocument-end = 'add'\nmax-blank-lines = 0\n",
    )
    .unwrap();
    let input = document.render(&[], &cfg);
    for rules in SCRAMBLERS
        .map(|rule| vec![rule])
        .into_iter()
        .chain([SCRAMBLERS.to_vec()])
    {
        let scrambled = document.render(&rules, &cfg);
        assert_ne!(input, scrambled, "{rules:?}");
        assert_ne!(format(&scrambled, &cfg), scrambled, "{rules:?}");
        check_confluence(&input, &scrambled, &|text| format(text, &cfg))
            .unwrap_or_else(|error| panic!("{rules:?}: {error}"));
    }
}

#[test]
fn spread_flow_collections_converge_under_each_expand_target() {
    let document = Document {
        words: vec!["alpha".into(), "beta".into(), "delta".into()],
        amount: 2,
    };
    // At width 12 the `q -  s` and `q -  m` values overflow their key line but fit alone
    // on the next one.
    for (expand, width) in [("auto", 80), ("auto", 12), ("always", 80), ("never", 80)] {
        let cfg = YamlLintConfig::from_toml_str(&format!(
            "line-length = {width}\n[format]\nsequence-expand = '{expand}'\n\
             mapping-expand = '{expand}'\n"
        ))
        .unwrap();
        let input = document.render(&[], &cfg);
        for rules in [vec!["brackets"], vec!["braces"]] {
            let scrambled = document.render(&rules, &cfg);
            assert_ne!(input, scrambled, "{expand} {rules:?}");
            check_confluence(&input, &scrambled, &|text| format(text, &cfg))
                .unwrap_or_else(|error| panic!("{expand} {rules:?}: {error}"));
        }
    }
}

#[test]
fn confluence_oracle_rejects_invalid_sources_and_broken_passes() {
    assert!(check_confluence("a: [", "a: [", &str::to_owned).is_err());
    assert!(check_confluence("a: 1\n", "a: [", &str::to_owned).is_err());
    assert!(check_confluence("a: 1\n", "a: 2\n", &str::to_owned).is_err());
    assert!(check_confluence("a: 1\n", "a:  1\n", &str::to_owned).is_err());
    assert!(check_confluence("a: 1\n", "a:  1\n", &|_| "a: 2\n".into()).is_err());
    assert!(
        check_confluence("a: 1 # note\n", "a: 1 # lost\n", &str::to_owned).is_err()
    );
    assert!(check_confluence("a: 1\n", "a:  1\n", &|_| "a: [".into()).is_err());
}

#[test]
fn scalar_indicator_text_does_not_change_collection_targets() {
    for key in ["a -  key", "a ?  key", "a :  key"] {
        for (setting, flow, block) in [
            ("sequence-style", "[z]", "  - z\n"),
            ("mapping-style", "{a: z}", "  a: z\n"),
        ] {
            let input = format!("\"{key}\": {flow}\n");
            let scrambled = format!("\"{key}\":\n{block}");
            for style in ["flow", "block"] {
                let cfg = YamlLintConfig::from_toml_str(&format!(
                    "[format]\n{setting} = '{style}'\n"
                ))
                .unwrap();
                check_confluence(&input, &scrambled, &|text| format(text, &cfg))
                    .unwrap_or_else(|error| panic!("{setting}={style}: {error}"));
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/proptest-regressions/property_format_confluence.txt",
        ))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn presentation_perturbations_converge(
        document in arb_document(),
        config in arb_config(),
        rule in prop::sample::select(SCRAMBLERS.to_vec()),
        combined in any::<[bool; 16]>(),
    ) {
        let cfg = YamlLintConfig::from_toml_str(&config).unwrap();
        let input = document.render(&[], &cfg);
        let combined: Vec<_> = SCRAMBLERS.into_iter().zip(combined)
            .filter_map(|(rule, enabled)| enabled.then_some(rule)).collect();
        for rules in [vec![rule], combined] {
            let scrambled = document.render(&rules, &cfg);
            check_confluence(&input, &scrambled, &|text| format(text, &cfg))
                .map_err(|error| TestCaseError::fail(format!("{rules:?}, {config}: {error}")))?;
        }
    }
}
