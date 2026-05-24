use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use ryl::config::{Overrides, YamlLintConfig, discover_config};
use ryl::fix::apply_safe_fixes;
use ryl::lint::{LintProblem, lint_str};
use saphyr::{LoadableYamlNode, YamlOwned};
use tempfile::TempDir;

const COMMON_SAFE_FIX_RULES_YAML: &str = "rules:
  new-lines: enable
  comments: enable
  comments-indentation: enable
  commas: enable
  braces: enable
  brackets: enable
  new-line-at-end-of-file: enable
  trailing-spaces: enable
  document-start: enable
  document-end: enable
  empty-lines: enable
";

const QUOTED_STRINGS_VARIANTS: &[(&str, &str)] = &[
    ("yamllint-default", "  quoted-strings: enable\n"),
    (
        "best-practice",
        "  quoted-strings:
    quote-type: single
    required: only-when-needed
",
    ),
    (
        "strict-single",
        "  quoted-strings:
    quote-type: single
    required: true
",
    ),
    (
        "strict-double",
        "  quoted-strings:
    quote-type: double
    required: true
",
    ),
    (
        "consistent",
        "  quoted-strings:
    quote-type: consistent
    required: true
",
    ),
];

const SAFE_FIX_RULES: &[&str] = &[
    "new-lines",
    "comments",
    "comments-indentation",
    "commas",
    "braces",
    "brackets",
    "new-line-at-end-of-file",
    "quoted-strings",
    "trailing-spaces",
    "document-start",
    "document-end",
    "empty-lines",
];

const BEST_PRACTICE_TOML: &str = "[rules]
new-lines = 'enable'
comments = 'enable'
comments-indentation = 'enable'
commas = 'enable'
braces = 'enable'
brackets = 'enable'
new-line-at-end-of-file = 'enable'
trailing-spaces = 'enable'
document-start = 'enable'
document-end = 'enable'
empty-lines = 'enable'

[rules.quoted-strings]
quote-type = 'single'
required = 'only-when-needed'
allow-double-quotes-for-escaping = true
";

struct PreparedConfig {
    name: &'static str,
    cfg: YamlLintConfig,
    // Holds the tempdir containing the .ryl.toml that `discover_config` was
    // given; kept alive so the path embedded in `cfg` (used by per-file
    // ignore matching) stays valid for the lifetime of the LazyLock.
    _backing: Option<TempDir>,
}

static SAFE_FIX_CONFIGS: LazyLock<Vec<PreparedConfig>> = LazyLock::new(|| {
    let mut configs: Vec<PreparedConfig> = QUOTED_STRINGS_VARIANTS
        .iter()
        .map(|(name, suffix)| {
            let yaml = format!("{COMMON_SAFE_FIX_RULES_YAML}{suffix}");
            let cfg = YamlLintConfig::from_yaml_str(&yaml)
                .expect("named safe-fix config must parse");
            PreparedConfig {
                name,
                cfg,
                _backing: None,
            }
        })
        .collect();

    let dir = TempDir::new().expect("create tempdir for TOML config");
    let toml_path = dir.path().join(".ryl.toml");
    fs::write(&toml_path, BEST_PRACTICE_TOML).expect("write TOML config");
    let overrides = Overrides {
        config_file: Some(toml_path),
        config_data: None,
    };
    let ctx = discover_config(&[], &overrides)
        .expect("TOML-backed best-practice config must load");
    configs.push(PreparedConfig {
        name: "best-practice-toml",
        cfg: ctx.config,
        _backing: Some(dir),
    });

    configs
});

fn safe_fix_configs() -> &'static [PreparedConfig] {
    &SAFE_FIX_CONFIGS
}

fn named_config(name: &str) -> &'static YamlLintConfig {
    &safe_fix_configs()
        .iter()
        .find(|prepared| prepared.name == name)
        .unwrap_or_else(|| panic!("unknown safe-fix config '{name}'"))
        .cfg
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
    BlockScalar(BlockScalarSpec),
    MultilineQuoted(MultilineQuotedSpec),
}

#[derive(Debug, Clone)]
struct BlockScalarSpec {
    style: char,
    chomp: Option<char>,
    explicit_indent: Option<u8>,
    body: Vec<BlockBodyLine>,
}

