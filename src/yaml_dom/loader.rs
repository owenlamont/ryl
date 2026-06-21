// Vendored from saphyr v0.0.6 (`saphyr/src/loader.rs`), specialized to `YamlOwned`
// and retargeted at granit-parser's event stream. Saphyr is MIT OR Apache-2.0; ryl
// ships under MIT.

use std::borrow::Cow;
use std::collections::BTreeMap;

use granit_parser::{
    Event, Marker, Parser, ScanError, Span, SpannedEventReceiver, Tag,
};

use super::{core_schema_suffix, is_core_schema};
use crate::yaml_dom::scalar::Scalar;
use crate::yaml_dom::yaml_owned::{MappingOwned, YamlOwned};

/// Cap on alias-expanded nodes, bounding a billion-laughs payload.
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
    /// Child node count so far; the container's own size is this plus one
    /// ([`Loader::close_frame`]).
    size: usize,
}

#[derive(Default)]
struct Loader {
    docs: Vec<YamlOwned>,
    stack: Vec<Frame>,
    root: Option<YamlOwned>,
    anchor_map: BTreeMap<usize, YamlOwned>,
    /// Node count of each anchor's value, so an alias costs O(1) without walking the
    /// (possibly huge) cloned subtree.
    anchor_sizes: BTreeMap<usize, usize>,
    /// Running total of alias-expanded nodes. Source-built nodes are bounded by the
    /// input length and not counted; only clones can multiply.
    expanded: usize,
    /// First location where [`MAX_EXPANDED_NODES`] was exceeded.
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
                    Some(tag_ref) if !is_core_schema(tag_ref) => {
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
                // granit rejects unresolved aliases during the scan, but a linter must
                // not panic on malformed input, so fall back to `BadValue`.
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
        // `default_suffix` is the core tag this collection resolves to implicitly, so
        // an explicit tag equal to it is redundant and dropped.
        let (node, default_suffix) = match container {
            Container::Sequence(seq) => (YamlOwned::Sequence(seq), "seq"),
            Container::Mapping(map, _) => (YamlOwned::Mapping(map), "map"),
        };
        // Drop only the matching default core tag; a mismatched core tag (`!!seq` on a
        // mapping), unknown core suffix (`!!custom`), or local tag is kept as `Tagged`
        // so config validation rejects it rather than treating the node as untyped.
        let node = match tag {
            Some(tag)
                if core_schema_suffix(&tag).as_deref() == Some(default_suffix) =>
            {
                node
            }
            Some(tag) => YamlOwned::Tagged(tag, Box::new(node)),
            None => node,
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
