use std::collections::BTreeSet;

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use ryl::config::YamlLintConfig;

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
    fn render(&self, scramblers: &[&str]) -> String {
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
        let prose = self.words.join(" ");
        let prose = if on("line-length") {
            self.words.join(&format!("\n{pad}{pad}"))
        } else {
            prose
        };
        let word = &self.words[0];
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
             {pad}text{colon}\"{prose}\"\n\
             {pad}literal: |-\n{pad}{pad}{word}\n\
             copy{colon}*root\n{blanks}"
        ));
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
    )
        .prop_map(|(indent, width, double, crlf, brace, sequences, dash, gap, sequence, mapping)| {
            format!(
                "indent-width = {indent}\nline-length = {width}\n[format]\n\
                 quote-style = '{}'\nline-ending = '{}'\nbrace-spacing = {brace}\n\
                 indent-sequences = {sequences}\ndash-on-own-line = {dash}\n\
                 comment-spacing = {gap}\ncomment-starting-space = 'add'\n\
                 document-start = 'add'\ndocument-end = 'add'\nmax-blank-lines = 0\n\
                 fold-long-lines = true\nsequence-style = '{sequence}'\nmapping-style = '{mapping}'\n",
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
    let input = document.render(&[]);
    for rules in SCRAMBLERS
        .map(|rule| vec![rule])
        .into_iter()
        .chain([SCRAMBLERS.to_vec()])
    {
        let scrambled = document.render(&rules);
        assert_ne!(input, scrambled, "{rules:?}");
        assert_ne!(format(&scrambled, &cfg), scrambled, "{rules:?}");
        check_confluence(&input, &scrambled, &|text| format(text, &cfg))
            .unwrap_or_else(|error| panic!("{rules:?}: {error}"));
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
        let input = document.render(&[]);
        let combined: Vec<_> = SCRAMBLERS.into_iter().zip(combined)
            .filter_map(|(rule, enabled)| enabled.then_some(rule)).collect();
        for rules in [vec![rule], combined] {
            let scrambled = document.render(&rules);
            check_confluence(&input, &scrambled, &|text| format(text, &cfg))
                .map_err(|error| TestCaseError::fail(format!("{rules:?}, {config}: {error}")))?;
        }
    }
}
