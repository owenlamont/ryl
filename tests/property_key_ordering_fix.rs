//! Property tests for `key-ordering`'s `--fix` over the shapes its bail conditions
//! guard: nested and sequence-item mappings, leading, trailing and loose comments,
//! anchors and aliases, merge keys, same-key spellings, keep-chomping scalars, tags,
//! explicit keys, flow mappings, and suppression directives.
//!
//! Invariants, under a codepoint config, a locale-plus-`ignored-keys` config, and
//! `orders` configs with either `unlisted` value:
//!  * one `apply_safe_fixes` reaches a fixed point and keeps the loaded data;
//!  * every line survives, so no text is lost or invented;
//!  * each comment classified as leading (directly above a key at its column) or
//!    trailing (indented deeper than the line above) keeps its anchor line;
//!  * the fixer never needs its verification backstop: every mapping still out of
//!    order is named by `unfixed` with a bail reason, and when `unfixed` names none,
//!    no `key-ordering` diagnostic remains.

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use ryl::config::{Overrides, YamlLintConfig, discover_config};
use ryl::fix::apply_safe_fixes;
use ryl::lint::lint_str;
use ryl::rules::key_ordering;
use ryl::yaml_dom::YamlOwned;
use std::path::Path;
use std::sync::LazyLock;
use tempfile::TempDir;

const CONFIGS: [&str; 4] = [
    "[rules.key-ordering]\n",
    "locale = 'en_US.UTF-8'\n[rules.key-ordering]\nignored-keys = ['^c']\n",
    "[rules.key-ordering]\n\n[[rules.key-ordering.orders]]\nfiles = ['*']\npath = '$'\n\
     keys = ['d', 'b']\n\n[[rules.key-ordering.orders]]\nfiles = ['*']\n\
     path = '$.*[*]'\nkeys = ['c', 'a']\n",
    "[rules.key-ordering]\nignored-keys = ['^c']\n\n[[rules.key-ordering.orders]]\n\
     files = ['*']\npath = \"$['a'][*]\"\nkeys = ['d', 'a']\nunlisted = 'keep'\n\n\
     [[rules.key-ordering.orders]]\nfiles = ['*']\npath = '$.*.*'\n\
     keys = ['b', 'a']\nunlisted = 'keep'\n",
];
static LOADED: LazyLock<(TempDir, Vec<YamlLintConfig>)> = LazyLock::new(|| {
    let dir = TempDir::new().expect("tempdir");
    let configs = CONFIGS.iter().enumerate().map(|(index, toml)| {
        let file = dir.path().join(format!("{index}.toml"));
        std::fs::write(&file, toml).expect("write config");
        let overrides = Overrides {
            config_file: Some(file),
            config_data: None,
        };
        discover_config(&[], &overrides)
            .expect("config loads")
            .config
    });
    let configs = configs.collect();
    (dir, configs)
});
const VERIFY: &str = "the sorted output failed verification";

