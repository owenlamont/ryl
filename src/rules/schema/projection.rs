use std::borrow::Cow;
use std::collections::BTreeMap;
use std::iter::Peekable;

use ryl_yaml_parser::{Event, ScalarStyle, Span, Tag};
use serde_json::{Map, Number, Value};

use crate::rules::support::yaml_version;
use crate::yaml_dom::{
    Scalar, core_schema_int, core_schema_suffix, is_core_schema_null,
    parse_core_schema_fp, parse_core_schema_int,
};

const MAX_NODES: usize = 1_000_000;
const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_DEPTH: usize = 128;

#[derive(Default)]
pub(super) struct Budget {
    nodes: usize,
    bytes: usize,
}

impl Budget {
    pub(super) fn exhausted(&self) -> bool {
        self.nodes > MAX_NODES || self.bytes > MAX_BYTES
    }

    fn charge(
        &mut self,
        nodes: usize,
        bytes: usize,
        span: Span,
    ) -> Result<(), Refusal> {
        self.nodes = self.nodes.saturating_add(nodes);
        self.bytes = self.bytes.saturating_add(bytes);
        if self.exhausted() {
            return Err(refuse("projection resource limit exceeded", span));
        }
        Ok(())
    }
}

pub(super) struct Refusal {
    pub(super) reason: String,
    pub(super) span: Box<Span>,
}

fn refuse(reason: &str, span: Span) -> Refusal {
    Refusal {
        reason: reason.to_owned(),
        span: Box::new(span),
    }
}

#[derive(Clone)]
struct Node {
    value: Value,
    nodes: BTreeMap<String, Span>,
    keys: BTreeMap<String, Span>,
    span: Span,
    size: usize,
    bytes: usize,
    depth: usize,
    merge_key: bool,
}

pub(super) struct Projection {
    pub(super) value: Value,
    nodes: BTreeMap<String, Span>,
    pub(super) keys: BTreeMap<String, Span>,
    pub(super) root: Span,
}

impl Projection {
    pub(super) fn span(&self, pointer: &str) -> Span {
        self.nodes.get(pointer).copied().unwrap_or(self.root)
    }
}

pub(super) fn child_pointer(parent: &str, key: &str) -> String {
    format!("{parent}/{}", key.replace('~', "~0").replace('/', "~1"))
}

fn integer(value: &num_bigint::BigInt, span: Span) -> Result<Value, Refusal> {
    if let Ok(value) = i64::try_from(value) {
        return Ok(value.into());
    }
    u64::try_from(value)
        .map(Value::from)
        .map_err(|_| refuse("integer outside i64/u64 range", span))
}

fn float(value: f64, span: Span) -> Result<Value, Refusal> {
    Number::from_f64(value)
        .map(Value::Number)
        .ok_or_else(|| refuse("non-finite float", span))
}

fn scalar(
    raw: Cow<'_, str>,
    style: ScalarStyle,
    tag: Option<&Tag>,
    yaml11: bool,
    span: Span,
) -> Result<Value, Refusal> {
    let suffix = tag.and_then(core_schema_suffix);
    if tag.is_some_and(|tag| tag.handle().is_empty() && tag.suffix() == "!") {
        return Ok(Value::String(raw.into_owned()));
    }
    if let Some(tag) = tag
        && suffix.is_none()
    {
        return Err(refuse(&format!("tag {}", tag.original()), span));
    }
    let suffix = suffix.as_deref();
    if suffix == Some("str") {
        return Ok(Value::String(raw.into_owned()));
    }
    if yaml11 && suffix == Some("float") {
        let number = raw.replace('_', "");
        let decimal = format!("{number}.0");
        let value = yaml_version::value(&decimal, true, None, true)
            .or_else(|| yaml_version::value(&number, true, None, true));
        return if let Some(yaml_version::Value::Float(value)) = value {
            float(value.0, span)
        } else {
            parse_core_schema_fp(&number)
                .ok_or_else(|| refuse("invalid scalar tag", span))
                .and_then(|value| float(value, span))
        };
    }
    if yaml11 && suffix == Some("bool") {
        match raw.as_ref() {
            "y" | "Y" => return Ok(Value::Bool(true)),
            "n" | "N" => return Ok(Value::Bool(false)),
            _ => {}
        }
    }
    let implicit = suffix.is_none() && style == ScalarStyle::Plain;
    if yaml11 && (implicit || matches!(suffix, Some("int" | "bool"))) {
        if let Some(value) = yaml_version::value(&raw, true, suffix, true) {
            if !matches!(
                (suffix, &value),
                (None, _)
                    | (Some("bool"), yaml_version::Value::Bool(_))
                    | (Some("int"), yaml_version::Value::Int(_))
            ) {
                return Err(refuse("invalid scalar tag", span));
            }
            return match value {
                yaml_version::Value::Bool(value) => Ok(Value::Bool(value)),
                yaml_version::Value::Int(value) => integer(&value, span),
                yaml_version::Value::Float(value) => float(value.0, span),
            };
        }
        if !implicit {
            return Err(refuse("invalid scalar tag", span));
        }
        if !is_core_schema_null(&raw)
            && yaml_version::resolves_to_nonstring_in_yaml_1_1(&raw)
        {
            return Err(refuse("unsupported YAML 1.1 scalar", span));
        }
        if implicit && !is_core_schema_null(&raw) {
            return Ok(Value::String(raw.into_owned()));
        }
    }
    if (implicit || suffix == Some("int"))
        && parse_core_schema_int(&raw).is_none()
        && let Some(value) = core_schema_int(&raw)
    {
        return integer(&value, span);
    }
    let tag = tag.map(Cow::Borrowed);
    match Scalar::resolve_scalar(raw, style, tag.as_ref()) {
        Some(Scalar::Null) => Ok(Value::Null),
        Some(Scalar::Boolean(value)) => Ok(Value::Bool(value)),
        Some(Scalar::Integer(value)) => Ok(value.into()),
        Some(Scalar::FloatingPoint(value)) => float(value.0, span),
        Some(Scalar::String(value)) => Ok(Value::String(value.into_owned())),
        None => Err(refuse("invalid scalar tag", span)),
    }
}

