//! Adds what the safe-fix generator never emits to the stacked generator's mapping
//! values: anchors, aliases to anchors defined earlier, core, local and non-specific
//! tags, and quoted scalars whose content needs escaping, the quote ladder's top rung.

use proptest::prelude::*;

use super::ast::{BlockEntry, Node, Scalar};
use super::stack::{StackedDocument, arb_stacked_document};

const TAGS: [&str; 3] = ["!!str", "!local", "!"];

const ESCAPED: [(bool, &str); 5] = [
    (true, "line\nbreak"),
    (true, "tab\there"),
    (true, "back\\slash"),
    (true, "say \"hi\""),
    (false, "it's"),
];

#[derive(Debug, Clone, Copy)]
pub enum Property {
    None,
    Anchor,
    Tag(usize),
    AnchorAndTag(usize),
    Alias,
    Escaped(usize),
}

fn arb_property() -> impl Strategy<Value = Property> {
    prop_oneof![
        4 => Just(Property::None),
        2 => Just(Property::Anchor),
        1 => (0..TAGS.len()).prop_map(Property::Tag),
        1 => (0..TAGS.len()).prop_map(Property::AnchorAndTag),
        2 => Just(Property::Alias),
        2 => (0..ESCAPED.len()).prop_map(Property::Escaped),
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
            match &mut entry.value {
                Node::BlockMap(nested) => self.decorate(nested),
                node => self.decorate_inline(node),
            }
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
            Property::None => return scalar.clone(),
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
