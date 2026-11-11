use ryl_yaml_parser::{Event, ScalarStyle, StructureStyle};

use super::spelling::{EditSite, Fragment};
use crate::rules::comments::Attachments;
use crate::rules::support::span_utils::BytePos;
use crate::rules::support::span_utils::marker_byte_offset;
use crate::rules::support::token_spacing;
use crate::rules::{colons, comments, hyphens, indentation, quoted_strings};
use crate::text_version::Analysis;
use crate::text_version::lexical::Kind as LexicalKind;
use crate::text_version::model::{Node, NodeKind};

#[derive(Clone, Copy)]
pub(super) struct Boundary(usize);

pub(super) struct Collection {
    flow: bool,
    braced: bool,
}

impl Collection {
    pub(super) const fn flow(&self) -> bool {
        self.flow
    }
    pub(super) const fn braced(&self) -> bool {
        self.braced
    }
}

pub(super) struct KeyGap {
    own: colons::RetainedKey,
}

impl KeyGap {
    pub(super) const fn own(&self) -> &colons::RetainedKey {
        &self.own
    }
    pub(super) fn flow_start(&self) -> Boundary {
        Boundary(self.own.at().get() + 1)
    }
    pub(super) fn block_start(&self) -> Boundary {
        Boundary(self.own.after().get())
    }
}

/// Where an outermost flow collection lands: `column` is the final column after what
/// precedes it, ending at byte `before`, and `base` the final column its expanded entries
/// indent from.
pub(super) struct Placement {
    before: usize,
    pub(super) column: usize,
    pub(super) base: usize,
    /// The final column of the content of the line holding `before`.
    pub(super) line: usize,
    pub(super) lead: bool,
    pub(super) nested_dash: bool,
}

impl Placement {
    pub(super) const fn boundary(&self) -> Boundary {
        Boundary(self.before)
    }
    pub(super) const fn edit(&self, end: Boundary) -> EditSite {
        EditSite::new(self.before, end.0)
    }
}

pub(super) struct FlowSite<'a> {
    start: usize,
    end: usize,
    comments: bool,
    indicators: token_spacing::RetainedIndicators<'a>,
}

impl FlowSite<'_> {
    pub(super) const fn clear(&self) -> bool {
        !self.comments
    }
    pub(super) const fn position(&self) -> BytePos {
        BytePos::new(self.start)
    }
    pub(super) const fn indicators(&self) -> &token_spacing::RetainedIndicators<'_> {
        &self.indicators
    }
    pub(super) fn edit(&self) -> EditSite {
        EditSite::new(self.start, self.end)
    }
}

pub(super) struct Outer<'a> {
    prefix: hyphens::CollectionPrefix,
    comment: comments::Trailing,
    ytt_directive: bool,
    fragment: Option<Fragment>,
    extent: Fragment,
    indicators: token_spacing::RetainedIndicators<'a>,
}

impl<'a> Outer<'a> {
    pub(super) const fn prefix(&self) -> &hyphens::CollectionPrefix {
        &self.prefix
    }
    pub(super) const fn comment(&self) -> &comments::Trailing {
        &self.comment
    }
    pub(super) const fn ytt_directive(&self) -> bool {
        self.ytt_directive
    }
    pub(super) fn take_comment(&mut self) -> Option<Fragment> {
        self.fragment.take()
    }
    pub(super) const fn extent(&self) -> &Fragment {
        &self.extent
    }
    pub(super) const fn indicators(&self) -> &token_spacing::RetainedIndicators<'a> {
        &self.indicators
    }
}

#[derive(Clone, Copy)]
pub(super) struct Initial<'a> {
    source: Analysis<'a>,
    sites: &'a [Site],
    comments: &'a Attachments,
    selected: Option<(
        &'a [crate::format::layout::PlanEdit],
        &'a quoted_strings::Styles,
        &'a crate::format::layout::projection::Projection,
    )>,
}

impl<'a> Initial<'a> {
    pub(super) const fn new(
        source: Analysis<'a>,
        sites: &'a [Site],
        comments: &'a Attachments,
    ) -> Self {
        Self {
            source,
            sites,
            comments,
            selected: None,
        }
    }

