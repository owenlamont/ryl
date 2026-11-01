//! Property tests for `yaml-directive`'s migration of `%YAML 1.1` documents to 1.2.
//!
//! The generator draws each scalar's value first and spells it twice, once as YAML 1.1
//! (`0777`, `22:22`, `yes`) and once as the 1.2 spelling it expects, so the oracle never
//! shares the fixer's resolver. Invariants, over streams mixing `%YAML 1.1` and `1.2`
//! documents:
//!  * a `%YAML 1.1` document with no declining scalar comes out byte-for-byte as the
//!    expected 1.2 text: its directive reads `%YAML 1.2`, each divergent scalar has its
//!    expected spelling and every other byte is untouched;
//!  * a document with a declining scalar, and every `%YAML 1.2` document, is untouched;
//!  * every `%YAML 1.1` document is reported, and only the declined ones still are;
//!  * the fix is idempotent.

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use ryl::config::YamlLintConfig;
use ryl::rules::yaml_directive;
use std::sync::LazyLock;

#[derive(Debug, Clone)]
enum Outcome {
    Keep,
    Respell(String),
    Decline,
}

type Scalar = (String, Outcome);

static CONFIG: LazyLock<yaml_directive::Config> = LazyLock::new(|| {
    let cfg = YamlLintConfig::from_toml_str("target-version = \"1.2\"\n")
        .expect("target 1.2 config parses");
    yaml_directive::Config::resolve(&cfg)
});

fn underscored(digits: String) -> impl Strategy<Value = String> {
    prop::collection::vec(prop::bool::weighted(0.2), digits.len()).prop_map(
        move |marks| {
            digits
                .chars()
                .zip(marks)
                .flat_map(|(digit, mark)| [Some(digit), mark.then_some('_')])
                .flatten()
                .collect()
        },
    )
}

fn arb_bool() -> impl Strategy<Value = Scalar> {
    prop::sample::select(&["yes", "No", "ON", "off", "True", "false", "Yes", "OFF"][..])
        .prop_map(|word| {
            let value = matches!(word, "yes" | "ON" | "True" | "Yes");
            let outcome = if word.eq_ignore_ascii_case(&value.to_string()) {
                Outcome::Keep
            } else {
                Outcome::Respell(value.to_string())
            };
            (word.to_owned(), outcome)
        })
}

fn arb_int() -> impl Strategy<Value = Scalar> {
    (
        prop::sample::select(&["", "-", "+"][..]),
        1u32..100_000,
        prop::sample::select(&[2u32, 8, 10, 16][..]),
    )
        .prop_flat_map(|(sign, n, radix)| {
            let (prefix, digits) = match radix {
                2 => ("0b", format!("{n:b}")),
                8 => ("0", format!("{n:o}")),
                16 => ("0x", format!("{n:X}")),
                _ => ("", n.to_string()),
            };
            underscored(digits).prop_map(move |body| {
                let text = format!("{sign}{prefix}{body}");
                let value = if sign == "-" {
                    format!("-{n}")
                } else {
                    n.to_string()
                };
                let keep = !body.contains('_')
                    && (radix == 10 || radix == 16 && sign.is_empty());
                let outcome = match radix {
                    _ if keep => Outcome::Keep,
                    8 if sign != "-" => Outcome::Respell(format!("0o{n:o}")),
                    16 if sign != "-" => Outcome::Respell(format!("0x{n:x}")),
                    _ => Outcome::Respell(value),
                };
                (text, outcome)
            })
        })
}

fn arb_sexagesimal() -> impl Strategy<Value = Scalar> {
    (
        prop::sample::select(&["", "-"][..]),
        1u32..2000,
        prop::collection::vec((0u32..60, any::<bool>()), 1..=2),
        prop::option::of("[0-9]{0,3}"),
    )
        .prop_map(|(sign, head, parts, fraction)| {
            let mut text = format!("{sign}{head}");
            for (part, padded) in &parts {
                text.push_str(&if *padded {
                    format!(":{part:02}")
                } else {
                    format!(":{part}")
                });
            }
            let Some(fraction) = fraction else {
                let value = parts.iter().fold(u64::from(head), |sum, (part, _)| {
                    sum * 60 + u64::from(*part)
                });
                return (text, Outcome::Respell(format!("{sign}{value}")));
            };
            text.push_str(&format!(".{fraction}"));
            let last = text.rsplit(':').next().expect("a part follows a colon");
            let mut value = last.parse::<f64>().expect("decimal");
            let mut base = 60.0;
            for part in std::iter::once(head)
                .chain(parts.iter().map(|(part, _)| *part))
                .rev()
                .skip(1)
            {
                value += f64::from(part) * base;
                base *= 60.0;
            }
            let value = if sign == "-" { -value } else { value };
            (text, Outcome::Respell(format!("{value:?}")))
        })
}

