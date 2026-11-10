//! Adds what the safe-fix generator never emits to the stacked generator's mappings:
//! anchors, aliases to anchors defined earlier, core, local and non-specific tags on
//! scalars and flow collections, quoted scalars whose content needs escaping (the quote
//! ladder's top rung), quoted block mapping keys, which the ladder rewrites too, and alias
//! keys and anchored or tagged empty keys, whose space before `:` is required.

use proptest::prelude::*;

use super::ast::{
    BlockEntry, FlowStyle, MultilineLine, MultilineQuotedSpec, Node, Scalar, SeqBody,
};
use super::stack::{StackedDocument, arb_stacked_document};
use super::strategy::arb_quoted_payload;

const TAGS: [&str; 3] = ["!!str", "!local", "!"];

const ESCAPED: [(bool, &str); 18] = [
    (false, "/health?ready=true"),
    (false, "secret"),
    (true, "keep"),
    (true, "line\nbreak"),
    (true, "tab\there"),
    (true, "back\\slash"),
    (true, "say \"hi\""),
    (false, "it's"),
    (true, "it's: x"),
    (false, "say \"hi\": x"),
    (false, "it's \\ x: y"),
    (true, "it's \"x\": y"),
    (false, "it's \"x\": y"),
    (true, "café \"quoted\""),
    (true, "café \"quoted\": value"),
    (false, "café's: value"),
    (true, "café [value]"),
    (true, "🦀 é 世 \"quoted\""),
];

const QUOTED_KEYS: [&str; 8] = [
    "'quoted'",
    "\"double\"",
    "'a: b'",
    "'no'",
    "'011'",
    "\"tab\\there\"",
    "'it''s'",
    "'#hash'",
];

#[derive(Debug, Clone, Copy)]
pub enum Property {
    None,
    Anchor,
    Tag(usize),
    AnchorAndTag(usize),
    Alias,
    Escaped(usize),
    QuotedKey(usize),
    GuardedKey(usize),
    AliasKey,
    /// A tag from `TAGS`, or an anchor past its end, on an empty key.
    EmptyKey(usize),
    OmittedKey,
    AnchoredOmittedKey,
    GapInteractions,
}

fn arb_property() -> impl Strategy<Value = Property> {
    prop_oneof![
        4 => Just(Property::None),
        2 => Just(Property::Anchor),
        1 => (0..TAGS.len()).prop_map(Property::Tag),
        1 => (0..TAGS.len()).prop_map(Property::AnchorAndTag),
        2 => Just(Property::Alias),
        2 => (0..ESCAPED.len()).prop_map(Property::Escaped),
        2 => (0..QUOTED_KEYS.len()).prop_map(Property::QuotedKey),
        1 => (1021..=1023usize).prop_map(Property::GuardedKey),
        1 => Just(Property::AliasKey),
        1 => (0..=TAGS.len()).prop_map(Property::EmptyKey),
        1 => Just(Property::OmittedKey),
        1 => Just(Property::AnchoredOmittedKey),
        1 => Just(Property::GapInteractions),
    ]
}

struct Decorator<'a> {
    properties: &'a [Property],
    next: usize,
    anchors: usize,
}