    pub(super) const fn unselected(self) -> Self {
        Self {
            selected: None,
            ..self
        }
    }

    pub(super) fn selected(
        mut self,
        edits: &'a [crate::format::layout::PlanEdit],
        styles: &'a quoted_strings::Styles,
        projection: &'a crate::format::layout::projection::Projection,
    ) -> Self {
        self.selected = Some((edits, styles, projection));
        self
    }

    pub(super) fn flow_spelled(
        mut self,
        spelling: &'a quoted_strings::FlowSpellings,
    ) -> Self {
        let (_, _, projection) =
            self.selected.expect("flow candidates have selected edits");
        self.selected = Some((&spelling.edits, &spelling.styles, projection));
        self
    }

    pub(super) fn collection(&self, node: usize) -> Collection {
        let site = &self.sites[node];
        Collection {
            flow: site.flow,
            braced: site.braced,
        }
    }

    pub(super) fn explicit_key(&self, node: usize) -> bool {
        self.sites[node].explicit_key
    }

    pub(super) fn scalar(&self, node: usize) -> Option<quoted_strings::Retained<'a>> {
        let site = &self.sites[node];
        site.scalar.map(|style| {
            let (style, text) = self
                .selected
                .and_then(|(edits, styles, _)| {
                    let index = edits
                        .binary_search_by_key(&site.start, |(from, _, _)| from.get())
                        .ok()?;
                    match (&edits[index], styles.at(site.start)) {
                        (
                            (
                                from,
                                end,
                                crate::format::layout::Replacement::Text {
                                    text, ..
                                },
                            ),
                            Some(style),
                        ) if from.get() == site.start && end.get() == site.end => {
                            Some((style, text.as_str()))
                        }
                        _ => None,
                    }
                })
                .unwrap_or((style, &self.source.text()[site.start..site.end]));
            quoted_strings::retained_spelling(style, text)
        })
    }

    pub(super) fn line(&self, node: usize) -> indentation::RetainedLine {
        indentation::RetainedLine::from_position(self.sites[node].line)
    }
    pub(super) fn column(&self, node: usize) -> indentation::RetainedColumn {
        indentation::RetainedColumn::from_position(self.sites[node].column)
    }
    pub(super) fn start(&self, node: usize) -> Boundary {
        Boundary(self.sites[node].start)
    }
    pub(super) fn open(&self, node: usize) -> Boundary {
        Boundary(self.sites[node].start + 1)
    }
    pub(super) fn end(&self, node: usize) -> Boundary {
        Boundary(self.sites[node].end)
    }

    fn skip_space(&self, from: usize) -> usize {
        self.source.text()[from..]
            .find(|ch| !super::is_space(ch))
            .map_or(self.source.text().len(), |at| from + at)
    }

    fn after(&self, end: usize) -> Boundary {
        let at = self.skip_space(end);
        Boundary(at + usize::from(self.source.text()[at..].starts_with(',')))
    }

    pub(super) fn after_entry(&self, node: usize) -> Boundary {
        self.after(self.sites[node].end)
    }
    pub(super) fn after_value(&self, node: usize, gap: &KeyGap) -> Boundary {
        self.after(self.sites[node].end.max(gap.block_start().0))
    }
    pub(super) fn key_gap(&self, node: usize) -> KeyGap {
        let own = colons::retained_key(
            self.source.text(),
            BytePos::new(self.sites[node].end),
        );
        KeyGap {
            own: match self.selected {
                Some((_, _, projection)) => own.selected(projection),
                None => own,
            },
        }
    }

    fn fragment(&self, from: Boundary, to: usize) -> Fragment {
        let buffer = self.source.text();
        let written = buffer[from.0..to.max(from.0)].trim_matches(super::is_space);
        let text = written
            .strip_prefix(['?', '-'])
            .filter(|rest| rest.starts_with(super::is_space))
            .map_or(written, |rest| rest.trim_start_matches(super::is_space));
        let start = text.as_ptr() as usize - buffer.as_ptr() as usize;
        let fragment = Fragment::observe(buffer, start, start + text.len());
        self.selected.map_or(fragment, |(edits, _, _)| {
            Fragment::observe(buffer, start, start + text.len()).selected(buffer, edits)
        })
    }

    pub(super) fn entry(&self, from: Boundary, node: usize) -> Fragment {
        self.fragment(from, self.sites[node].end).for_node(node)
    }
    pub(super) fn property_width(&self, from: Boundary, node: usize) -> usize {
        let properties = self.properties(from, node);
        properties.characters() + usize::from(!properties.empty())
    }

    pub(super) fn properties(&self, from: Boundary, node: usize) -> Fragment {
        self.fragment(from, self.sites[node].start)
    }
    pub(super) fn has_comment(&self, node: usize) -> bool {
        self.comments
            .between(self.sites[node].start, self.sites[node].end)
    }

    fn line_start(&self, at: usize) -> usize {
        self.source.text()[..at]
            .rfind(['\n', '\r'])
            .map_or(0, |byte| byte + 1)
    }
    fn line_end(&self, at: usize) -> usize {
        self.source.text()[at..]
            .find(['\n', '\r'])
            .map_or(self.source.text().len(), |byte| at + byte)
    }

    pub(super) fn flow_site(&self, node: usize, last: usize) -> FlowSite<'a> {
        let buffer = self.source.text();
        let end = self.sites[last].end;
        let start = buffer[..self.sites[node].start]
            .trim_end_matches(super::is_space)
            .len();
        let line_start = self.line_start(start);
        let indicators = token_spacing::retained_indicators(
            self.source,
            line_start..self.line_end(line_start),
        );
        let (_, _, projection) = self
            .selected
            .expect("a block collection turns flow only once its edits are selected");
        let indicators = indicators.selected(projection);
        FlowSite {
            start,
            end,
            comments: self.comments.between(start, self.line_end(end))
                || self.comments.touches(start),
            indicators,
        }
    }

    /// The leading edit boundary, retaining standalone properties.
    pub(super) fn lead_start(&self, node: usize) -> usize {
        let start = self.properties_start(node);
        let before = self.source.text()[..start]
            .trim_end_matches(super::is_space)
            .len();
        if start < self.sites[node].start
            && self.line_start(start) > self.line_start(before)
        {
            return start;
        }
        self.selected
            .and_then(|(edits, _, _)| {
                let index = edits.partition_point(|(_, to, _)| to.get() < before);
                edits.get(index).filter(|(from, _, replacement)| {
                    from.get() <= before && replacement.len() == 0
                })
            })
            .map_or(before, |_| start)
    }

    pub(super) fn start_of(&self, node: usize) -> usize {
        self.sites[node].start
    }

    /// The characters `node` is written with in the source.
    pub(super) fn written_width(&self, node: usize) -> usize {
        let site = &self.sites[node];
        self.source.text()[site.start..site.end].chars().count()
    }

    /// Where `node`'s anchor and tag start, else the node itself.
    pub(super) fn properties_start(&self, node: usize) -> usize {
        let buffer = self.source.text();
        let mut start = self.sites[node].start;
        loop {
            let trimmed = buffer[..start].trim_end_matches(super::is_space);
            let token = trimmed.rfind(super::is_space).map_or(0, |at| at + 1);
            let word = &buffer[token..trimmed.len()];
            // A property can follow a flow indicator unspaced, as in `[1,&a {`, but
            // not one inside a verbatim tag such as `!<tag:yaml.org,2002:map>`, nor
            // end a key such as `"e !-":`.
            let verbatim = word.strip_suffix('>').and_then(|_| word.rfind("!<"));
            let property = word[..verbatim.unwrap_or(word.len())]
                .rfind(['[', '{', ','])
                .map_or(0, |at| at + 1);
            if !word[property..].starts_with(['&', '!']) || word.ends_with(':') {
                return start;
            }
            start = token + property;
        }
    }

    pub(super) fn starts_below(&self, node: usize, from: Boundary) -> bool {
        self.line_start(self.properties_start(node)) > self.line_start(from.0)
    }

    pub(super) fn opens_below(&self, node: usize, from: Boundary) -> bool {
        self.line_start(self.sites[node].start) > self.line_start(from.0)
    }

    pub(super) fn placement(
        &self,
        node: usize,
        column: usize,
        (base, line): (usize, usize),
    ) -> Placement {
        let before = self.lead_start(node);
        let text = self.source.text();
        let lead = column > line;
        Placement {
            before,
            column,
            base,
            line,
            // Re-indenting puts entries after `- - [` no deeper than the inner `-`.
            nested_dash: lead
                && text[..before].ends_with('-')
                && self.line_content(before) + 1 != before,
            lead,
        }
    }

    /// The first byte of the line holding `at` past its indentation.
    pub(super) fn line_content(&self, at: usize) -> usize {
        let start = self.line_start(at);
        start + self.source.text()[start..at].len()
            - self.source.text()[start..at]
                .trim_start_matches([' ', '\t'])
                .len()
    }

    /// Whether a comment sits in `node` or between it and what precedes it.
    pub(super) fn comment_inside(&self, node: usize) -> bool {
        let start = self.lead_start(node);
        self.comments.between(start, self.sites[node].end)
            || self.comments.touches(start)
    }

    /// The end of the line `node` ends on.
    pub(super) fn line_end_of(&self, node: usize) -> Boundary {
        Boundary(self.line_end(self.sites[node].end))
    }

    /// Whether a comment follows `node` on its last line.
    pub(super) fn trailing_comment(&self, node: usize) -> bool {
        let end = self.sites[node].end;
        self.comments.between(end, self.line_end(end))
    }

    pub(super) fn text(&self) -> &'a str {
        self.source.text()
    }

    /// Whether a line break separates flow collection `node`'s brackets and `entries`,
    /// each given by its first and last node, outside the entries themselves.
    pub(super) fn breaks_between(
        &self,
        node: usize,
        entries: impl Iterator<Item = (usize, usize)>,
    ) -> bool {
        let starts = self.source.byte_line_starts();
        let line = |at: usize| starts.partition_point(|&start| start <= at);
        let site = &self.sites[node];
        let mut previous = line(site.start);
        for (first, last) in entries {
            if line(self.sites[first].start) != previous {
                return true;
            }
            previous = line(self.sites[last].end);
        }
        line(site.end - 1) != previous
    }

    pub(super) fn spans_lines(&self, node: usize) -> bool {
        let site = &self.sites[node];
        self.fragment(Boundary(site.start), site.end).multiline()
    }

    pub(super) fn outer(&self, node: usize) -> Outer<'a> {
        let buffer = self.source.text();
        let site = &self.sites[node];
        let line_start = self.line_start(site.start);
        let prefix = hyphens::collection_prefix(&buffer[line_start..site.start]);
        let comment =
            comments::Trailing::observe(buffer, site.end, self.line_end(site.end));
        let fragment = comment
            .range()
            .map(|range| Fragment::observe(buffer, range.start, range.end));
        let indicators = token_spacing::retained_indicators(
            self.source,
            line_start..line_start + prefix.outer(),
        );
        let indicators = if let Some((_, _, projection)) = self.selected {
            indicators.selected(projection)
        } else {
            indicators
        };
        let ytt_directive = comment.range().is_some_and(|range| {
            comments::is_ytt_directive(&buffer[range.start + 1..range.end])
        });
        Outer {
            prefix,
            comment,
            ytt_directive,
            fragment,
            extent: self.fragment(Boundary(site.start), site.end),
            indicators,
        }
    }

    fn edit_end(&self, node: usize, outer: &Outer<'_>) -> usize {
        if outer.comment.present() {
            self.line_end(self.sites[node].end)
        } else {
            self.sites[node].end
        }
    }

    pub(super) fn compact_edit(&self, node: usize, outer: &Outer<'_>) -> EditSite {
        let start = if outer.prefix.dashes() == 0 {
            self.sites[node].start
        } else {
            self.line_start(self.sites[node].start) + outer.prefix.dashes()
        };
        EditSite::new(start, self.edit_end(node, outer))
    }

    pub(super) fn block_edit(&self, node: usize, outer: &Outer<'_>) -> EditSite {
        let start = self.line_start(self.sites[node].start);
        let prefix = &self.source.text()[start..self.sites[node].start];
        EditSite::new(
            start + prefix.trim_end_matches(super::is_space).len(),
            self.edit_end(node, outer),
        )
    }
}

