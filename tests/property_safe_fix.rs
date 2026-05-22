use std::path::Path;

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use ryl::config::YamlLintConfig;
use ryl::fix::apply_safe_fixes;
use ryl::lint::{LintProblem, lint_str};
use saphyr::{LoadableYamlNode, YamlOwned};

const SAFE_FIX_CONFIG_YAML: &str = "rules:
  new-lines: enable
  comments: enable
  comments-indentation: enable
  commas: enable
  braces: enable
  brackets: enable
  new-line-at-end-of-file: enable
  quoted-strings: enable
";

const SAFE_FIX_RULES: &[&str] = &[
    "new-lines",
    "comments",
    "comments-indentation",
    "commas",
    "braces",
    "brackets",
    "new-line-at-end-of-file",
    "quoted-strings",
];

fn safe_fix_config() -> YamlLintConfig {
    YamlLintConfig::from_yaml_str(SAFE_FIX_CONFIG_YAML)
        .expect("safe-fix config string is valid")
}

fn synthetic_path() -> &'static Path {
    Path::new("synthetic.yaml")
}

fn synthetic_base_dir() -> &'static Path {
    Path::new(".")
}

fn safe_fix_rule_diagnostics(content: &str, cfg: &YamlLintConfig) -> Vec<LintProblem> {
    lint_str(content, synthetic_path(), cfg, synthetic_base_dir())
        .into_iter()
        .filter(|diag| {
            diag.rule
                .map(|rule| SAFE_FIX_RULES.contains(&rule))
                .unwrap_or(false)
        })
        .collect()
}

fn parse_for_compare(content: &str) -> Option<Vec<YamlOwned>> {
    YamlOwned::load_from_str(content).ok()
}

#[derive(Debug, Clone)]
enum Scalar {
    Plain(String),
    SingleQuoted(String),
    DoubleQuoted(String),
}

#[derive(Debug, Clone)]
enum Node {
    Scalar(Scalar),
    FlowSeq(Vec<Node>, FlowStyle),
    FlowMap(Vec<(Scalar, Node)>, FlowStyle),
}

#[derive(Debug, Clone, Copy)]
struct FlowStyle {
    inner_padding: u8,
    spaces_before_comma: u8,
    spaces_after_comma: u8,
}

#[derive(Debug, Clone)]
struct InlineComment {
    spaces_after_hash: u8,
    text: String,
}

#[derive(Debug, Clone)]
struct BlockEntry {
    key: String,
    value: Node,
    trailing_inline_comment: Option<InlineComment>,
}

#[derive(Debug, Clone, Copy)]
enum NewlineStyle {
    Lf,
    Crlf,
}

#[derive(Debug, Clone)]
struct Document {
    entries: Vec<BlockEntry>,
    newline: NewlineStyle,
    has_final_newline: bool,
}

fn push_spaces(buffer: &mut String, count: u8) {
    for _ in 0..count {
        buffer.push(' ');
    }
}

impl Scalar {
    fn render(&self, buffer: &mut String) {
        match self {
            Self::Plain(text) => buffer.push_str(text),
            Self::SingleQuoted(text) => {
                buffer.push('\'');
                for ch in text.chars() {
                    if ch == '\'' {
                        buffer.push_str("''");
                    } else {
                        buffer.push(ch);
                    }
                }
                buffer.push('\'');
            }
            Self::DoubleQuoted(text) => {
                buffer.push('"');
                for ch in text.chars() {
                    match ch {
                        '"' => buffer.push_str("\\\""),
                        '\\' => buffer.push_str("\\\\"),
                        _ => buffer.push(ch),
                    }
                }
                buffer.push('"');
            }
        }
    }
}

