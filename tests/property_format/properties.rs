//! Adds what the safe-fix generator never emits to the stacked generator's mappings:
//! anchors, aliases to anchors defined earlier, core, local and non-specific tags, quoted
//! scalars whose content needs escaping (the quote ladder's top rung), quoted block
//! mapping keys, which the ladder rewrites too, and alias keys and anchored or tagged empty
//! keys, whose space before `:` is required.

use proptest::prelude::*;

use super::ast::{BlockEntry, Node, Scalar, SeqBody};
use super::stack::{StackedDocument, arb_stacked_document};

const TAGS: [&str; 3] = ["!!str", "!local", "!"];

const ESCAPED: [(bool, &str); 5] = [
    (true, "line\nbreak"),
    (true, "tab\there"),
    (true, "back\\slash"),
    (true, "say \"hi\""),
    (false, "it's"),
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
    AliasKey,
    /// A tag from `TAGS`, or an anchor past its end, on an empty key.
    EmptyKey(usize),
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
        1 => Just(Property::AliasKey),
        1 => (0..=TAGS.len()).prop_map(Property::EmptyKey),
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
            let key = match self.properties[self.next % self.properties.len()] {
                Property::QuotedKey(index) => Some(QUOTED_KEYS[index].to_string()),
                Property::AliasKey if self.anchors > 0 => {
                    Some(format!("*a{} ", self.next % self.anchors))
                }
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
            SeqBody::TaggedMap(nested) | SeqBody::CompactMap(nested) => {
                self.decorate(nested)
            }
            SeqBody::CompactSeq(nodes) => nodes
                .iter_mut()
                .for_each(|(_, node)| self.decorate_inline(node)),
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
                items.iter_mut().for_each(|item| self.decorate_inline(item))
            }
            Node::FlowMap(pairs, _) => {
                pairs
                    .iter_mut()
                    .for_each(|(_, value)| self.decorate_inline(value));
            }
            _ => {}
        }
    }

    fn apply(&mut self, property: Property, scalar: &Scalar) -> Scalar {
        let already_tagged =
            matches!(scalar, Scalar::Plain(text) if text.starts_with('!'));
        let prefix = match property {
            Property::None
            | Property::QuotedKey(_)
            | Property::AliasKey
            | Property::EmptyKey(_) => return scalar.clone(),
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
    )
        .prop_map(|(mut stacked, properties)| {
            Decorator {
                properties: &properties,
                next: 0,
                anchors: 0,
            }
            .decorate(&mut stacked.document.entries);
            stacked
        })
}