fn arb_float() -> impl Strategy<Value = Scalar> {
    (
        "[0-9]{1,4}",
        "[0-9]{0,3}",
        prop::option::of("[eE][-+][0-9]"),
    )
        .prop_flat_map(|(whole, fraction, exponent)| {
            (underscored(whole), underscored(fraction)).prop_map(
                move |(whole, fraction)| {
                    let exponent = exponent.clone().unwrap_or_default();
                    let text = format!("{whole}.{fraction}{exponent}");
                    let outcome = if text.contains('_') {
                        Outcome::Respell(text.replace('_', ""))
                    } else {
                        Outcome::Keep
                    };
                    (text, outcome)
                },
            )
        })
}
fn listed(
    texts: &'static [&'static str],
    outcome: Outcome,
) -> impl Strategy<Value = Scalar> {
    prop::sample::select(texts).prop_map(move |text| (text.to_owned(), outcome.clone()))
}

fn arb_scalar() -> impl Strategy<Value = Scalar> {
    const TAGGED: [(&str, &str); 3] = [
        ("!!bool yes", "!!bool true"),
        ("!!int 0777", "!!int 0o777"),
        ("!!float 1_0.5", "!!float 10.5"),
    ];
    prop_oneof![
        2 => arb_bool(),
        3 => arb_int(),
        2 => arb_sexagesimal(),
        2 => arb_float(),
        1 => prop::sample::select(&TAGGED[..]).prop_map(|(text, spelling)| {
            (text.to_owned(), Outcome::Respell(spelling.to_owned()))
        }),
        2 => listed(
            &["word", "'yes'", "\"0777\"", "~", "null", ".inf", "-.Inf", ".NaN", "0",
              "!!str yes", "! on", "._5"],
            Outcome::Keep,
        ),
        1 => listed(
            &["2026-10-10", "08", "1e5", "1.5e3", "y", "N", "0o17", "-.5", "=", "0b_",
              "!!int yes", "!!bool 'yes'"],
            Outcome::Decline,
        ),
    ]
}

#[derive(Debug, Clone)]
enum Place {
    Value,
    Key,
    Flow,
}

#[derive(Debug, Clone)]
struct Document {
    yaml_1_1: bool,
    entries: Vec<(Scalar, Place)>,
}

impl Document {
    fn declines(&self) -> bool {
        self.entries
            .iter()
            .any(|((_, outcome), _)| matches!(outcome, Outcome::Decline))
    }

    fn render(&self, migrated: bool) -> String {
        let migrated = migrated && self.yaml_1_1 && !self.declines();
        let minor = if self.yaml_1_1 && !migrated { 1 } else { 2 };
        let mut out = format!("%YAML 1.{minor}\n---\n");
        for (index, ((text, outcome), place)) in self.entries.iter().enumerate() {
            let text = match outcome {
                Outcome::Respell(spelling) if migrated => spelling,
                _ => text,
            };
            out.push_str(&match place {
                Place::Value => format!("k{index}: {text}\n"),
                Place::Key => format!("{text}: k{index}\n"),
                Place::Flow => format!("k{index}: [{text}, x]\n"),
            });
        }
        out
    }
}

fn arb_document() -> impl Strategy<Value = Document> {
    let place = prop_oneof![Just(Place::Value), Just(Place::Key), Just(Place::Flow)];
    (
        prop::bool::weighted(0.8),
        prop::collection::vec((arb_scalar(), place), 1..=5),
    )
        .prop_map(|(yaml_1_1, entries)| Document { yaml_1_1, entries })
}

fn render(documents: &[Document], migrated: bool) -> String {
    documents
        .iter()
        .map(|document| document.render(migrated))
        .collect::<Vec<_>>()
        .join("...\n")
}

fn older_reports(text: &str) -> usize {
    yaml_directive::check(text, *CONFIG)
        .iter()
        .filter(|violation| violation.message.contains("is older than"))
        .count()
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/proptest-regressions/property_yaml_migration.txt",
        ))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn migration_matches_the_independently_spelled_yaml_1_2_text(
        documents in prop::collection::vec(arb_document(), 1..=3)
    ) {
        let input = render(&documents, false);
        let expected = render(&documents, true);
        let fixed = yaml_directive::fix(&input, &CONFIG, &[]);
        prop_assert_eq!(
            fixed.as_deref().unwrap_or(&input),
            expected.as_str(),
            "input {:?}",
            input
        );
        prop_assert_eq!(fixed.is_some(), input != expected);
        let yaml_1_1 = documents.iter().filter(|document| document.yaml_1_1);
        prop_assert_eq!(older_reports(&input), yaml_1_1.clone().count());
        prop_assert_eq!(
            older_reports(&expected),
            yaml_1_1.filter(|document| document.declines()).count()
        );
        prop_assert_eq!(yaml_directive::fix(&expected, &CONFIG, &[]), None);
    }
}