#[derive(Debug, Clone)]
enum Value {
    Plain(&'static str),
    Anchored(&'static str),
    Alias(&'static str),
    Flow(&'static str),
    Keep,
    Map(Map),
    Seq(Vec<Map>),
}

#[derive(Debug, Clone)]
struct Entry {
    above: Option<&'static str>,
    key: &'static str,
    value: Value,
    inline: Option<&'static str>,
    below: Option<&'static str>,
}

#[derive(Debug, Clone)]
struct Map {
    tag: bool,
    entries: Vec<Entry>,
}

fn arb_entry(map: BoxedStrategy<Map>) -> impl Strategy<Value = Entry> {
    let value = prop_oneof![
        10 => prop_oneof![Just("1"), Just("x"), Just("~")].prop_map(Value::Plain),
        2 => prop_oneof![Just("p"), Just("q")].prop_map(Value::Anchored),
        2 => prop_oneof![Just("p"), Just("q")].prop_map(Value::Alias),
        1 => prop_oneof![Just("{a: 1, b: 2}"), Just("{b: 1, a: 2}")].prop_map(Value::Flow),
        1 => Just(Value::Keep),
        3 => map.clone().prop_map(Value::Map),
        2 => prop::collection::vec(map, 1..=2).prop_map(Value::Seq),
    ];
    (
        prop::option::weighted(
            0.4,
            prop_oneof![
                12 => Just("# lead"),
                1 => Just("# loose\n"),
                3 => Just("\n"),
                1 => Just("# ryl disable-line rule:key-ordering"),
                1 => Just("# ryl disable rule:truthy"),
                1 => Just("# ryl disable-line rule:truthy"),
            ],
        ),
        prop_oneof![
            24 => prop_oneof![Just("a"), Just("b"), Just("c"), Just("d")],
            1 => Just("true"),
            1 => Just("True"),
            1 => Just("<<"),
            1 => Just("? e"),
            1 => Just("&k f"),
        ],
        value,
        prop::option::weighted(
            0.2,
            prop_oneof![
                5 => Just("# note"),
                1 => Just("# ryl disable-line rule:key-ordering"),
            ],
        ),
        prop::option::weighted(0.2, prop_oneof![Just("  # deep"), Just("# shallow")]),
    )
        .prop_map(|(above, key, value, inline, below)| Entry {
            above,
            key,
            value,
            inline,
            below,
        })
}

fn arb_map() -> impl Strategy<Value = Map> {
    let leaf = (prop::bool::weighted(0.05), Just(Vec::new()))
        .prop_map(|(tag, entries)| Map { tag, entries })
        .boxed();
    leaf.prop_recursive(3, 24, 4, |inner| {
        (
            prop::bool::weighted(0.05),
            prop::collection::vec(arb_entry(inner), 1..=4),
        )
            .prop_map(|(tag, entries)| Map { tag, entries })
    })
}

fn render_map(map: &Map, indent: &str, first: &str, out: &mut Vec<String>) {
    if map.tag && first == indent {
        out.push(format!("{indent}!t"));
    }
    for (index, entry) in map.entries.iter().enumerate() {
        let prefix = if index == 0 { first } else { indent };
        if let Some(above) = entry.above.filter(|_| index > 0 || first == indent) {
            out.extend(above.split('\n').map(|line| {
                if line.is_empty() {
                    String::new()
                } else {
                    format!("{indent}{line}")
                }
            }));
        }
        let inline = entry.inline.map_or(String::new(), |c| format!("  {c}"));
        let head = format!("{prefix}{}:", entry.key);
        let deeper = format!("{indent}  ");
        match &entry.value {
            Value::Plain(text) => out.push(format!("{head} {text}{inline}")),
            Value::Anchored(name) => out.push(format!("{head} &{name} v{inline}")),
            Value::Alias(name) => out.push(format!("{head} *{name}{inline}")),
            Value::Flow(flow) => out.push(format!("{head} {flow}{inline}")),
            Value::Keep => {
                out.push(format!("{head} |+{inline}"));
                out.push(format!("{deeper}t"));
                out.push(String::new());
            }
            Value::Map(inner) if !inner.entries.is_empty() => {
                out.push(format!("{head}{inline}"));
                render_map(inner, &deeper, &deeper, out);
            }
            Value::Seq(items) => {
                out.push(format!("{head}{inline}"));
                for item in items.iter().filter(|item| !item.entries.is_empty()) {
                    render_map(
                        item,
                        &format!("{indent}    "),
                        &format!("{deeper}- "),
                        out,
                    );
                }
            }
            Value::Map(_) => out.push(format!("{head} 0{inline}")),
        }
        if let Some(below) = entry.below {
            out.push(format!("{indent}{below}"));
        }
    }
}

fn render(map: &Map) -> String {
    let mut lines = Vec::new();
    render_map(map, "", "", &mut lines);
    lines.join("\n") + "\n"
}

fn loaded(text: &str) -> Option<Vec<YamlOwned>> {
    fn canonical(node: YamlOwned) -> YamlOwned {
        match node {
            YamlOwned::Mapping(mapping) => {
                let mut entries: Vec<_> = mapping
                    .into_iter()
                    .map(|(key, value)| (canonical(key), canonical(value)))
                    .collect();
                entries.sort_by_cached_key(|(key, _)| format!("{key:?}"));
                YamlOwned::Mapping(entries.into_iter().collect())
            }
            YamlOwned::Sequence(items) => {
                YamlOwned::Sequence(items.into_iter().map(canonical).collect())
            }
            YamlOwned::Tagged(tag, inner) => {
                YamlOwned::Tagged(tag, Box::new(canonical(*inner)))
            }
            other => other,
        }
    }
    Some(
        YamlOwned::load_from_str(text)
            .ok()?
            .into_iter()
            .map(canonical)
            .collect(),
    )
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

fn anchor(line: &str) -> &str {
    line.trim_start_matches([' ', '-'])
}

/// `(kind, comment, anchor)` for each own-line comment the fix must carry along: a
/// run at one column directly above a line at that column leads it, and a run indented
/// deeper than the line directly above trails that line.
fn attachments(text: &str) -> Vec<(char, String, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let is_comment = |line: &str| line.trim_start().starts_with('#');
    let is_content = |line: &&str| !is_comment(line) && !line.trim().is_empty();
    let mut found = Vec::new();
    for (index, line) in lines.iter().enumerate().filter(|(_, l)| is_comment(l)) {
        let column = indent(line);
        let below = lines[index + 1..]
            .iter()
            .find(|l| !(is_comment(l) && indent(l) == column))
            .filter(|next| is_content(next) && indent(next) == column);
        let start = lines[..index].iter().rposition(|l| !is_comment(l));
        let above = start
            .map(|at| lines[at])
            .filter(|prev| is_content(prev))
            .filter(|prev| {
                lines[start.unwrap_or(0) + 1..=index]
                    .iter()
                    .all(|l| indent(l) > indent(prev))
            });
        if let Some(next) = below {
            found.push(('L', (*line).to_owned(), anchor(next).to_owned()));
        } else if let Some(prev) = above {
            found.push(('T', (*line).to_owned(), anchor(prev).to_owned()));
        }
    }
    found.sort();
    found
}

fn sorted_lines(text: &str) -> Vec<String> {
    let mut lines: Vec<String> =
        text.lines().map(|line| anchor(line).to_owned()).collect();
    lines.sort();
    lines
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/proptest-regressions/property_key_ordering_fix.txt",
        ))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn key_ordering_fix_is_sound_and_explains_what_it_leaves(map in arb_map()) {
        let input = render(&map);
        let Some(before) = loaded(&input) else {
            return Ok(());
        };
        for cfg in &LOADED.1 {
            let path = Path::new("synthetic.yaml");
            let fixed = apply_safe_fixes(&input, cfg, path, Path::new("."));
            prop_assert_eq!(
                apply_safe_fixes(&fixed, cfg, path, Path::new(".")),
                fixed.clone(),
                "not idempotent; input {:?}", input
            );
            prop_assert_eq!(loaded(&fixed), Some(before.clone()), "data changed; input {:?}", input);
            prop_assert_eq!(sorted_lines(&fixed), sorted_lines(&input), "lines changed; input {:?}", input);
            prop_assert_eq!(
                attachments(&fixed),
                attachments(&input),
                "a comment changed anchor; input {:?}; fixed {:?}", input, fixed
            );
            let rule = key_ordering::Config::resolve(cfg, path);
            let unfixed = key_ordering::unfixed(&fixed, &rule, &[]);
            prop_assert!(
                unfixed.iter().all(|notice| notice.message != VERIFY),
                "the verification backstop fired; input {:?}; fixed {:?}", input, fixed
            );
            let remaining = lint_str(&fixed, path, cfg, Path::new("."))
                .iter()
                .any(|problem| problem.rule == Some(key_ordering::ID));
            prop_assert!(
                !remaining || !unfixed.is_empty(),
                "unsorted with no notice; input {:?}; fixed {:?}", input, fixed
            );
        }
    }
}

#[test]
fn generator_reaches_both_sorted_and_declined_mappings() {
    let rule = key_ordering::Config::resolve(&LOADED.1[0], Path::new("t.yaml"));
    let sorted = "b:\n  - d: 1\n    c: 2\n  # deep\na: &p v\n";
    assert_eq!(
        key_ordering::fix(sorted, &rule, &[]).as_deref(),
        Some("a: &p v\nb:\n  - c: 2\n    d: 1\n  # deep\n")
    );
    let declined = "b: 1\n# loose\n\na: 2\n";
    assert_eq!(key_ordering::fix(declined, &rule, &[]), None);
    assert_eq!(key_ordering::unfixed(declined, &rule, &[]).len(), 1);
}

#[test]
fn orders_configs_select_the_generated_shapes() {
    let fix = |config: usize, input| {
        let rule =
            key_ordering::Config::resolve(&LOADED.1[config], Path::new("t.yaml"));
        key_ordering::fix(input, &rule, &[])
    };
    assert_eq!(fix(2, "b: 1\nd: 2\n").as_deref(), Some("d: 2\nb: 1\n"));
    assert_eq!(
        fix(2, "x:\n  - a: 1\n    b: 2\n    c: 3\n").as_deref(),
        Some("x:\n  - c: 3\n    a: 1\n    b: 2\n")
    );
    assert_eq!(
        fix(3, "x:\n  y:\n    a: 1\n    c: 0\n    d: 3\n    b: 2\n").as_deref(),
        Some("x:\n  y:\n    b: 2\n    c: 0\n    d: 3\n    a: 1\n")
    );
    assert_eq!(
        fix(3, "a:\n  - b: 0\n    a: 1\n    d: 2\n").as_deref(),
        Some("a:\n  - b: 0\n    d: 2\n    a: 1\n")
    );
}
