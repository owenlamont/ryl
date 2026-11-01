//! Property tests for the unsafe fixes of `truthy`, `octal-values` and
//! `schema-dependent-values` in an undeclared document under `target-version = "1.1"`.
//!
//! The generator draws each scalar's value first and spells, per `source-version`, the
//! fix it expects, so the oracle never shares the fixer's resolver. Every expected
//! spelling is one YAML 1.1 and 1.2 readers agree on by construction: a quoted string, a
//! decimal integer, `true`/`false`, or a decimal float with a `.`. Invariants, under each
//! source version, with unsafe fixes on and off, and with `quoted-strings` disabled or
//! asking for single quotes, no needless quotes (`required = false`), or both:
//!  * `--fix` output is byte-for-byte the expected text: each scalar keeping the value
//!    the source version reads takes its expected spelling, a declined or unflagged one
//!    is untouched, and with unsafe fixes off only the safe `truthy` re-casing applies;
//!  * fixing that output again changes nothing;
//!  * with unsafe fixes off each unapplied fix is counted for the hint and no message
//!    names one; with them on none is counted and each applied fix's message names it.

use std::path::Path;
use std::sync::LazyLock;

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use ryl::config::YamlLintConfig;
use ryl::fix::apply_safe_fixes;
use ryl::rules::{octal_values, schema_dependent_values, truthy};

#[derive(Debug, Clone)]
enum Outcome {
    Keep,
    Safe(String),
    Unsafe(String),
    Quote,
    Decline,
}

/// A scalar's text and its outcome under `source-version = "1.2"` and `"1.1"`.
type Scalar = (String, [Outcome; 2]);