impl Node {
    fn render(&self, buffer: &mut String) {
        match self {
            Self::Scalar(scalar) => scalar.render(buffer),
            Self::FlowSeq(items, style) => {
                buffer.push('[');
                push_spaces(buffer, style.inner_padding);
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        push_spaces(buffer, style.spaces_before_comma);
                        buffer.push(',');
                        push_spaces(buffer, style.spaces_after_comma);
                    }
                    item.render(buffer);
                }
                push_spaces(buffer, style.inner_padding);
                buffer.push(']');
            }
            Self::FlowMap(pairs, style) => {
                buffer.push('{');
                push_spaces(buffer, style.inner_padding);
                for (index, (key, value)) in pairs.iter().enumerate() {
                    if index > 0 {
                        push_spaces(buffer, style.spaces_before_comma);
                        buffer.push(',');
                        push_spaces(buffer, style.spaces_after_comma);
                    }
                    key.render(buffer);
                    buffer.push_str(": ");
                    value.render(buffer);
                }
                push_spaces(buffer, style.inner_padding);
                buffer.push('}');
            }
        }
    }
}

impl Document {
    fn render(&self) -> String {
        let mut buffer = String::new();
        let line_terminator = match self.newline {
            NewlineStyle::Lf => "\n",
            NewlineStyle::Crlf => "\r\n",
        };
        for (index, entry) in self.entries.iter().enumerate() {
            if index > 0 {
                buffer.push_str(line_terminator);
            }
            buffer.push_str(&entry.key);
            buffer.push_str(": ");
            entry.value.render(&mut buffer);
            if let Some(comment) = &entry.trailing_inline_comment {
                buffer.push_str("  #");
                push_spaces(&mut buffer, comment.spaces_after_hash);
                buffer.push_str(&comment.text);
            }
        }
        if self.has_final_newline {
            buffer.push_str(line_terminator);
        }
        buffer
    }
}

fn arb_plain_identifier() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,6}".prop_map(|value| value)
}

fn arb_quoted_payload() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            Just('a'),
            Just('b'),
            Just('1'),
            Just(' '),
            Just('#'),
            Just(','),
            Just('{'),
            Just('}'),
            Just('['),
            Just(']'),
        ],
        0usize..=6,
    )
    .prop_map(|chars| chars.into_iter().collect())
}

fn arb_scalar() -> impl Strategy<Value = Scalar> {
    prop_oneof![
        arb_plain_identifier().prop_map(Scalar::Plain),
        arb_quoted_payload().prop_map(Scalar::SingleQuoted),
        arb_quoted_payload().prop_map(Scalar::DoubleQuoted),
    ]
}

fn arb_flow_style() -> impl Strategy<Value = FlowStyle> {
    (0u8..=2, 0u8..=2, 0u8..=2).prop_map(
        |(inner_padding, spaces_before_comma, spaces_after_comma)| FlowStyle {
            inner_padding,
            spaces_before_comma,
            spaces_after_comma,
        },
    )
}

fn arb_node() -> impl Strategy<Value = Node> {
    let leaf = arb_scalar().prop_map(Node::Scalar);
    leaf.prop_recursive(2, 16, 4, |inner| {
        prop_oneof![
            (
                prop::collection::vec(inner.clone(), 0..=4),
                arb_flow_style()
            )
                .prop_map(|(items, style)| Node::FlowSeq(items, style)),
            (
                prop::collection::vec((arb_scalar(), inner), 0..=4),
                arb_flow_style(),
            )
                .prop_map(|(pairs, style)| Node::FlowMap(pairs, style)),
        ]
    })
}

fn arb_inline_comment() -> impl Strategy<Value = InlineComment> {
    (0u8..=2, "[a-z][a-z0-9 ]{0,8}").prop_map(|(spaces_after_hash, text)| {
        InlineComment {
            spaces_after_hash,
            text,
        }
    })
}

fn arb_block_entry() -> impl Strategy<Value = BlockEntry> {
    (
        arb_plain_identifier(),
        arb_node(),
        prop::option::of(arb_inline_comment()),
    )
        .prop_map(|(key, value, trailing_inline_comment)| BlockEntry {
            key,
            value,
            trailing_inline_comment,
        })
}

