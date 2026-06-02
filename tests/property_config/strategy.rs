//! Strategy for `property_config`: generate plausible-but-sometimes-invalid ryl
//! configurations and render them to YAML and TOML.
//!
//! Option values are drawn from curated pools that deliberately mix the valid with
//! the hostile — invalid regexes (`(`, `(a+)+$`), out-of-range and wrong-typed
//! scalars, bogus locales — so the parse -> validate -> rule-`resolve()` pipeline is
//! exercised with inputs that must produce an error or a clean config, never a
//! panic. The regex-bearing rules (`key-ordering`, `quoted-strings`) are generated
//! with their real option keys so generation reaches the `.expect()` calls in their
//! `resolve()` paths rather than bouncing off `deny_unknown_fields` at parse time.

use proptest::prelude::*;

/// A single option value, renderable to both YAML and TOML scalar/array syntax. The
/// string pools never contain quotes, backslashes, or newlines, so naive quoting is
/// valid in both formats.
#[derive(Debug, Clone)]
pub enum OptVal {
    Bool(bool),
    Int(i64),
    Str(&'static str),
    List(Vec<&'static str>),
}

#[derive(Debug, Clone)]
pub enum Setting {
    Disable,
    Enable,
    Leveled {
        level: &'static str,
        options: Vec<(&'static str, OptVal)>,
    },
}

#[derive(Debug, Clone)]
pub struct RuleCfg {
    pub id: &'static str,
    pub setting: Setting,
}

#[derive(Debug, Clone)]
pub struct ConfigModel {
    pub rules: Vec<RuleCfg>,
    pub ignore: Option<Vec<&'static str>>,
    pub locale: Option<&'static str>,
}

/// Regex strings spanning valid, invalid, and pathological-but-valid (the last to
/// confirm the linear `regex` engine, not catastrophic backtracking).
const REGEX_POOL: &[&str] = &["^ok$", "(", "[", "(a+)+$", ".*", "[0-9]+"];
const LEVELS: &[&str] = &["error", "warning", "bogus"];
const LOCALES: &[&str] = &["en_US.UTF-8", "C", "garbage", "xx_XX"];
const INTS: &[i64] = &[-1, 0, 1, 2, 80, 9999];

/// What kind of value an option accepts; `arb` yields both well-typed and
/// deliberately ill-typed values so validation is stressed.
#[derive(Debug, Clone, Copy)]
pub enum OptValKind {
    Bool,
    Int,
    RegexList,
    StrEnum(&'static [&'static str]),
}

impl OptValKind {
    fn arb(self) -> BoxedStrategy<OptVal> {
        match self {
            Self::Bool => any::<bool>().prop_map(OptVal::Bool).boxed(),
            Self::Int => prop::sample::select(INTS).prop_map(OptVal::Int).boxed(),
            Self::RegexList => {
                prop::collection::vec(prop::sample::select(REGEX_POOL), 0..3)
                    .prop_map(OptVal::List)
                    .boxed()
            }
            Self::StrEnum(choices) => {
                prop::sample::select(choices).prop_map(OptVal::Str).boxed()
            }
        }
    }
}

/// Rules paired with a couple of their real option keys and the value kinds those
/// keys accept. Keeping keys real lets generation reach each rule's `resolve()`.
const CATALOG: &[(&str, &[(&str, OptValKind)])] = &[
    ("key-ordering", &[("ignored-keys", OptValKind::RegexList)]),
    (
        "quoted-strings",
        &[
            ("extra-required", OptValKind::RegexList),
            ("extra-allowed", OptValKind::RegexList),
            (
                "quote-type",
                OptValKind::StrEnum(&["single", "double", "any", "bogus"]),
            ),
        ],
    ),
    (
        "line-length",
        &[
            ("max", OptValKind::Int),
            ("allow-non-breakable-words", OptValKind::Bool),
        ],
    ),
    ("comments", &[("min-spaces-from-content", OptValKind::Int)]),
    ("braces", &[("max-spaces-inside", OptValKind::Int)]),
    ("truthy", &[("check-keys", OptValKind::Bool)]),
    ("anchors", &[("forbid-unused-anchors", OptValKind::Bool)]),
    ("document-start", &[("present", OptValKind::Bool)]),
    ("trailing-spaces", &[]),
    (
        "octal-values",
        &[("forbid-implicit-octal", OptValKind::Bool)],
    ),
];

fn arb_rule() -> impl Strategy<Value = RuleCfg> {
    prop::sample::select(CATALOG).prop_flat_map(|(id, opts)| {
        let options: BoxedStrategy<Vec<(&'static str, OptVal)>> = if opts.is_empty() {
            Just(Vec::new()).boxed()
        } else {
            prop::collection::vec(
                prop::sample::select(opts.to_vec())
                    .prop_flat_map(|(key, kind)| arb_optval_for(key, kind)),
                0..=opts.len(),
            )
            .boxed()
        };
        prop_oneof![
            Just(Setting::Disable),
            Just(Setting::Enable),
            (prop::sample::select(LEVELS), options)
                .prop_map(|(level, options)| Setting::Leveled { level, options }),
        ]
        .prop_map(move |setting| RuleCfg { id, setting })
    })
}

fn arb_optval_for(
    key: &'static str,
    kind: OptValKind,
) -> impl Strategy<Value = (&'static str, OptVal)> {
    kind.arb().prop_map(move |value| (key, value))
}

pub fn arb_config() -> impl Strategy<Value = ConfigModel> {
    (
        prop::collection::vec(arb_rule(), 0..6),
        prop::option::of(prop::collection::vec(
            prop::sample::select(&["vendor/**", "*.gen.yaml", "[", ""][..]),
            0..3,
        )),
        prop::option::of(prop::sample::select(LOCALES)),
    )
        .prop_map(|(rules, ignore, locale)| ConfigModel {
            rules,
            ignore: ignore.map(|patterns| patterns.into_iter().collect()),
            locale,
        })
}

fn render_optval_yaml(value: &OptVal) -> String {
    match value {
        OptVal::Bool(b) => b.to_string(),
        OptVal::Int(i) => i.to_string(),
        OptVal::Str(s) => format!("\"{s}\""),
        OptVal::List(items) => {
            let inner: Vec<String> = items.iter().map(|s| format!("\"{s}\"")).collect();
            format!("[{}]", inner.join(", "))
        }
    }
}

fn render_optval_toml(value: &OptVal) -> String {
    // Same shapes as YAML for these pools; booleans/ints/strings/arrays render
    // identically in TOML scalar syntax.
    render_optval_yaml(value)
}

pub fn render_yaml(model: &ConfigModel) -> String {
    let mut out = String::new();
    if let Some(patterns) = &model.ignore {
        out.push_str("ignore:\n");
        for pattern in patterns {
            out.push_str(&format!("  - \"{pattern}\"\n"));
        }
    }
    if let Some(locale) = model.locale {
        out.push_str(&format!("locale: \"{locale}\"\n"));
    }
    out.push_str("rules:\n");
    for rule in &model.rules {
        match &rule.setting {
            Setting::Disable => out.push_str(&format!("  {}: disable\n", rule.id)),
            Setting::Enable => out.push_str(&format!("  {}: enable\n", rule.id)),
            Setting::Leveled { level, options } => {
                out.push_str(&format!("  {}:\n", rule.id));
                out.push_str(&format!("    level: \"{level}\"\n"));
                for (key, value) in options {
                    out.push_str(&format!(
                        "    {key}: {}\n",
                        render_optval_yaml(value)
                    ));
                }
            }
        }
    }
    out
}

pub fn render_toml(model: &ConfigModel) -> String {
    let mut out = String::new();
    if let Some(patterns) = &model.ignore {
        let inner: Vec<String> = patterns.iter().map(|p| format!("\"{p}\"")).collect();
        out.push_str(&format!("ignore = [{}]\n", inner.join(", ")));
    }
    if let Some(locale) = model.locale {
        out.push_str(&format!("locale = \"{locale}\"\n"));
    }
    // Scalar toggles belong under the inline [rules] table; leveled rules each get
    // their own [rules.<id>] table emitted afterwards so TOML stays well-formed.
    out.push_str("[rules]\n");
    for rule in &model.rules {
        match &rule.setting {
            Setting::Disable => out.push_str(&format!("{} = \"disable\"\n", rule.id)),
            Setting::Enable => out.push_str(&format!("{} = \"enable\"\n", rule.id)),
            Setting::Leveled { .. } => {}
        }
    }
    for rule in &model.rules {
        if let Setting::Leveled { level, options } = &rule.setting {
            out.push_str(&format!("[rules.{}]\n", rule.id));
            out.push_str(&format!("level = \"{level}\"\n"));
            for (key, value) in options {
                out.push_str(&format!("{key} = {}\n", render_optval_toml(value)));
            }
        }
    }
    out
}
