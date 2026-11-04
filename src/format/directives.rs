use std::ops::Range;

use ryl_yaml_parser::{Event, Placement, Span, StructureStyle};

use crate::rules::support::span_utils::marker_byte_offset;
use crate::text_version::{Analysis, lexical::Kind};

#[derive(Clone, Default)]
pub(crate) struct Suppression {
    pub(crate) lines: Vec<Range<usize>>,
    pub(crate) mappings: Vec<usize>,
    pub(crate) prefixes: Vec<Range<usize>>,
}

struct Node {
    start: usize,
    mapping: bool,
    header: Option<usize>,
    first: usize,
    last: usize,
    column: usize,
    value: Option<usize>,
}

struct Collection {
    node: usize,
    mapping: bool,
    flow: bool,
    key: Option<usize>,
}

#[derive(Default)]
struct Builder {
    lines: Vec<Range<usize>>,
    nodes: Vec<Node>,
    stack: Vec<Collection>,
    ignored: Vec<usize>,
    pending: Option<usize>,
    disabled: Option<usize>,
}

impl Suppression {
    pub(crate) fn intersects(&self, first: usize, last: usize) -> bool {
        let before = self.lines.partition_point(|range| range.end <= first);
        self.lines
            .get(before)
            .is_some_and(|range| range.start <= last)
    }

    pub(crate) fn parse(input: Analysis<'_>) -> Self {
        if !input.text().contains("ryl format-") {
            return Self::default();
        }
        let mut builder = Builder::default();
        let mut properties =
            input
                .tokens()
                .iter(input.text())
                .filter_map(|(span, kind)| {
                    matches!(kind, Kind::Anchor | Kind::Tag)
                        .then_some(span.start.line())
                });
        let mut headers =
            crate::rules::indentation::block_collection_headers(input).into_iter();
        for (event, span) in input.event_iter() {
            let header = matches!(
                event,
                Event::MappingStart(StructureStyle::Block, ..)
                    | Event::SequenceStart(StructureStyle::Block, ..)
            )
            .then(|| headers.next().expect("each block collection has a header"));
            let count = usize::from(event.anchor_id().is_some())
                + usize::from(event.tag().is_some());
            let first = properties
                .by_ref()
                .take(count)
                .min()
                .map_or(span.start.line(), |line| line.min(span.start.line()));
            builder.observe(event, span, header, first);
        }
        builder.finish(input)
    }
}

impl Builder {
    fn observe(
        &mut self,
        event: Event<'_>,
        span: Span,
        header: Option<usize>,
        first: usize,
    ) {
        match event {
            Event::Comment(text, placement) if placement != Placement::Right => {
                self.comment(&text, span);
            }
            Event::DocumentEnd => {
                self.enable(span.start.line());
                self.pending = None;
            }
            Event::MappingEnd | Event::SequenceEnd => self.end(span),
            event if event.is_node() => self.node(&event, span, header, first),
            _ => {}
        }
    }

    fn enable(&mut self, line: usize) {
        if let Some(first) = self.disabled.take() {
            self.lines.push(first..line.max(first) + 1);
        }
    }

    fn comment(&mut self, text: &str, span: Span) {
        let line = span.start.line();
        match text.trim_end() {
            " ryl format-ignore" => {
                self.lines.push(line..line + 1);
                self.pending = Some(line);
            }
            " ryl format-disable" => {
                self.disabled.get_or_insert(line);
            }
            " ryl format-enable" => self.enable(line),
            _ => {}
        }
        if let Some(collection) = self.stack.last() {
            let node = &mut self.nodes[collection.node];
            if span.start.col() >= node.column {
                node.last = node.last.max(line);
            }
        }
    }

    fn node(
        &mut self,
        event: &Event<'_>,
        span: Span,
        header: Option<usize>,
        first: usize,
    ) {
        let index = self.nodes.len();
        let ignored = self.pending.take_if(|line| *line < span.start.line());
        self.nodes.push(Node {
            start: marker_byte_offset(span.start).get(),
            mapping: matches!(event, Event::MappingStart(..)),
            header,
            first: ignored.map_or(first, |line| first.min(line + 1)),
            last: last_line(span),
            column: span.start.col(),
            value: None,
        });
        if ignored.is_some() {
            self.ignored.push(index);
        }
        if let Some(parent) = self.stack.last_mut()
            && parent.mapping
        {
            if let Some(key) = parent.key.take() {
                self.nodes[key].value = Some(index);
            } else {
                parent.key = Some(index);
            }
        }
        match event {
            Event::MappingStart(style, ..) | Event::SequenceStart(style, ..) => {
                self.stack.push(Collection {
                    node: index,
                    mapping: matches!(event, Event::MappingStart(..)),
                    flow: *style == StructureStyle::Flow,
                    key: None,
                });
            }
            _ => self.complete(index),
        }
    }

    fn end(&mut self, span: Span) {
        let collection = self.stack.pop().expect("a collection end has a start");
        if collection.flow {
            self.nodes[collection.node].last = last_line(span);
        }
        self.complete(collection.node);
    }

    fn complete(&mut self, index: usize) {
        if let Some(parent) = self.stack.last() {
            self.nodes[parent.node].last =
                self.nodes[parent.node].last.max(self.nodes[index].last);
        }
    }

    fn finish(mut self, input: Analysis<'_>) -> Suppression {
        let line_count = input.line_count();
        if let Some(first) = self.disabled {
            self.lines.push(first..line_count + 1);
        }
        self.lines.extend(self.ignored.into_iter().map(|index| {
            let node = &self.nodes[index];
            node.first..self.nodes[node.value.unwrap_or(index)].last + 1
        }));
        self.lines.sort_unstable_by_key(|range| range.start);
        let mut merged: Vec<Range<usize>> = Vec::new();
        for range in self.lines {
            if let Some(previous) = merged.last_mut()
                && range.start <= previous.end
            {
                previous.end = previous.end.max(range.end);
            } else {
                merged.push(range);
            }
        }
        let mut suppression = Suppression {
            lines: merged,
            ..Suppression::default()
        };
        for (index, &first) in input.byte_line_starts().iter().enumerate() {
            if suppression.intersects(index + 1, index + 1) {
                let tail = &input.text()[first..];
                let end = first + tail.len()
                    - tail.trim_start_matches([' ', '\t', '\u{feff}']).len();
                suppression.prefixes.push(first..end);
            }
        }
        for node in &self.nodes {
            if let Some(at) = node.header
                && suppression.intersects(node.first, node.last)
            {
                let first = input.text()[..at]
                    .rfind(['\n', '\r'])
                    .map_or(0, |at| at + 1);
                suppression.prefixes.push(first..at);
            }
        }
        suppression
            .prefixes
            .sort_unstable_by_key(|range| (range.start, range.end));
        suppression.mappings = self
            .nodes
            .iter()
            .filter(|node| {
                node.mapping && suppression.intersects(node.first, node.last)
            })
            .map(|node| node.start)
            .collect();
        suppression
    }
}

fn last_line(span: Span) -> usize {
    if span.end.col() == 0 && span.end.line() > span.start.line() {
        span.end.line() - 1
    } else {
        span.end.line()
    }
}