#[derive(Clone)]
pub(super) struct Site {
    pub(super) scalar: Option<ScalarStyle>,
    pub(super) flow: bool,
    pub(super) braced: bool,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) line: usize,
    pub(super) column: usize,
    explicit_key: bool,
}

pub(super) struct Tree {
    pub(super) nodes: Vec<Node>,
    pub(super) sites: Vec<Site>,
    pub(super) comments: Attachments,
}

pub(super) fn observe(input: Analysis<'_>) -> Tree {
    let buffer = input.text();
    let mut key_indicators = input
        .tokens()
        .iter(buffer)
        .filter_map(|(span, kind)| matches!(kind, LexicalKind::Key).then_some(span))
        .peekable();
    let mut nodes: Vec<Node> = Vec::new();
    let mut sites: Vec<Site> = Vec::new();
    let mut comments = Attachments::default();
    let mut open: Vec<usize> = Vec::new();
    for (event, span) in input.event_iter() {
        let (start, end) = (
            marker_byte_offset(span.start).get(),
            marker_byte_offset(span.end).get(),
        );
        let (kind, flow, scalar) = match event {
            Event::Comment(..) => {
                comments.record(span);
                continue;
            }
            Event::Scalar(_, style, ..) => (NodeKind::Scalar, false, Some(style)),
            Event::Alias(_) => (NodeKind::Alias, false, None),
            Event::SequenceStart(style, ..) => {
                (NodeKind::Sequence, style == StructureStyle::Flow, None)
            }
            Event::MappingStart(style, ..) => {
                (NodeKind::Mapping, style == StructureStyle::Flow, None)
            }
            Event::SequenceEnd | Event::MappingEnd => {
                let index = open.pop().expect("an end event closes an open collection");
                let last = nodes[index]
                    .children()
                    .last()
                    .map(|&child| sites[child].end);
                let site = &mut sites[index];
                site.end = if site.braced {
                    end
                } else {
                    last.unwrap_or(site.start)
                };
                continue;
            }
            _ => continue,
        };
        let index = nodes.len();
        let parent = open.last().copied();
        // An indentless sequence's event starts after its first dash.
        let start = if kind == NodeKind::Sequence
            && !flow
            && parent.is_some_and(|parent| nodes[parent].kind() == NodeKind::Mapping)
        {
            buffer[..start]
                .trim_end_matches([' ', '\t'])
                .strip_suffix('-')
                .map_or(start, str::len)
        } else {
            start
        };
        let position = parent.map_or(0, |parent| nodes[parent].children().len());
        let explicit_key = parent.is_some_and(|parent| {
            nodes[parent].kind() == NodeKind::Mapping && position.is_multiple_of(2)
        }) && key_indicators
            .next_if(|span| marker_byte_offset(span.start).get() <= start)
            .is_some_and(|span| span.slice(buffer) == Some("?"));
        if let Some(parent) = parent {
            nodes[parent].push_child(index);
        }
        nodes.push(Node::new(kind, parent, position));
        sites.push(Site {
            scalar,
            flow,
            braced: flow && end > start && buffer[start..].starts_with(['[', '{']),
            start,
            end,
            line: span.start.line(),
            column: span.start.col() - (marker_byte_offset(span.start).get() - start),
            explicit_key,
        });
        if matches!(kind, NodeKind::Sequence | NodeKind::Mapping) {
            open.push(index);
        }
    }
    Tree {
        nodes,
        sites,
        comments,
    }
}