struct Builder<'budget> {
    anchors: BTreeMap<usize, Node>,
    start: Span,
    yaml11: bool,
    budget: &'budget mut Budget,
}

impl Builder<'_> {
    fn node<'a>(
        &mut self,
        events: &mut Peekable<impl Iterator<Item = (Event<'a>, Span)>>,
        depth: usize,
        key_context: bool,
    ) -> Result<Node, Refusal> {
        let (event, span) = events
            .next()
            .ok_or_else(|| refuse("incomplete YAML document", self.start))?;
        self.budget.charge(1, 0, span)?;
        if depth > MAX_DEPTH {
            return Err(refuse("projection depth limit exceeded", span));
        }
        if let Event::Alias(id) = event {
            let anchor = self
                .anchors
                .get(&id)
                .ok_or_else(|| refuse("unresolved alias", span))?;
            self.budget.charge(anchor.size, anchor.bytes, span)?;
            if depth + anchor.depth > MAX_DEPTH {
                return Err(refuse("projection depth limit exceeded", span));
            }
            let mut node = anchor.clone();
            node.span = span;
            node.nodes
                .values_mut()
                .chain(node.keys.values_mut())
                .for_each(|site| *site = span);
            return Ok(node);
        }
        let mut node = Node {
            value: Value::Null,
            nodes: BTreeMap::new(),
            keys: BTreeMap::new(),
            span,
            size: 1,
            bytes: 0,
            depth: 0,
            merge_key: false,
        };
        let anchor;
        let mapping = matches!(event, Event::MappingStart(..));
        match event {
            Event::Scalar(raw, style, id, tag) => {
                anchor = id;
                self.budget.charge(0, raw.len(), span)?;
                node.bytes = raw.len();
                node.merge_key = raw == "<<"
                    && (style == ScalarStyle::Plain && tag.is_none()
                        || tag.as_deref().and_then(core_schema_suffix).as_deref()
                            == Some("merge"));
                node.value = if node.merge_key && key_context {
                    Value::String(raw.into_owned())
                } else {
                    scalar(raw, style, tag.as_deref(), self.yaml11, span)?
                };
            }
            Event::SequenceStart(_, id, tag) | Event::MappingStart(_, id, tag) => {
                anchor = id;
                let expected = if mapping { "map" } else { "seq" };
                if let Some(tag) = tag
                    && !(tag.handle().is_empty() && tag.suffix() == "!")
                    && core_schema_suffix(&tag).as_deref() != Some(expected)
                {
                    return Err(refuse(&format!("tag {}", tag.original()), span));
                }
                if mapping {
                    self.mapping(events, depth, &mut node)?;
                } else {
                    let mut values = Vec::new();
                    while !matches!(events.peek(), Some((Event::SequenceEnd, _))) {
                        let child = self.node(events, depth + 1, false)?;
                        node.attach(
                            &child,
                            &format!("/{}", values.len()),
                            self.budget,
                        )?;
                        values.push(child.value);
                    }
                    node.value = Value::Array(values);
                }
                let (_, end) = events.next().expect("parser closes collection");
                node.span = Span::new(span.start, end.end);
            }
            _ => return Err(refuse("unexpected node event", span)),
        }
        node.nodes.insert(String::new(), node.span);
        if anchor != 0 {
            self.budget.charge(node.size, node.bytes, span)?;
            self.anchors.insert(anchor, node.clone());
        }
        Ok(node)
    }

    fn mapping<'a>(
        &mut self,
        events: &mut Peekable<impl Iterator<Item = (Event<'a>, Span)>>,
        depth: usize,
        node: &mut Node,
    ) -> Result<(), Refusal> {
        let mut explicit = Map::new();
        let mut merged = Map::new();
        let mut merge_seen = false;
        while !matches!(events.peek(), Some((Event::MappingEnd, _))) {
            let key = self.node(events, depth + 1, true)?;
            let Value::String(name) = key.value else {
                return Err(refuse("non-string or complex mapping key", key.span));
            };
            let child = self.node(events, depth + 1, false)?;
            if key.merge_key {
                if merge_seen {
                    return Err(refuse("duplicate mapping key", key.span));
                }
                merge_seen = true;
                node.merge(&child, &explicit, &mut merged, self.budget, key.span)?;
            } else {
                if explicit.contains_key(&name) {
                    return Err(refuse("duplicate mapping key", key.span));
                }
                let pointer = child_pointer("", &name);
                if merged.contains_key(&name) {
                    node.remove_subtree(&pointer);
                }
                node.attach(&child, &pointer, self.budget)?;
                node.keys.insert(pointer, key.span);
                explicit.insert(name, child.value);
            }
        }
        merged.extend(explicit);
        node.value = Value::Object(merged);
        Ok(())
    }
}