#[derive(Debug, Clone)]
enum BlockBodyLine {
    Content { text: String, trailing_ws: u8 },
    Blank,
}

#[derive(Debug, Clone)]
struct MultilineQuotedSpec {
    style: MultilineQuoteStyle,
    lines: Vec<MultilineLine>,
}

#[derive(Debug, Clone, Copy)]
enum MultilineQuoteStyle {
    Single,
    Double,
}

#[derive(Debug, Clone)]
enum MultilineLine {
    Content(String),
    Blank,
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
                        '\n' => buffer.push_str("\\n"),
                        '\t' => buffer.push_str("\\t"),
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
            Self::BlockScalar(_) | Self::MultilineQuoted(_) => {
                unreachable!("multi-line nodes must be rendered via BlockEntry");
            }
        }
    }
}

impl BlockScalarSpec {
    fn render(&self, buffer: &mut String, line_term: &str) {
        buffer.push(self.style);
        if let Some(n) = self.explicit_indent {
            buffer.push((b'0' + n) as char);
        }
        if let Some(c) = self.chomp {
            buffer.push(c);
        }
        let indent = self.body_indent();
        for line in &self.body {
            buffer.push_str(line_term);
            if let BlockBodyLine::Content { text, trailing_ws } = line {
                for _ in 0..indent {
                    buffer.push(' ');
                }
                buffer.push_str(text);
                for _ in 0..*trailing_ws {
                    buffer.push(' ');
                }
            }
        }
    }

    fn body_indent(&self) -> usize {
        self.explicit_indent.map_or(2, usize::from)
    }
}

impl MultilineQuotedSpec {
    fn render(&self, buffer: &mut String, line_term: &str) {
        let quote = match self.style {
            MultilineQuoteStyle::Single => '\'',
            MultilineQuoteStyle::Double => '"',
        };
        buffer.push(quote);
        for (index, line) in self.lines.iter().enumerate() {
            if index > 0 {
                buffer.push_str(line_term);
            }
            if let MultilineLine::Content(text) = line {
                for ch in text.chars() {
                    match (self.style, ch) {
                        (MultilineQuoteStyle::Single, '\'') => buffer.push_str("''"),
                        (MultilineQuoteStyle::Double, '"') => buffer.push_str("\\\""),
                        (MultilineQuoteStyle::Double, '\\') => buffer.push_str("\\\\"),
                        _ => buffer.push(ch),
                    }
                }
            }
        }
        buffer.push(quote);
    }
}

impl Document {
    fn has_partial_safe_fix_residue(&self) -> bool {
        self.entries
            .iter()
            .any(BlockEntry::has_partial_safe_fix_residue)
    }
}

impl BlockEntry {
    fn has_partial_safe_fix_residue(&self) -> bool {
        match &self.value {
            Node::BlockScalar(spec) => spec.body.iter().any(|line| match line {
                BlockBodyLine::Content { trailing_ws, .. } => *trailing_ws > 0,
                BlockBodyLine::Blank => true,
            }),
            Node::MultilineQuoted(spec) => spec
                .lines
                .iter()
                .any(|line| matches!(line, MultilineLine::Blank)),
            _ => false,
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
            entry.render(&mut buffer, line_terminator);
        }
        if self.has_final_newline {
            buffer.push_str(line_terminator);
        }
        buffer
    }
}

impl BlockEntry {
    fn render(&self, buffer: &mut String, line_term: &str) {
        buffer.push_str(&self.key);
        buffer.push_str(": ");
        let allow_trailing_comment = match &self.value {
            Node::Scalar(_) | Node::FlowSeq(_, _) | Node::FlowMap(_, _) => {
                self.value.render(buffer);
                true
            }
            Node::BlockScalar(spec) => {
                spec.render(buffer, line_term);
                false
            }
            Node::MultilineQuoted(spec) => {
                spec.render(buffer, line_term);
                false
            }
        };
        if allow_trailing_comment && let Some(comment) = &self.trailing_inline_comment {
            buffer.push_str("  #");
            push_spaces(buffer, comment.spaces_after_hash);
            buffer.push_str(&comment.text);
        }
    }
}

fn arb_plain_identifier() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,6}".prop_map(|value| value)
}

