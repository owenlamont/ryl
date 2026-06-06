// Vendored from saphyr v0.0.6 (`saphyr/src/loader.rs`).
// Specialized to a single concrete node type (`YamlOwned`) and retargeted at
// granit-parser's event stream. The original is dual-licensed MIT OR
// Apache-2.0; ryl ships under MIT.

use std::borrow::Cow;
use std::collections::BTreeMap;

use granit_parser::{
    Event, Marker, Parser, ScanError, Span, SpannedEventReceiver, Tag,
};

use crate::yaml_dom::scalar::Scalar;
use crate::yaml_dom::yaml_owned::{MappingOwned, YamlOwned};

/// Upper bound on the number of nodes an alias may materialise across a whole
/// document. granit emits one O(1) `Event::Alias` per alias and never expands;
/// this loader is what resolves an alias by cloning the referenced subtree, so
/// nested anchors (`a: &a [..]`, `b: &b [*a, *a, ..]`, …) would otherwise blow up
/// exponentially — the classic "billion laughs" denial of service. Real configs
/// (the only input that reaches this loader) expand to at most a few hundred
/// nodes, so this ceiling is enormous headroom while still bounding a malicious
/// payload to a fixed, fast-to-reject cost.
const MAX_EXPANDED_NODES: usize = 1_000_000;

/// # Errors
/// Propagates [`ScanError`] when the granit parser rejects the input, or when
/// alias expansion exceeds [`MAX_EXPANDED_NODES`] (a billion-laughs payload).
pub fn load_owned_documents(source: &str) -> Result<Vec<YamlOwned>, ScanError> {
    let mut parser = Parser::new_from_str(source);
    let mut loader = Loader::default();
    parser.load(&mut loader, true)?;
    if let Some(mark) = loader.overflow {
        return Err(ScanError::new(
            mark,
            format!(
                "too many alias expansions (limit {MAX_EXPANDED_NODES}); \
                 refusing to expand further to avoid a denial-of-service"
            ),
        ));
    }
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
    /// Node count of the children attached so far; the container's own size is
    /// this plus one (computed in [`Loader::close_frame`]).
    size: usize,
}

#[derive(Default)]
struct Loader {
    docs: Vec<YamlOwned>,
    stack: Vec<Frame>,
    root: Option<YamlOwned>,
    anchor_map: BTreeMap<usize, YamlOwned>,
    /// Materialised node count of each anchor's value, so an alias can be costed
    /// in O(1) without walking the (possibly huge) cloned subtree.
    anchor_sizes: BTreeMap<usize, usize>,
    /// Running total of nodes produced by alias expansion. Source-built nodes are
    /// bounded by the input length and are not counted; only clones can multiply.
    expanded: usize,
    /// Set to the first location where [`MAX_EXPANDED_NODES`] was exceeded.
    overflow: Option<Marker>,
}

impl<'input> SpannedEventReceiver<'input> for Loader {
    fn on_event(&mut self, ev: Event<'input>, span: Span) {
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
                    size: 0,
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
                    size: 0,
                });
            }
            Event::MappingEnd | Event::SequenceEnd => {
                let frame = self
                    .stack
                    .pop()
                    .expect("structure end without matching start");
                let (node, size) = self.close_frame(frame);
                self.attach(node, size);
            }
            Event::Scalar(value, style, aid, tag) => {
                let node = match tag.as_ref() {
                    Some(tag_ref) if !tag_ref.is_yaml_core_schema() => {
                        let inner = Scalar::resolve_scalar(value, style, None)
                            .map_or(YamlOwned::BadValue, |scalar| {
                                YamlOwned::Value(scalar.into_owned())
                            });
                        YamlOwned::Tagged(tag_ref.as_ref().clone(), Box::new(inner))
                    }
                    _ => Scalar::resolve_scalar(value, style, tag.as_ref())
                        .map_or(YamlOwned::BadValue, |scalar| {
                            YamlOwned::Value(scalar.into_owned())
                        }),
                };
                if aid > 0 {
                    self.anchor_map.insert(aid, node.clone());
                    self.anchor_sizes.insert(aid, 1);
                }
                self.attach(node, 1);
            }
            Event::Alias(id) => {
                // granit normally rejects unresolved aliases during the scan,
                // but a linter must never panic on malformed input, so fall
                // back to `BadValue` rather than relying on that ordering.
                let size = self.anchor_sizes.get(&id).copied().unwrap_or(1);
                if self.expanded.saturating_add(size) > MAX_EXPANDED_NODES {
                    self.overflow.get_or_insert(span.start);
                    self.attach(YamlOwned::BadValue, 1);
                } else {
                    self.expanded += size;
                    let node = self
                        .anchor_map
                        .get(&id)
                        .cloned()
                        .unwrap_or(YamlOwned::BadValue);
                    self.attach(node, size);
                }
            }
        }
    }
}

impl Loader {
    fn close_frame(&mut self, frame: Frame) -> (YamlOwned, usize) {
        let Frame {
            container,
            anchor_id,
            tag,
            size,
        } = frame;
        let self_size = size + 1;
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
            self.anchor_sizes.insert(anchor_id, self_size);
        }
        (node, self_size)
    }

    fn attach(&mut self, node: YamlOwned, size: usize) {
        match self.stack.last_mut() {
            Some(frame) => {
                frame.size += size;
                match &mut frame.container {
                    Container::Sequence(seq) => seq.push(node),
                    Container::Mapping(map, pending_key) => {
                        if matches!(pending_key, YamlOwned::BadValue) {
                            *pending_key = node;
                        } else {
                            let key =
                                std::mem::replace(pending_key, YamlOwned::BadValue);
                            map.insert(key, node);
                        }
                    }
                }
            }
            None => self.root = Some(node),
        }
    }
}
