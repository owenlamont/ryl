// Vendored from saphyr v0.0.6 (`saphyr/src/loader.rs`).
// Specialized to a single concrete node type (`YamlOwned`) and retargeted at
// granit-parser's event stream. The original is dual-licensed MIT OR
// Apache-2.0; ryl ships under MIT.

use std::borrow::Cow;
use std::collections::BTreeMap;

use granit_parser::{Event, Parser, ScanError, Span, SpannedEventReceiver, Tag};

use crate::yaml_dom::scalar::Scalar;
use crate::yaml_dom::yaml_owned::{MappingOwned, YamlOwned};

/// # Errors
/// Propagates [`ScanError`] when the granit parser rejects the input.
pub fn load_owned_documents(source: &str) -> Result<Vec<YamlOwned>, ScanError> {
    let mut parser = Parser::new_from_str(source);
    let mut loader = Loader::default();
    parser.load(&mut loader, true)?;
    Ok(loader.docs)
}

enum Container {
    Sequence(Vec<YamlOwned>),
    Mapping(MappingOwned, YamlOwned),
}

struct Frame {
    container: Container,
    anchor_id: usize,
    tag: Option<Tag>,
}

#[derive(Default)]
struct Loader {
    docs: Vec<YamlOwned>,
    stack: Vec<Frame>,
    root: Option<YamlOwned>,
    anchor_map: BTreeMap<usize, YamlOwned>,
}

impl<'input> SpannedEventReceiver<'input> for Loader {
    fn on_event(&mut self, ev: Event<'input>, _span: Span) {
        match ev {
            Event::DocumentStart(_)
            | Event::Nothing
            | Event::StreamStart
            | Event::StreamEnd
            | Event::Comment(_, _) => {}
            Event::DocumentEnd => {
                let node = self.root.take().unwrap_or(YamlOwned::BadValue);
                self.docs.push(node);
            }
            Event::SequenceStart(_, aid, tag) => {
                self.stack.push(Frame {
                    container: Container::Sequence(Vec::new()),
                    anchor_id: aid,
                    tag: tag.map(Cow::into_owned),
                });
            }
            Event::MappingStart(_, aid, tag) => {
                self.stack.push(Frame {
                    container: Container::Mapping(
                        MappingOwned::new(),
                        YamlOwned::BadValue,
                    ),
                    anchor_id: aid,
                    tag: tag.map(Cow::into_owned),
                });
            }
            Event::MappingEnd | Event::SequenceEnd => {
                let frame = self
                    .stack
                    .pop()
                    .expect("structure end without matching start");
                let node = self.close_frame(frame);
                self.attach(node);
            }
            Event::Scalar(value, style, aid, tag) => {
                let node = match tag.as_ref() {
                    Some(tag_ref) if !tag_ref.is_yaml_core_schema() => {
                        let inner =
                            Scalar::parse_from_cow_and_metadata(value, style, None)
                                .map_or(YamlOwned::BadValue, |scalar| {
                                    YamlOwned::Value(scalar.into_owned())
                                });
                        YamlOwned::Tagged(tag_ref.as_ref().clone(), Box::new(inner))
                    }
                    _ => {
                        Scalar::parse_from_cow_and_metadata(value, style, tag.as_ref())
                            .map_or(YamlOwned::BadValue, |scalar| {
                                YamlOwned::Value(scalar.into_owned())
                            })
                    }
                };
                if aid > 0 {
                    self.anchor_map.insert(aid, node.clone());
                }
                self.attach(node);
            }
            Event::Alias(id) => {
                // granit normally rejects unresolved aliases during the scan,
                // but a linter must never panic on malformed input, so fall
                // back to `BadValue` rather than relying on that ordering.
                // `unwrap_or` constructs the unit variant eagerly, keeping the
                // branch covered without a lazy closure.
                let node = self
                    .anchor_map
                    .get(&id)
                    .cloned()
                    .unwrap_or(YamlOwned::BadValue);
                self.attach(node);
            }
        }
    }
}

impl Loader {
    fn close_frame(&mut self, frame: Frame) -> YamlOwned {
        let Frame {
            container,
            anchor_id,
            tag,
        } = frame;
        let node = match container {
            Container::Sequence(seq) => YamlOwned::Sequence(seq),
            Container::Mapping(map, _) => YamlOwned::Mapping(map),
        };
        let node = match tag {
            Some(tag) if !tag.is_yaml_core_schema() => {
                YamlOwned::Tagged(tag, Box::new(node))
            }
            _ => node,
        };
        if anchor_id > 0 {
            self.anchor_map.insert(anchor_id, node.clone());
        }
        node
    }

    fn attach(&mut self, node: YamlOwned) {
        match self.stack.last_mut() {
            Some(frame) => match &mut frame.container {
                Container::Sequence(seq) => seq.push(node),
                Container::Mapping(map, pending_key) => {
                    if matches!(pending_key, YamlOwned::BadValue) {
                        *pending_key = node;
                    } else {
                        let key = std::mem::replace(pending_key, YamlOwned::BadValue);
                        map.insert(key, node);
                    }
                }
            },
            None => self.root = Some(node),
        }
    }
}