impl Decorator<'_> {
    fn decorate(&mut self, entries: &mut [BlockEntry]) {
        for entry in entries {
            if matches!(
                self.properties[self.next % self.properties.len()],
                Property::AnchoredOmittedKey
            ) {
                self.next += 1;
                entry.key.clear();
                entry.colon.explicit = None;
                entry.value =
                    Node::Scalar(Scalar::Plain(format!("{}[x, y]", self.anchor())));
                continue;
            }
            let key = match self.properties[self.next % self.properties.len()] {
                Property::QuotedKey(index) => Some(QUOTED_KEYS[index].to_string()),
                Property::GuardedKey(1021) => Some("[a, {b: c}]".to_owned()),
                Property::GuardedKey(length) => Some("é".repeat(length)),
                Property::AliasKey if self.anchors > 0 => {
                    Some(format!("*a{} ", self.next % self.anchors))
                }
                Property::OmittedKey => Some(String::new()),
                Property::EmptyKey(index) => Some(
                    TAGS.get(index)
                        .map_or_else(|| self.anchor(), |tag| format!("{tag} ")),
                ),
                _ => None,
            };
            if let Some(key) = key {
                self.next += 1;
                entry.key = key;
            }
            match &mut entry.value {
                Node::BlockMap(nested) => self.decorate(nested),
                Node::BlockSeq(items) => {
                    items
                        .iter_mut()
                        .for_each(|item| self.decorate_item(&mut item.body));
                }
                node => self.decorate_inline(node),
            }
        }
    }

    fn decorate_item(&mut self, body: &mut SeqBody) {
        match body {
            SeqBody::Inline(node) => self.decorate_inline(node),
            SeqBody::TaggedMap(_, nested) | SeqBody::CompactMap(nested) => {
                self.decorate(nested)
            }
            SeqBody::CompactSeq(nodes) => nodes
                .iter_mut()
                .for_each(|(_, node)| self.decorate_inline(node)),
            SeqBody::MultilineQuoted(spec) => self.decorate_multiline(spec),
            _ => {}
        }
    }

    fn decorate_inline(&mut self, node: &mut Node) {
        match node {
            Node::Scalar(scalar) => {
                let property = self.properties[self.next % self.properties.len()];
                self.next += 1;
                *scalar = self.apply(property, scalar);
            }
            Node::FlowSeq(items, _) => {
                items.iter_mut().for_each(|item| self.decorate_inline(item));
                self.decorate_collection(node);
            }
            Node::FlowMap(pairs, _) => {
                for (key, value) in pairs {
                    if matches!(
                        self.properties[self.next % self.properties.len()],
                        Property::OmittedKey
                    ) {
                        *key = Scalar::Plain(String::new());
                        self.next += 1;
                    }
                    self.decorate_inline(value);
                }
                self.decorate_collection(node);
            }
            Node::MultilineFlowSeq(spec) => {
                for (_, scalar) in &mut spec.items {
                    let property = self.properties[self.next % self.properties.len()];
                    self.next += 1;
                    *scalar = self.apply(property, scalar);
                }
            }
            Node::MultilineQuoted(spec) => self.decorate_multiline(spec),
            _ => {}
        }
    }

    fn decorate_multiline(&mut self, spec: &mut MultilineQuotedSpec) {
        for line in &mut spec.lines {
            let property = self.properties[self.next % self.properties.len()];
            self.next += 1;
            if let MultilineLine::Content(text) = line
                && let Property::QuotedKey(index) = property
            {
                *text = format!("#{}", QUOTED_KEYS[index]);
            }
        }
    }

    /// Puts an anchor or tag on flow collection `node`, which then renders as raw text.
    fn decorate_collection(&mut self, node: &mut Node) {
        let mut text = match self.properties[self.next % self.properties.len()] {
            Property::Anchor => self.anchor(),
            Property::Tag(tag) => format!("{} ", TAGS[tag]),
            Property::AnchorAndTag(tag) => format!("{}{} ", self.anchor(), TAGS[tag]),
            _ => return,
        };
        self.next += 1;
        node.render(&mut text);
        *node = Node::Scalar(Scalar::Plain(text));
    }

    fn apply(&mut self, property: Property, scalar: &Scalar) -> Scalar {
        let already_tagged =
            matches!(scalar, Scalar::Plain(text) if text.starts_with('!'));
        let prefix = match property {
            Property::GapInteractions => {
                return Scalar::Plain("{ é :  [ \"é\tvalue\" ,y ] }".into());
            }
            Property::OmittedKey => {
                return Scalar::Plain("[: 1, {: [a, b]}]".into());
            }
            Property::None
            | Property::QuotedKey(_)
            | Property::GuardedKey(_)
            | Property::AliasKey
            | Property::EmptyKey(_)
            | Property::AnchoredOmittedKey => return scalar.clone(),
            Property::Escaped(index) => {
                let (double, payload) = ESCAPED[index];
                let payload = payload.to_string();
                return if double {
                    Scalar::DoubleQuoted(payload)
                } else {
                    Scalar::SingleQuoted(payload)
                };
            }
            _ if already_tagged => return scalar.clone(),
            Property::Alias if self.anchors > 0 => {
                return Scalar::Plain(format!("*a{}", self.next % self.anchors));
            }
            Property::Alias => return scalar.clone(),
            Property::Anchor => self.anchor(),
            Property::Tag(tag) => format!("{} ", TAGS[tag]),
            Property::AnchorAndTag(tag) => format!("{}{} ", self.anchor(), TAGS[tag]),
        };
        let mut text = prefix;
        scalar.render(&mut text);
        Scalar::Plain(text)
    }

    fn anchor(&mut self) -> String {
        self.anchors += 1;
        format!("&a{} ", self.anchors - 1)
    }
}