fn arb_single_quoted_payload() -> impl Strategy<Value = String> {
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
            Just('*'),
            Just('?'),
            Just('&'),
            Just('!'),
            Just(':'),
        ],
        0usize..=6,
    )
    .prop_map(|chars| chars.into_iter().collect())
}

fn arb_double_quoted_payload() -> impl Strategy<Value = String> {
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
            Just('*'),
            Just('?'),
            Just('&'),
            Just('!'),
            Just(':'),
        ],
        0usize..=6,
    )
    .prop_map(|chars| chars.into_iter().collect())
}

fn arb_scalar() -> impl Strategy<Value = Scalar> {
    prop_oneof![
        arb_plain_identifier().prop_map(Scalar::Plain),
        arb_single_quoted_payload().prop_map(Scalar::SingleQuoted),
        arb_double_quoted_payload().prop_map(Scalar::DoubleQuoted),
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

fn arb_top_level_node() -> impl Strategy<Value = Node> {
    prop_oneof![
        10 => arb_node(),
        3 => arb_block_scalar_spec().prop_map(Node::BlockScalar),
        3 => arb_multiline_quoted_spec().prop_map(Node::MultilineQuoted),
    ]
}

fn arb_block_scalar_spec() -> impl Strategy<Value = BlockScalarSpec> {
    (
        prop_oneof![Just('|'), Just('>')],
        prop::option::of(prop_oneof![Just('-'), Just('+')]),
        prop::option::of(2u8..=4u8),
        arb_block_body_content(),
        prop::collection::vec(arb_block_body_line(), 0..=3),
    )
        .prop_map(|(style, chomp, explicit_indent, first, rest)| {
            let mut body = vec![first];
            body.extend(rest);
            BlockScalarSpec {
                style,
                chomp,
                explicit_indent,
                body,
            }
        })
}

fn arb_block_body_content() -> impl Strategy<Value = BlockBodyLine> {
    ("[a-z][a-z0-9]{0,6}", 0u8..=3)
        .prop_map(|(text, trailing_ws)| BlockBodyLine::Content { text, trailing_ws })
}

fn arb_block_body_line() -> impl Strategy<Value = BlockBodyLine> {
    prop_oneof![
        3 => arb_block_body_content(),
        1 => Just(BlockBodyLine::Blank),
    ]
}

fn arb_multiline_quoted_spec() -> impl Strategy<Value = MultilineQuotedSpec> {
    (
        prop_oneof![
            Just(MultilineQuoteStyle::Single),
            Just(MultilineQuoteStyle::Double),
        ],
        prop::collection::vec(arb_multiline_line(), 1..=4),
    )
        .prop_map(|(style, lines)| MultilineQuotedSpec { style, lines })
}

fn arb_multiline_line() -> impl Strategy<Value = MultilineLine> {
    prop_oneof![
        3 => "[a-z][a-z0-9]{0,5}".prop_map(MultilineLine::Content),
        1 => Just(MultilineLine::Blank),
    ]
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
        arb_top_level_node(),
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
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/proptest-regressions/property_safe_fix.txt",
        ))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn safe_fix_is_idempotent(document in arb_document()) {
        let input = document.render();
        for prepared in safe_fix_configs() {
            let cfg_name = prepared.name;
            let cfg = &prepared.cfg;
            let once =
                apply_safe_fixes(&input, cfg, synthetic_path(), synthetic_base_dir());
            let twice =
                apply_safe_fixes(&once, cfg, synthetic_path(), synthetic_base_dir());
            prop_assert_eq!(
                &once,
                &twice,
                "applying safe fixes is not idempotent under config '{}' for input {:?}; once -> {:?}; twice -> {:?}",
                cfg_name,
                input,
                once,
                twice
            );
        }
    }

    #[test]
    fn safe_fix_leaves_no_safe_fix_rule_diagnostics(document in arb_document()) {
        if document.has_partial_safe_fix_residue() {
            return Ok(());
        }
        let input = document.render();
        for prepared in safe_fix_configs() {
            let cfg_name = prepared.name;
            let cfg = &prepared.cfg;
            let fixed =
                apply_safe_fixes(&input, cfg, synthetic_path(), synthetic_base_dir());
            if parse_for_compare(&fixed).is_none() {
                continue;
            }
            let remaining = safe_fix_rule_diagnostics(&fixed, cfg);
            prop_assert!(
                remaining.is_empty(),
                "safe-fix-rule diagnostics survived fix under config '{}' for input {:?}; fixed {:?}; diagnostics {:?}",
                cfg_name,
                input,
                fixed,
                remaining
            );
        }
    }

    #[test]
    fn safe_fix_preserves_parsed_value(document in arb_document()) {
        let input = document.render();
        let Some(before) = parse_for_compare(&input) else {
            return Ok(());
        };
        for prepared in safe_fix_configs() {
            let cfg_name = prepared.name;
            let cfg = &prepared.cfg;
            let fixed =
                apply_safe_fixes(&input, cfg, synthetic_path(), synthetic_base_dir());
            let after = parse_for_compare(&fixed).ok_or_else(|| {
                TestCaseError::fail(format!(
                    "safe fix broke a previously-parseable document under config '{cfg_name}'; input {input:?}; fixed {fixed:?}"
                ))
            })?;
            prop_assert_eq!(
                &before,
                &after,
                "safe fix changed parsed YAML value under config '{}'; input {:?}; fixed {:?}",
                cfg_name,
                input,
                fixed
            );
        }
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
    let before = parse_for_compare(&input).expect("known dirty input must parse");
    for prepared in safe_fix_configs() {
        let cfg_name = prepared.name;
        let cfg = &prepared.cfg;
        let fixed =
            apply_safe_fixes(&input, cfg, synthetic_path(), synthetic_base_dir());
        assert_ne!(
            input, fixed,
            "renderer must emit inputs that exercise safe fixers under config '{cfg_name}'; input={input:?} fixed={fixed:?}"
        );
        let after = parse_for_compare(&fixed).unwrap_or_else(|| {
            panic!(
                "safe fix broke parseable input under config '{cfg_name}': {fixed:?}"
            )
        });
        assert_eq!(
            before, after,
            "safe fix must preserve parsed value under config '{cfg_name}'"
        );
        let remaining = safe_fix_rule_diagnostics(&fixed, cfg);
        assert!(
            remaining.is_empty(),
            "safe-fix-rule diagnostics must clear after fix under config '{cfg_name}': {remaining:?}"
        );
        let twice =
            apply_safe_fixes(&fixed, cfg, synthetic_path(), synthetic_base_dir());
        assert_eq!(
            fixed, twice,
            "safe fix must be idempotent under config '{cfg_name}'"
        );
    }
}

#[test]
fn best_practice_retains_quotes_around_yaml_metachars() {
    let input = "schedule: '30 21 * * 0'\n";
    let cfg = named_config("best-practice");
    let before = parse_for_compare(input).expect("input parses");
    let fixed = apply_safe_fixes(input, cfg, synthetic_path(), synthetic_base_dir());
    let after = parse_for_compare(&fixed)
        .expect("best-practice fix must keep cron-like input parseable");
    assert_eq!(before, after, "parse must be preserved: {fixed:?}");
    assert!(
        fixed.contains("'30 21 * * 0'"),
        "best-practice must retain quotes around scalars containing YAML metachars (issue #206): {fixed:?}"
    );
}

#[test]
fn best_practice_does_not_break_parse_for_escape_sequences() {
    let input = "message: \"line1\\nline2\"\n";
    let cfg = named_config("best-practice");
    let before = parse_for_compare(input).expect("input parses");
    let fixed = apply_safe_fixes(input, cfg, synthetic_path(), synthetic_base_dir());
    let after = parse_for_compare(&fixed)
        .expect("best-practice fix must keep escape-sequence input parseable");
    assert_eq!(
        before, after,
        "safe fix must not change parsed value of escape-bearing scalars (issue #184): {fixed:?}"
    );
}

#[test]
fn best_practice_preserves_trailing_comment_when_unquoting() {
    let input = "key: 'value'  # important comment\n";
    let cfg = named_config("best-practice");
    let before = parse_for_compare(input).expect("input parses");
    let fixed = apply_safe_fixes(input, cfg, synthetic_path(), synthetic_base_dir());
    let after = parse_for_compare(&fixed)
        .expect("best-practice fix must keep quoted-with-comment input parseable");
    assert_eq!(before, after, "parse must be preserved: {fixed:?}");
    assert!(
        fixed.contains("# important comment"),
        "trailing comment must survive quote removal (issue #206): {fixed:?}"
    );
}