fn arb_document() -> impl Strategy<Value = Document> {
    (
        prop::collection::vec(arb_block_entry(), 1..=4),
        prop_oneof![Just(NewlineStyle::Lf), Just(NewlineStyle::Crlf)],
        any::<bool>(),
    )
        .prop_map(|(entries, newline, has_final_newline)| Document {
            entries,
            newline,
            has_final_newline,
        })
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: Some(Box::new(FileFailurePersistence::WithSource(
            "proptest-regressions",
        ))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn safe_fix_is_idempotent(document in arb_document()) {
        let input = document.render();
        let cfg = safe_fix_config();
        let once = apply_safe_fixes(&input, &cfg, synthetic_path(), synthetic_base_dir());
        let twice = apply_safe_fixes(&once, &cfg, synthetic_path(), synthetic_base_dir());
        prop_assert_eq!(
            &once,
            &twice,
            "applying safe fixes is not idempotent for input {:?}; once -> {:?}; twice -> {:?}",
            input,
            once,
            twice
        );
    }

    #[test]
    fn safe_fix_leaves_no_safe_fix_rule_diagnostics(document in arb_document()) {
        let input = document.render();
        let cfg = safe_fix_config();
        let fixed = apply_safe_fixes(&input, &cfg, synthetic_path(), synthetic_base_dir());
        if parse_for_compare(&fixed).is_none() {
            return Ok(());
        }
        let remaining = safe_fix_rule_diagnostics(&fixed, &cfg);
        prop_assert!(
            remaining.is_empty(),
            "safe-fix-rule diagnostics survived fix for input {:?}; fixed {:?}; diagnostics {:?}",
            input,
            fixed,
            remaining
        );
    }

    #[test]
    fn safe_fix_preserves_parsed_value(document in arb_document()) {
        let input = document.render();
        let Some(before) = parse_for_compare(&input) else {
            return Ok(());
        };
        let cfg = safe_fix_config();
        let fixed = apply_safe_fixes(&input, &cfg, synthetic_path(), synthetic_base_dir());
        let after = parse_for_compare(&fixed).ok_or_else(|| {
            TestCaseError::fail(format!(
                "safe fix broke a previously-parseable document; input {input:?}; fixed {fixed:?}"
            ))
        })?;
        prop_assert_eq!(
            &before,
            &after,
            "safe fix changed parsed YAML value; input {:?}; fixed {:?}",
            input,
            fixed
        );
    }
}

#[test]
fn safe_fix_properties_hold_for_known_dirty_input() {
    let plain = |name: &str| Node::Scalar(Scalar::Plain(name.to_string()));
    let dirty_flow_seq = Document {
        entries: vec![BlockEntry {
            key: "items".to_string(),
            value: Node::FlowSeq(
                vec![plain("a"), plain("b")],
                FlowStyle {
                    inner_padding: 1,
                    spaces_before_comma: 1,
                    spaces_after_comma: 2,
                },
            ),
            trailing_inline_comment: Some(InlineComment {
                spaces_after_hash: 0,
                text: "trailing".to_string(),
            }),
        }],
        newline: NewlineStyle::Crlf,
        has_final_newline: false,
    };
    let input = dirty_flow_seq.render();
    let cfg = safe_fix_config();
    let before = parse_for_compare(&input).expect("known dirty input must parse");
    let fixed = apply_safe_fixes(&input, &cfg, synthetic_path(), synthetic_base_dir());
    assert_ne!(
        input, fixed,
        "renderer must emit inputs that exercise safe fixers; input={input:?} fixed={fixed:?}"
    );
    let after =
        parse_for_compare(&fixed).expect("safe fix must keep known input parseable");
    assert_eq!(before, after, "safe fix must preserve parsed value");
    let remaining = safe_fix_rule_diagnostics(&fixed, &cfg);
    assert!(
        remaining.is_empty(),
        "safe-fix-rule diagnostics must clear after fix on known input: {remaining:?}"
    );
    let twice = apply_safe_fixes(&fixed, &cfg, synthetic_path(), synthetic_base_dir());
    assert_eq!(fixed, twice, "safe fix must be idempotent on known input");
}