pub fn arb_document_with_properties() -> impl Strategy<Value = StackedDocument> {
    (
        arb_stacked_document(),
        prop::collection::vec(arb_property(), 1..=8),
        0usize..=8,
        prop::option::weighted(0.2, any::<bool>()),
    )
        .prop_map(|(mut stacked, properties, siblings, explicit_flow)| {
            if let Some(first) = stacked.document.entries.first_mut()
                && let Some(inner_first) = explicit_flow
            {
                let style = FlowStyle {
                    trailing_comma: false,
                    inner_padding: 0,
                    spaces_before_comma: 0,
                    spaces_after_comma: 1,
                    spaces_before_colon: 0,
                    spaces_after_colon: 1,
                };
                let scalar = |text| Node::Scalar(Scalar::Plain(text));
                let inner = Node::FlowSeq(
                    vec![scalar("a".to_owned()), scalar("b".to_owned())],
                    style,
                );
                let other = Node::FlowMap(
                    vec![(
                        Scalar::SingleQuoted(String::new()),
                        scalar("c".repeat(100)),
                    )],
                    style,
                );
                first.colon.explicit = Some(0);
                first.value = Node::FlowSeq(
                    if inner_first {
                        vec![inner, other]
                    } else {
                        vec![other, inner]
                    },
                    style,
                );
            }
            if let (Some(entry), Some(decoration)) = (
                stacked.document.entries.first().cloned(),
                stacked.decorations.first().cloned(),
            ) {
                for index in 0..siblings {
                    let mut sibling = entry.clone();
                    sibling.key = format!("collection_{index}");
                    stacked.document.entries.push(sibling);
                    stacked.decorations.push(decoration.clone());
                }
            }
            Decorator {
                properties: &properties,
                next: 0,
                anchors: 0,
            }
            .decorate(&mut stacked.document.entries);
            stacked
        })
}

#[allow(
    dead_code,
    reason = "the benchmark shares only the decorated AST generator"
)]
pub fn arb_nested_sequence_tail() -> impl Strategy<Value = String> {
    (
        1usize..=4,
        0usize..=8,
        prop::sample::select(vec!["", "...\n", "---\nnext: value\n"]),
        prop::sample::select(vec!["\n", "\r\n", "\r"]),
    )
        .prop_map(|(width, comment, marker, newline)| {
            format!(
                "links:\n{}- nested:\n{}- value\n{}# tail\n{marker}",
                " ".repeat(width),
                " ".repeat(width + 2),
                " ".repeat(comment),
            )
            .replace('\n', newline)
        })
}