fn underscored(digits: String) -> impl Strategy<Value = String> {
    prop::collection::vec(prop::bool::weighted(0.3), digits.len()).prop_map(
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

fn signed(sign: &str, value: u64) -> String {
    if sign == "-" && value != 0 {
        format!("-{value}")
    } else {
        value.to_string()
    }
}

fn arb_truthy() -> impl Strategy<Value = Scalar> {
    prop::sample::select(
        &[
            "yes", "No", "ON", "off", "Yes", "OFF", "no", "On", "True", "FALSE",
        ][..],
    )
    .prop_map(|word| {
        let value = matches!(word, "yes" | "ON" | "Yes" | "On" | "True");
        let outcomes = if word.eq_ignore_ascii_case(&value.to_string()) {
            [
                Outcome::Safe(value.to_string()),
                Outcome::Safe(value.to_string()),
            ]
        } else {
            [Outcome::Quote, Outcome::Unsafe(value.to_string())]
        };
        (word.to_owned(), outcomes)
    })
}

fn arb_octal() -> impl Strategy<Value = Scalar> {
    (
        prop::sample::select(&["", "-", "+"][..]),
        0u64..5000,
        1usize..=2,
        any::<bool>(),
    )
        .prop_map(|(sign, n, zeros, explicit)| {
            if explicit {
                let text = format!("0o{n:o}");
                return (text, [Outcome::Unsafe(n.to_string()), Outcome::Decline]);
            }
            let digits = format!("{}{n:o}", "0".repeat(zeros));
            let as_decimal = digits.parse().expect("octal digits are decimal digits");
            (
                format!("{sign}{digits}"),
                [
                    Outcome::Unsafe(signed(sign, as_decimal)),
                    Outcome::Unsafe(signed(sign, n)),
                ],
            )
        })
}

fn arb_leading_zero() -> impl Strategy<Value = Scalar> {
    (
        prop::sample::select(&["", "-", "+"][..]),
        1usize..=2,
        8u64..100_000,
    )
        .prop_filter("an 8 or 9 makes the digits non-octal", |(_, _, n)| {
            n.to_string().contains(['8', '9'])
        })
        .prop_map(|(sign, zeros, n)| {
            (
                format!("{sign}{}{n}", "0".repeat(zeros)),
                [Outcome::Unsafe(signed(sign, n)), Outcome::Decline],
            )
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
                return (text, [Outcome::Quote, Outcome::Unsafe(signed(sign, value))]);
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
            (
                text,
                [Outcome::Quote, Outcome::Unsafe(format!("{value:?}"))],
            )
        })
}

fn arb_underscored() -> impl Strategy<Value = Scalar> {
    (
        prop::sample::select(&["", "-"][..]),
        1u64..100_000,
        prop::sample::select(&[2u32, 10, 16][..]),
    )
        .prop_flat_map(|(sign, n, radix)| {
            let (prefix, digits) = match radix {
                2 => ("0b", format!("{n:b}")),
                16 => ("0x", format!("{n:X}")),
                _ => ("", n.to_string()),
            };
            underscored(digits).prop_map(move |body| {
                let text = format!("{sign}{prefix}{body}");
                let core_int = radix == 10 || radix == 16 && sign.is_empty();
                let outcomes = if core_int && !body.contains('_') {
                    [Outcome::Keep, Outcome::Keep]
                } else {
                    [Outcome::Quote, Outcome::Unsafe(signed(sign, n))]
                };
                (text, outcomes)
            })
        })
}

fn arb_underscored_float() -> impl Strategy<Value = Scalar> {
    (
        "[1-9][0-9]{0,3}",
        "[0-9]{0,3}",
        prop::option::of("[eE][-+][0-9]"),
    )
        .prop_flat_map(|(whole, fraction, exponent)| {
            (underscored(whole), underscored(fraction)).prop_map(
                move |(whole, fraction)| {
                    let exponent = exponent.clone().unwrap_or_default();
                    let text = format!("{whole}.{fraction}{exponent}");
                    let value: f64 =
                        text.replace('_', "").parse().expect("a decimal float");
                    let spelling = format!("{value:?}");
                    // PyYAML reads a float only with a `.` and a signed exponent, so `9e-9`
                    // and `1.5e16` have no shared spelling.
                    let shared = spelling.contains('.')
                        && spelling
                            .split_once('e')
                            .is_none_or(|(_, exp)| exp.starts_with('-'));
                    let yaml_1_1 = if shared {
                        Outcome::Unsafe(spelling)
                    } else {
                        Outcome::Decline
                    };
                    let outcomes = if text.contains('_') {
                        [Outcome::Quote, yaml_1_1]
                    } else {
                        [Outcome::Keep, Outcome::Keep]
                    };
                    (text, outcomes)
                },
            )
        })
}

fn listed(
    texts: &'static [&'static str],
    outcomes: [Outcome; 2],
) -> impl Strategy<Value = Scalar> {
    prop::sample::select(texts)
        .prop_map(move |text| (text.to_owned(), outcomes.clone()))
}

fn arb_scalar() -> impl Strategy<Value = Scalar> {
    prop_oneof![
        3 => arb_truthy(),
        3 => arb_octal(),
        1 => arb_leading_zero(),
        2 => arb_sexagesimal(),
        2 => arb_underscored(),
        2 => arb_underscored_float(),
        1 => listed(&["2026-10-10", "1999-01-31", "y", "N"], [Outcome::Quote, Outcome::Decline]),
        2 => listed(
            &["word", "'yes'", "'0777'", "0", "12", "1.5", ".inf", "~", "null", "-0o17",
              "0x1F", "true", "false", "1e5", "!!str yes", "!!int 0777"],
            [Outcome::Keep, Outcome::Keep],
        ),
    ]
}

#[derive(Debug, Clone)]
enum Place {
    Value,
    Key,
    Flow,
}

#[derive(Debug, Clone, Copy)]
struct Variant {
    yaml_1_1: bool,
    unsafe_fixes: bool,
    single_quotes: bool,
    quotes_forbidden: bool,
}

impl Variant {
    const COUNT: u8 = 16;

    const fn from_bits(bits: u8) -> Self {
        Self {
            yaml_1_1: bits & 1 != 0,
            unsafe_fixes: bits & 2 != 0,
            single_quotes: bits & 4 != 0,
            quotes_forbidden: bits & 8 != 0,
        }
    }

    fn config(self) -> YamlLintConfig {
        let mut text = String::new();
        if self.yaml_1_1 {
            text.push_str("source-version = \"1.1\"\n");
        }
        if self.unsafe_fixes {
            text.push_str("unsafe-fixes = true\n");
        }
        text.push_str(
            "[lint.rules]\ntruthy = \"enable\"\noctal-values = \"enable\"\n\
             schema-dependent-values = \"enable\"\n",
        );
        if self.single_quotes || self.quotes_forbidden {
            let quote_type = if self.single_quotes { "single" } else { "any" };
            let required = if self.quotes_forbidden {
                "false"
            } else {
                "\"only-when-needed\""
            };
            text.push_str(&format!(
                "quoted-strings = {{ level = \"error\", quote-type = \"{quote_type}\", \
                 required = {required} }}\n"
            ));
        }
        YamlLintConfig::from_toml_str(&text).expect("variant config parses")
    }

    /// `text` as `--fix` should leave it under this variant.
    fn expected<'a>(self, text: &'a str, outcomes: &'a [Outcome; 2]) -> String {
        let quote = if self.single_quotes { '\'' } else { '"' };
        match &outcomes[usize::from(self.yaml_1_1)] {
            Outcome::Safe(spelling) => spelling.clone(),
            Outcome::Unsafe(spelling) if self.unsafe_fixes => spelling.clone(),
            Outcome::Quote if self.unsafe_fixes => format!("{quote}{text}{quote}"),
            _ => text.to_owned(),
        }
    }

    fn fixes(self, outcomes: &[Outcome; 2]) -> bool {
        matches!(
            outcomes[usize::from(self.yaml_1_1)],
            Outcome::Unsafe(_) | Outcome::Quote
        )
    }
}