impl Node {
    fn merge(
        &mut self,
        child: &Self,
        explicit: &Map<String, Value>,
        merged: &mut Map<String, Value>,
        budget: &mut Budget,
        span: Span,
    ) -> Result<(), Refusal> {
        let sources = match &child.value {
            Value::Object(_) => vec![(&child.value, String::new())],
            Value::Array(values) => values
                .iter()
                .enumerate()
                .map(|(i, value)| (value, format!("/{i}")))
                .collect(),
            _ => return Err(refuse("merge key", span)),
        };
        for (source, prefix) in sources {
            let Value::Object(source) = source else {
                return Err(refuse("merge key", span));
            };
            for (name, value) in source {
                if merged.contains_key(name) || explicit.contains_key(name) {
                    continue;
                }
                let pointer = child_pointer(&prefix, name);
                let subtree = child.subtree(&pointer, value, budget)?;
                let target = child_pointer("", name);
                self.attach(&subtree, &target, budget)?;
                self.keys.insert(
                    target,
                    child.keys.get(&pointer).copied().unwrap_or(subtree.span),
                );
                merged.insert(name.clone(), subtree.value);
            }
        }
        Ok(())
    }

    fn remove_subtree(&mut self, pointer: &str) {
        let prefix = format!("{pointer}/");
        for map in [&mut self.nodes, &mut self.keys] {
            map.remove(pointer);
            while let Some((path, _)) = map
                .range(prefix.clone()..)
                .next()
                .filter(|(path, _)| path.starts_with(&prefix))
            {
                let path = path.clone();
                map.remove(&path);
            }
        }
    }
    fn attach(
        &mut self,
        child: &Self,
        prefix: &str,
        budget: &mut Budget,
    ) -> Result<(), Refusal> {
        self.size += child.size;
        self.bytes += child.bytes;
        self.depth = self.depth.max(child.depth + 1);
        for (path, span) in &child.nodes {
            let pointer = format!("{prefix}{path}");
            budget.charge(0, pointer.len(), *span)?;
            self.bytes += pointer.len();
            self.nodes.insert(pointer, *span);
        }
        for (path, span) in &child.keys {
            let pointer = format!("{prefix}{path}");
            budget.charge(0, pointer.len(), *span)?;
            self.bytes += pointer.len();
            self.keys.insert(pointer, *span);
        }
        Ok(())
    }

    fn subtree(
        &self,
        pointer: &str,
        value: &Value,
        budget: &mut Budget,
    ) -> Result<Self, Refusal> {
        let prefix = format!("{pointer}/");
        let descendants = |map: &BTreeMap<String, Span>| {
            map.range(prefix.clone()..)
                .take_while(|(path, _)| path.starts_with(&prefix))
                .map(|(path, span)| (path[pointer.len()..].to_owned(), *span))
                .collect::<BTreeMap<_, _>>()
        };
        let mut nodes = descendants(&self.nodes);
        let root = self.nodes[pointer];
        nodes.insert(String::new(), root);
        let keys = descendants(&self.keys);
        let bytes = value_bytes(value)
            + nodes
                .keys()
                .chain(keys.keys())
                .map(String::len)
                .sum::<usize>();
        budget.charge(nodes.len(), bytes, root)?;
        Ok(Self {
            value: value.clone(),
            size: nodes.len(),
            bytes,
            depth: self.depth,
            span: nodes.get("").copied().unwrap_or(self.span),
            nodes,
            keys,
            merge_key: false,
        })
    }
}

fn value_bytes(value: &Value) -> usize {
    match value {
        Value::String(value) => value.len(),
        Value::Array(values) => values.iter().map(value_bytes).sum(),
        Value::Object(values) => values
            .iter()
            .map(|(key, value)| key.len() + value_bytes(value))
            .sum(),
        _ => 8,
    }
}

pub(super) fn document<'a>(
    events: &mut Peekable<impl Iterator<Item = (Event<'a>, Span)>>,
    budget: &mut Budget,
    start: Span,
    version: Option<ryl_yaml_parser::YamlVersion>,
) -> Result<Projection, Refusal> {
    let mut builder = Builder {
        anchors: BTreeMap::new(),
        start,
        yaml11: yaml_version::resolves_as_yaml_1_1(
            version.map(yaml_version::event_version),
        ),
        budget,
    };
    let node = builder.node(events, 0, false)?;
    Ok(Projection {
        value: node.value,
        nodes: node.nodes,
        keys: node.keys,
        root: node.span,
    })
}
