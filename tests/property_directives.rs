//! A comment with no whitespace-then-`#` must parse exactly as the whole-payload
//! directive grammar did before directives could share a comment with other text.

use std::sync::LazyLock;

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use regex::Regex;
use ryl::directives::Directives;
use ryl::rules::ALL_RULE_IDS;

static SEGMENT_BREAK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([ \t])#").unwrap());
static OLD_DIRECTIVE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^ (?:yamllint|ryl) (disable-line|disable|enable)(?: rule:\S+)*\s*$")
        .unwrap()
});
static OLD_DISABLE_FILE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^#\s*(?:yamllint|ryl) disable-file\s*$").unwrap());
static RULE_TOKEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"rule:(\S+)").unwrap());

const PREFIXES: &[&str] = &[" ryl ", " yamllint ", "  ryl ", "#ryl ", " x "];
const ACTIONS: &[&str] = &["disable", "enable", "disable-line", "disable-file", ""];
const TOKENS: &[&str] = &[
    " rule:colons",
    " rule:truthy",
    " rule:bogus",
    "rule:colons",
    " ",
    "\t",
    "#",
    "x",
];

fn arb_single_segment_payload() -> impl Strategy<Value = String> {
    (
        prop::sample::select(PREFIXES),
        prop::sample::select(ACTIONS),
        prop::collection::vec(prop::sample::select(TOKENS), 0..6),
    )
        .prop_map(|(prefix, action, tokens)| {
            let payload = format!("{prefix}{action}{}", tokens.concat());
            SEGMENT_BREAK.replace_all(&payload, "${1}x#").into_owned()
        })
}

/// `(rules disabled on line 1, rules disabled on line 2)` under the old grammar.
fn old_disabled(payload: &str) -> (Vec<&'static str>, Vec<&'static str>) {
    let Some(caps) = OLD_DIRECTIVE.captures(payload) else {
        return (Vec::new(), Vec::new());
    };
    let rules: Vec<&'static str> = if RULE_TOKEN.is_match(payload) {
        ALL_RULE_IDS
            .into_iter()
            .filter(|id| {
                RULE_TOKEN
                    .captures_iter(payload)
                    .any(|token| &token[1] == *id)
            })
            .collect()
    } else {
        ALL_RULE_IDS.to_vec()
    };
    match &caps[1] {
        "disable-line" => (rules, Vec::new()),
        "disable" => (rules.clone(), rules),
        _ => (Vec::new(), Vec::new()),
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/proptest-regressions/property_directives.txt",
        ))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn single_segment_comments_parse_as_before(payload in arb_single_segment_payload()) {
        let directives = Directives::parse(&format!("a: 1  #{payload}\nb: 2\n"));
        let disabled_on = |line| -> Vec<&'static str> {
            ALL_RULE_IDS
                .into_iter()
                .filter(|id| directives.is_disabled(id, line))
                .collect()
        };
        prop_assert_eq!((disabled_on(1), disabled_on(2)), old_disabled(&payload));
        prop_assert_eq!(
            ryl::directives::disables_file(&format!("#{payload}\na: 1\n")),
            OLD_DISABLE_FILE.is_match(&format!("#{payload}"))
        );
    }
}