static CONFIGS: LazyLock<Vec<YamlLintConfig>> = LazyLock::new(|| {
    (0..Variant::COUNT)
        .map(|bits| Variant::from_bits(bits).config())
        .collect()
});

fn render(
    entries: &[(Scalar, Place)],
    spell: impl Fn(&str, &[Outcome; 2]) -> String,
) -> String {
    entries
        .iter()
        .enumerate()
        .map(|(index, ((text, outcomes), place))| {
            let text = spell(text, outcomes);
            match place {
                Place::Value => format!("k{index}: {text}\n"),
                Place::Key => format!("{text}: k{index}\n"),
                Place::Flow => format!("k{index}: [{text}, x]\n"),
            }
        })
        .collect()
}

/// Each diagnostic of the three rules as (message, counted for the hint).
fn findings(text: &str, cfg: &YamlLintConfig) -> Vec<(String, bool)> {
    let truthy = truthy::check(text, &truthy::Config::resolve(cfg));
    let octal = octal_values::check(text, &octal_values::Config::resolve(cfg));
    let schema = schema_dependent_values::check(
        text,
        &schema_dependent_values::Config::resolve(cfg),
    );
    truthy
        .into_iter()
        .map(|v| (v.message, v.hidden_unsafe_fix))
        .chain(octal.into_iter().map(|v| (v.message, v.hidden_unsafe_fix)))
        .chain(schema.into_iter().map(|v| (v.message, v.hidden_unsafe_fix)))
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/proptest-regressions/property_unsafe_fix.txt",
        ))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn unsafe_fixes_keep_the_source_reading_in_a_spelling_both_versions_share(
        entries in prop::collection::vec(
            (arb_scalar(), prop_oneof![Just(Place::Value), Just(Place::Key), Just(Place::Flow)]),
            1..=6,
        ),
        bits in 0..Variant::COUNT,
    ) {
        let variant = Variant::from_bits(bits);
        let cfg = &CONFIGS[usize::from(bits)];
        let (path, base) = (Path::new("x.yaml"), Path::new("."));
        let input = render(&entries, |text, _| text.to_owned());
        let expected = render(&entries, |text, outcomes| variant.expected(text, outcomes));
        let fixed = apply_safe_fixes(&input, cfg, path, base);
        prop_assert_eq!(&fixed, &expected, "input {:?} under {:?}", input, variant);
        prop_assert_eq!(apply_safe_fixes(&fixed, cfg, path, base), fixed);
        let fixes = entries.iter().filter(|((_, outcomes), _)| variant.fixes(outcomes)).count();
        let found = findings(&input, cfg);
        let hidden = found.iter().filter(|(_, hidden)| *hidden).count();
        let named = found.iter().filter(|(message, _)| message.contains("; unsafe fix: ")).count();
        let (hidden_expected, named_expected) =
            if variant.unsafe_fixes { (0, fixes) } else { (fixes, 0) };
        prop_assert_eq!(hidden, hidden_expected, "{:?}", found);
        prop_assert_eq!(named, named_expected, "{:?}", found);
    }
}