#[allow(
    dead_code,
    reason = "the benchmark shares only the decorated AST generator"
)]
pub fn arb_quoted_document() -> impl Strategy<Value = String> {
    (
        prop_oneof![4 => arb_quoted_payload(), 1 => prop::sample::select(vec![
            "---",
            "...",
            "--- x",
            "... # c",
            "---x",
            "...x",
            "---\u{a0}x",
            "...\u{2003}x",
            "---\u{feff}x",
            "first ---\u{feff}x",
        ]).prop_map(str::to_owned)],
        prop::sample::select(vec![
            ("", "\n"),
            ("- ", "\n"),
            ("key: ", "\n"),
            ("[", "]\n"),
            ("[", "] # tail\n"),
            ("key:\n  [", "] # tail\n"),
            ("-\n  &col {k: ", "}\n"),
            ("- k: [", "] # tail\n"),
            ("key: [a: ", "]\n"),
            ("items: [", ", 'one\n  two']\n"),
            ("a: {b: ", "}\n"),
            ("{k: ", "} # tail\n"),
            ("", ": x\n"),
        ]),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(|(value, (prefix, suffix), double, multiline)| {
            let scalar = if double {
                Scalar::DoubleQuoted(value)
            } else {
                Scalar::SingleQuoted(value)
            };
            let mut quoted = String::new();
            scalar.render(&mut quoted);
            if multiline && suffix != ": x\n" {
                quoted.insert_str(1, "first\n    ");
            }
            format!("{prefix}{quoted}{suffix}")
        })
}

#[allow(
    dead_code,
    reason = "the benchmark shares only the decorated AST generator"
)]
pub fn arb_whitespace_block_scalar() -> impl Strategy<Value = String> {
    let ordinary = (1usize..=4).prop_flat_map(|indent| {
        (
            prop::sample::select(vec!['|', '>']),
            prop::sample::select(vec!["", "-", "+"]),
            any::<bool>(),
            prop::collection::vec((0usize..=3, "[a-z]{1,6}( [a-z]{1,6})?"), 0..=4),
            prop::collection::vec((0usize..=indent + 4, 0usize..=4), 1..=4),
            0usize..=4,
            prop::sample::select(vec!["\n", "\r\n", "\r"]),
            any::<bool>(),
            any::<bool>(),
        )
            .prop_map(
                move |(
                    style,
                    chomp,
                    explicit,
                    body,
                    whitespace,
                    trailing,
                    ending,
                    terminated,
                    conversion,
                )| {
                    let indicator = if explicit {
                        indent.to_string()
                    } else {
                        String::new()
                    };
                    let text = if conversion {
                        "one two three four five six seven eight nine ten"
                    } else {
                        "a"
                    };
                    let mut input = format!(
                        "{}a: {style}{indicator}{chomp}\n{}{text}\n",
                        if conversion { "first: [a,b]\n" } else { "" },
                        " ".repeat(indent),
                    );
                    for (extra, text) in body {
                        input.push_str(&format!(
                            "{}{text}\n",
                            " ".repeat(indent + extra)
                        ));
                    }
                    for (spaces, blanks) in whitespace {
                        input.push_str(&"\n".repeat(blanks));
                        input.push_str(&" ".repeat(spaces));
                        input.push('\n');
                    }
                    input.push_str(&"\n".repeat(trailing));
                    if !terminated {
                        input.pop();
                    }
                    input.replace('\n', ending)
                },
            )
    });
    let movable_blank = (
        prop::sample::select(vec!['|', '>']),
        prop::sample::select(vec!["", "-", "+"]),
        7usize..=9,
        "[a-z]{1,8}",
        prop::sample::select(vec!["\n", "\r\n", "\r"]),
    ).prop_map(|(style, chomp, spaces, word, ending)| {
        format!("root:\n  - a: {style}2{chomp}\n{}\n    b: x\nother: ' alpha beta {word} gamma delta'\n", " ".repeat(spaces)).replace('\n', ending)
    });
    prop_oneof![3 => ordinary, 1 => movable_blank]
}

pub fn arb_document_start_removal() -> impl Strategy<Value = String> {
    (
        prop::sample::select(vec![
            "---\nvalue\n",
            "---\n\nvalue\n",
            "--- |\n  value\n",
            "--- !tag value\n",
            "--- &a [value, *a]\n",
            "--- # comment\na: value\n",
            "---\na: value\n---\nb: 2\n",
            "---\n---\na: value\n",
            "%YAML 1.2\n---\na: value\n",
            "...\n---\na: value\n",
        ]),
        "[a-zé]{1,8}",
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(|(shape, value, bom, crlf)| {
            let input = format!(
                "{}{}",
                if bom { "\u{feff}" } else { "" },
                shape.replace("value", &value)
            );
            if crlf {
                input.replace('\n', "\r\n")
            } else {
                input
            }
        })
}
