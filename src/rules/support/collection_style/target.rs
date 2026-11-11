use std::cell::{Cell, OnceCell};

use crate::config_schema::{CollectionStyleTarget, ExpandTarget};
use crate::rules::{indentation, quoted_strings};
use crate::text_version::model::{Container, Node, NodeKind as Kind};

use super::input::{Boundary, Initial, Placement};
use super::spelling::{Form, Fragment, Recipe, Selected, Structure};
use super::{BlockSpellings, Shape};

pub(super) struct Planner<'a> {
    nodes: &'a [Node],
    initial: Initial<'a>,
    geometry: &'a indentation::CollectionGeometry,
    sequences: Option<CollectionStyleTarget>,
    mappings: Option<CollectionStyleTarget>,
    indent: usize,
    width: usize,
    shape: Shape,
    facts: OnceCell<Vec<Facts>>,
    shift: Cell<isize>,
    block: Option<&'a BlockSpellings>,
    protected: &'a [bool],
    flow: Option<&'a quoted_strings::FlowSpellings>,
}

/// Where a flow collection's line breaks go.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fit {
    /// On one line where it starts.
    Inline,
    /// On one line, alone on the line after its key.
    NextLine,
    /// One entry per line.
    Expanded,
}

/// A sequence entry or (`value`) mapping value on a line starting at column `entries`,
/// after `prefix` characters and before a `comma`; `entries` is `None` on one line.
#[derive(Clone, Copy)]
struct Member {
    entries: Option<usize>,
    prefix: usize,
    value: bool,
    comma: bool,
}

/// What [`Planner`] measures of each node once, bottom-up.
#[derive(Clone, Copy, Default)]
struct Facts {
    width: Option<usize>,
    governed: bool,
}

impl<'a> Planner<'a> {
    pub(super) const fn new(
        nodes: &'a [Node],
        initial: Initial<'a>,
        geometry: &'a indentation::CollectionGeometry,
        sequences: Option<CollectionStyleTarget>,
        mappings: Option<CollectionStyleTarget>,
        (indent, width): (usize, usize),
        shape: Shape,
    ) -> Self {
        Self {
            nodes,
            initial,
            geometry,
            sequences,
            mappings,
            indent,
            width,
            shape,
            facts: OnceCell::new(),
            shift: Cell::new(0),
            block: None,
            protected: &[],
            flow: None,
        }
    }

    pub(super) const fn with_block_spellings(
        mut self,
        block: &'a BlockSpellings,
    ) -> Self {
        self.block = Some(block);
        self
    }

    pub(super) const fn with_protection(mut self, protected: &'a [bool]) -> Self {
        self.protected = protected;
        self
    }

    pub(super) const fn with_flow_spellings(
        mut self,
        flow: Option<&'a quoted_strings::FlowSpellings>,
    ) -> Self {
        self.flow = flow;
        self
    }

    fn allowed(&self, index: usize) -> bool {
        self.protected.get(index).is_none_or(|protected| !protected)
    }

    /// The width of `key`, copied from `pos`, once its mapping is in block style.
    fn block_key_width(&self, key: usize, pos: Boundary, copied: &Fragment) -> usize {
        let Some(block) = self.block else {
            return copied.characters();
        };
        let written = self.initial.unselected().entry(pos, key).characters();
        let scalar = self.initial.written_width(key);
        written - scalar + block.width(self.initial.start_of(key)).unwrap_or(scalar)
    }

    fn expand_target(&self, kind: Kind) -> ExpandTarget {
        if kind == Kind::Sequence {
            self.shape.sequences
        } else {
            self.shape.mappings
        }
    }

    fn facts(&self, index: usize) -> Facts {
        self.facts.get_or_init(|| {
            let mut facts = vec![Facts::default(); self.nodes.len()];
            // Nodes are in document order, so a reverse sweep sees children first.
            for index in (0..self.nodes.len()).rev() {
                facts[index] = self.measure(index, &facts);
            }
            facts
        })[index]
    }

    /// Whether a collection in `index`'s subtree has a non-`preserve` line-break target.
    fn governed(&self, index: usize) -> bool {
        self.facts(index).governed
    }

    /// The width of flow collection `index` on one line, `None` where an entry cannot be
    /// copied or a nested collection must spread over lines.
    fn one(&self, index: usize) -> Option<usize> {
        self.facts(index).width
    }

    fn measure(&self, index: usize, facts: &[Facts]) -> Facts {
        let node = &self.nodes[index];
        if !matches!(node.kind(), Kind::Sequence | Kind::Mapping) {
            return Facts::default();
        }
        let governed = self.expand_target(node.kind()) != ExpandTarget::Preserve
            || node.children().iter().any(|&child| facts[child].governed);
        let width = (self.initial.collection(index).braced()
            && !node.children().is_empty())
        .then(|| self.measure_entries(index, facts))
        .flatten();
        Facts { width, governed }
    }

    fn measure_entries(&self, index: usize, facts: &[Facts]) -> Option<usize> {
        let node = &self.nodes[index];
        let mapping = node.kind() == Kind::Mapping;
        let step = 1 + usize::from(mapping);
        let mut characters = 2 * (node.children().len() / step)
            + if mapping { 2 * self.shape.padding } else { 0 };
        let mut pos = self.initial.open(index);
        for members in node.children().chunks(step) {
            characters += match *members {
                [key, value] => self.measure_pair(key, value, pos, facts)?,
                _ => self.measure_member(members[0], pos, false, facts)?,
            };
            pos = self.next_entry(members);
        }
        Some(characters)
    }

    fn key_needs_separator(&self, key: usize, text: &Fragment) -> bool {
        let empty = self
            .initial
            .scalar(key)
            .is_some_and(quoted_strings::Retained::empty);
        (self.nodes[key].kind() == Kind::Alias || empty && !text.empty())
            && self.initial.key_gap(key).own().present()
    }

    fn measure_pair(
        &self,
        key: usize,
        value: usize,
        pos: Boundary,
        facts: &[Facts],
    ) -> Option<usize> {
        let kind = self.nodes[key].kind();
        if matches!(kind, Kind::Sequence | Kind::Mapping) {
            return None;
        }
        let key_text = self.copy(key, pos).ok()?;
        let key_width = key_text.characters()
            + usize::from(self.key_needs_separator(key, &key_text));
        let gap = self.initial.key_gap(key);
        if !gap.own().present() {
            return Some(key_width);
        }
        Some(
            key_width
                + 1
                + self.measure_member(value, gap.flow_start(), true, facts)?,
        )
    }

    /// The key and value of `index` where it is a `key: value` entry of a flow sequence.
    fn implicit_pair(&self, index: usize) -> Option<(usize, usize)> {
        let site = self.initial.collection(index);
        match *self.nodes[index].children() {
            [key, value] if site.flow() && !site.braced() => Some((key, value)),
            _ => None,
        }
    }

    fn measure_member(
        &self,
        index: usize,
        pos: Boundary,
        value: bool,
        facts: &[Facts],
    ) -> Option<usize> {
        if let Some((key, pair_value)) = self.implicit_pair(index) {
            return self.measure_pair(key, pair_value, pos, facts);
        }
        if !self.initial.collection(index).braced()
            || self.nodes[index].children().is_empty()
        {
            let fragment = self.copy(index, pos).ok()?;
            return Some(
                fragment.characters() + usize::from(value && !fragment.empty()),
            );
        }
        if self.forced(index) {
            return None;
        }
        let properties = single(self.initial.properties(pos, index)).ok()?;
        Some(
            usize::from(value)
                + properties.characters()
                + usize::from(!properties.empty())
                + facts[index].width?,
        )
    }

    /// Whether flow collection `index` is written with its entries on separate lines.
    fn spread(&self, index: usize) -> bool {
        let node = &self.nodes[index];
        let step = 1 + usize::from(node.kind() == Kind::Mapping);
        self.initial.breaks_between(
            index,
            node.children()
                .chunks(step)
                .map(|members| (members[0], members[members.len() - 1])),
        )
    }

    /// Whether the flow collection `index`, copied into a block conversion, is respelled
    /// for a line-break target in its subtree.
    fn respaced(&self, index: usize) -> bool {
        self.initial.collection(index).braced()
            && !self.nodes[index].children().is_empty()
            && self.governed(index)
    }

    /// Where the flow entry after `members`, a sequence entry or a key and its value,
    /// starts: past an empty value's `:` too.
    fn next_entry(&self, members: &[usize]) -> Boundary {
        match *members {
            [key, value] => {
                let gap = self.initial.key_gap(key);
                if gap.own().present() {
                    self.initial.after_value(value, &gap)
                } else {
                    self.initial.after_entry(value)
                }
            }
            _ => self.initial.after_entry(members[0]),
        }
    }

    /// Whether `index` spreads over lines wherever it sits.
    fn forced(&self, index: usize) -> bool {
        match self.expand_target(self.nodes[index].kind()) {
            ExpandTarget::Always => true,
            ExpandTarget::Preserve => self.spread(index),
            ExpandTarget::Auto | ExpandTarget::Never => false,
        }
    }

    /// Whether the outermost flow collection `index` may be respelled once its final
    /// position is known.
    fn restyle_candidate(&self, index: usize) -> bool {
        let node = &self.nodes[index];
        self.allowed(index)
            && self.initial.collection(index).braced()
            && !node.children().is_empty()
            && !node
                .parent()
                .is_some_and(|parent| self.initial.collection(parent).flow())
            && match self.target(node.kind()) {
                Some(CollectionStyleTarget::Flow) => true,
                Some(CollectionStyleTarget::Preserve) => self.governed(index),
                _ => false,
            }
    }

    /// Whether `index`, near a comment, is written in a form its line-break target
    /// rejects.
    pub(super) fn refuses_comment(&self, index: usize) -> bool {
        self.restyle_candidate(index)
            && !self.is_key(index)
            && (self.initial.comment_inside(index)
                || self.initial.trailing_comment(index)
                    && self.initial.spans_lines(index))
            && match self.expand_target(self.nodes[index].kind()) {
                ExpandTarget::Always => !self.initial.spans_lines(index),
                ExpandTarget::Never => self.initial.spans_lines(index),
                ExpandTarget::Auto | ExpandTarget::Preserve => false,
            }
    }

    /// Whether `index` spreads over lines, given the column its one-line form would end
    /// at, `None` where it cannot sit on one line.
    fn expands(&self, index: usize, end: Option<usize>) -> bool {
        let Some(end) = end else {
            return true;
        };
        match self.expand_target(self.nodes[index].kind()) {
            ExpandTarget::Always => true,
            ExpandTarget::Never => false,
            ExpandTarget::Auto => end > self.width,
            ExpandTarget::Preserve => self.spread(index),
        }
    }

    /// The outermost flow collection `index` respelled at its final `place`, and whether
    /// it stays in flow style: on one line or one entry per line, or in block style where
    /// a `flow` leaf overflows.
    pub(super) fn respelled(
        &self,
        index: usize,
        place: &Placement,
    ) -> Option<(Selected, bool)> {
        let node = &self.nodes[index];
        if self.initial.comment_inside(index) {
            return None;
        }
        let trailing = self.initial.trailing_comment(index);
        // Respelling must keep value comments off their keys and closing comments off their openers.
        if trailing
            && (self.initial.spans_lines(index)
                || self.initial.opens_below(index, place.boundary()))
        {
            return None;
        }
        let properties =
            single(self.initial.properties(place.boundary(), index)).ok()?;
        let property_width = properties.characters() + usize::from(!properties.empty());
        let inline = place.column + usize::from(place.lead) + property_width;
        let width = self.one(index);
        let end = width.map(|characters| inline + characters);
        let value = place.lead
            && node
                .parent()
                .is_some_and(|parent| self.nodes[parent].kind() == Kind::Mapping);
        // As in prettier, a trailing comment keeps `[` on the key line; the comment stays
        // on that line, after `[`, so it keeps its place among the data.
        let hugged = value && trailing;
        let next_line = width
            .filter(|_| value && !hugged)
            .map(|characters| place.base + self.indent + property_width + characters);
        let fit = self.fit(index, end, next_line);
        let below = fit != Fit::Inline && value && !hugged;
        let start = if below {
            place.base + self.indent + property_width
        } else {
            inline
        };
        if self.target(node.kind()) == Some(CollectionStyleTarget::Flow)
            && width.is_some_and(|width| start + width > self.width)
        {
            return self
                .outermost(index, Some(self.body_column(index, place)))
                .ok()
                .map(|block| (block, false));
        }
        if !self.governed(index) {
            return None;
        }
        // Re-indenting would put entries no deeper than a key after a `-` on its line.
        if fit == Fit::Expanded
            && (place.nested_dash
                || self.joins_dash(index)
                || hugged && place.line != place.base)
        {
            return None;
        }
        let mut recipe = Recipe::new();
        if below {
            recipe.line(place.base + self.indent);
        } else if place.lead {
            recipe.text(" ");
        }
        if !properties.empty() {
            recipe.copy(properties);
            recipe.text(" ");
        }
        let entries = (fit == Fit::Expanded).then(|| {
            if hugged {
                place.line + self.indent
            } else {
                place.base + self.indent * (1 + usize::from(value))
            }
        });
        let mut outer = self.initial.outer(index);
        let comment = (trailing && fit == Fit::Expanded)
            .then(|| outer.take_comment())
            .flatten();
        if comment.is_some() && outer.ytt_directive() {
            return None;
        }
        let end = if comment.is_some() {
            self.initial.line_end_of(index)
        } else {
            self.initial.end(index)
        };
        self.spell(index, (entries, comment), &mut recipe)?;
        let selected = place.edit(end).select(recipe);
        (!selected.written(self.initial.text())).then_some((selected, true))
    }

    /// Appends the flow collection `index` on one line, or one entry per line at column
    /// `entries`; `None` where [`Self::one`] has no width for a one-line form.
    fn spell(
        &self,
        index: usize,
        (entries, comment): (Option<usize>, Option<Fragment>),
        recipe: &mut Recipe,
    ) -> Option<()> {
        let node = &self.nodes[index];
        let mapping = node.kind() == Kind::Mapping;
        let (container, open, close) = if mapping {
            (Container::Mapping, "{", "}")
        } else {
            (Container::Sequence, "[", "]")
        };
        recipe.structure(Structure::Start {
            node: index,
            kind: container,
            form: Form::Flow,
        });
        recipe.text(open);
        if let Some(comment) = comment {
            recipe.comment_gap(&comment);
            recipe.copy(comment);
        }
        if entries.is_none() {
            recipe.flow_gap(index, container);
        }
        let mut pos = self.initial.open(index);
        for (entry, members) in
            node.children().chunks(1 + usize::from(mapping)).enumerate()
        {
            if let Some(column) = entries {
                recipe.line(column);
            } else if entry != 0 {
                recipe.text(", ");
            }
            match *members {
                [key, value] => self.pair(key, value, pos, entries, recipe)?,
                _ => self.member(
                    members[0],
                    pos,
                    Member {
                        entries,
                        prefix: 0,
                        value: false,
                        comma: true,
                    },
                    recipe,
                )?,
            }
            if entries.is_some() {
                recipe.text(",");
            }
            pos = self.next_entry(members);
        }
        if let Some(column) = entries {
            recipe.line(column - self.indent);
        } else {
            recipe.flow_gap(index, container);
        }
        recipe.structure(Structure::End {
            node: index,
            kind: container,
            form: Form::Flow,
        });
        recipe.text(close);
        Some(())
    }

    fn pair(
        &self,
        key: usize,
        value: usize,
        pos: Boundary,
        entries: Option<usize>,
        recipe: &mut Recipe,
    ) -> Option<()> {
        let kind = self.nodes[key].kind();
        if matches!(kind, Kind::Sequence | Kind::Mapping) {
            return None;
        }
        let key_text = self.copy(key, pos).ok()?;
        let separator = self.key_needs_separator(key, &key_text);
        let width = key_text.characters() + usize::from(separator);
        recipe.structure(Structure::Key(key));
        recipe.copy(key_text);
        if separator {
            recipe.text(" ");
        }
        let gap = self.initial.key_gap(key);
        if !gap.own().present() {
            return Some(());
        }
        recipe.structure(Structure::Value(value));
        recipe.text(":");
        let member = Member {
            entries,
            prefix: width + 1,
            value: true,
            comma: true,
        };
        self.member(value, gap.flow_start(), member, recipe)
    }

    /// How `index` is laid out, given the column its one-line form would end at after
    /// what precedes it and, for a mapping value, alone on the next line; `None` where
    /// it cannot sit on one line.
    fn fit(
        &self,
        index: usize,
        inline: Option<usize>,
        next_line: Option<usize>,
    ) -> Fit {
        if !self.expands(index, inline) {
            Fit::Inline
        } else if self.expand_target(self.nodes[index].kind()) == ExpandTarget::Auto
            && next_line.is_some_and(|end| end <= self.width)
        {
            Fit::NextLine
        } else {
            Fit::Expanded
        }
    }

    /// Appends `index`, a sequence entry or mapping value, per `member`.
    fn member(
        &self,
        index: usize,
        pos: Boundary,
        member: Member,
        recipe: &mut Recipe,
    ) -> Option<()> {
        let Member {
            entries,
            prefix,
            value,
            comma,
        } = member;
        if let Some((key, pair_value)) = self.implicit_pair(index) {
            return self.pair(key, pair_value, pos, entries, recipe);
        }
        if !self.initial.collection(index).braced()
            || self.nodes[index].children().is_empty()
        {
            let fragment = self.copy(index, pos).ok()?;
            if !value {
                recipe.copy(fragment);
            } else if !fragment.empty() {
                recipe.space_copy(fragment);
            }
            return Some(());
        }
        let properties = single(self.initial.properties(pos, index)).ok()?;
        let props = properties.characters() + usize::from(!properties.empty());
        let fit = entries.map_or(Fit::Inline, |column| {
            let one = self
                .one(index)
                .map(|width| width + props + usize::from(comma));
            self.fit(
                index,
                one.map(|width| column + prefix + usize::from(value) + width),
                one.filter(|_| value)
                    .map(|width| column + self.indent + width),
            )
        });
        let column = entries.unwrap_or(0);
        match fit {
            Fit::Inline if value => recipe.text(" "),
            Fit::NextLine | Fit::Expanded if value => recipe.line(column + self.indent),
            _ => {}
        }
        if !properties.empty() {
            recipe.copy(properties);
            recipe.text(" ");
        }
        let entries = (fit == Fit::Expanded)
            .then(|| column + self.indent * (1 + usize::from(value)));
        self.spell(index, (entries, None), recipe)
    }

    fn target(&self, kind: Kind) -> Option<CollectionStyleTarget> {
        if kind == Kind::Sequence {
            self.sequences
        } else {
            self.mappings
        }
    }

    fn to_block(&self, index: usize) -> bool {
        self.allowed(index)
            && self.initial.collection(index).braced()
            && !self.nodes[index].children().is_empty()
            && self.target(self.nodes[index].kind())
                == Some(CollectionStyleTarget::Block)
    }

    pub(super) fn outermost_candidate(&self, index: usize) -> bool {
        self.to_block(index)
            && !self.nodes[index]
                .parent()
                .is_some_and(|parent| self.initial.collection(parent).flow())
    }

    pub(super) fn pending(&self, index: usize) -> bool {
        let node = &self.nodes[index];
        if !self.allowed(index) || self.is_key(index) {
            return false;
        }
        self.outermost_candidate(index)
            || self.restyle_candidate(index)
            || (matches!(node.kind(), Kind::Sequence | Kind::Mapping)
                && !self.initial.collection(index).flow()
                && node.parent().is_some()
                && self.target(node.kind()) == Some(CollectionStyleTarget::Flow)
                && !node.children().is_empty()
                && node.children().iter().all(|&child| {
                    matches!(self.nodes[child].kind(), Kind::Scalar | Kind::Alias)
                }))
    }

    fn is_key(&self, index: usize) -> bool {
        self.nodes[index].parent().is_some_and(|parent| {
            self.nodes[parent].kind() == Kind::Mapping
                && self.nodes[index].position().is_multiple_of(2)
        })
    }

    pub(super) fn flowed(&self, index: usize) -> Option<Selected> {
        let node = &self.nodes[index];
        if self.initial.collection(index).flow()
            || node.parent().is_none()
            || self.target(node.kind()) != Some(CollectionStyleTarget::Flow)
            || self.expand_target(node.kind()) == ExpandTarget::Always
            || self.is_key(index)
        {
            return None;
        }
        let last = node
            .children()
            .last()
            .expect("a pending block collection has entries");
        let site = self.initial.flow_site(index, *last);
        if !site.clear() {
            return None;
        }
        let (body, characters) = self.flow_body(index, true)?;
        let mut recipe = Recipe::new();
        recipe.text(" ");
        recipe.append(body);
        {
            let width = self
                .geometry
                .width(site.position(), characters + 1)
                .characters();
            width <= self.width && !site.indicators().widened()
        }
        .then(|| site.edit().select(recipe))
    }

    pub(super) fn implicit_body(&self, index: usize, column: usize) -> Option<Recipe> {
        if !self.allowed(index)
            || self.expand_target(Kind::Mapping) == ExpandTarget::Always
        {
            return None;
        }
        // Measured on its own line under `dash-on-own-line`, the flow form sits after
        // `- ` instead, where `flowed` measures it on the next run.
        let column = if self.shape.dash_on_own_line {
            (column + 2).saturating_sub(self.indent)
        } else {
            column
        };
        let (body, characters) = self.flow_body(index, false)?;
        (column + characters <= self.width).then_some(body)
    }

    fn flow_body(&self, index: usize, written: bool) -> Option<(Recipe, usize)> {
        let node = &self.nodes[index];
        let mut pos = self.initial.start(index);
        let mut recipe = Recipe::new();
        let mut characters = 2;
        if node.kind() == Kind::Mapping {
            characters += 2 * self.shape.padding;
        }
        let container = if node.kind() == Kind::Sequence {
            Container::Sequence
        } else {
            Container::Mapping
        };
        recipe.structure(Structure::Start {
            node: index,
            kind: container,
            form: Form::Flow,
        });
        if node.kind() == Kind::Sequence {
            recipe.text("[");
            recipe.flow_gap(index, container);
            for (entry, &child) in node.children().iter().enumerate() {
                let fragment = self.flow_entry(child, pos, written)?;
                characters += fragment.characters();
                if entry == 0 {
                    recipe.copy(fragment);
                } else {
                    characters += 2;
                    recipe.text(", ");
                    recipe.copy(fragment);
                }
                pos = self.initial.end(child);
            }
            recipe.flow_gap(index, container);
            recipe.structure(Structure::End {
                node: index,
                kind: container,
                form: Form::Flow,
            });
            recipe.text("]");
        } else {
            recipe.text("{");
            recipe.flow_gap(index, container);
            for (entry, &[key, value]) in
                node.children().as_chunks::<2>().0.iter().enumerate()
            {
                let key_text = self.flow_entry(key, pos, written)?;
                let gap = self.initial.key_gap(key);
                let value_text = self.flow_entry(value, gap.flow_start(), written)?;
                let alias = self.nodes[key].kind() == Kind::Alias;
                characters += key_text.characters()
                    + value_text.characters()
                    + 2
                    + usize::from(alias);
                if entry != 0 {
                    characters += 2;
                    recipe.text(", ");
                }
                recipe.structure(Structure::Key(key));
                recipe.copy(key_text);
                if alias {
                    recipe.text(" ");
                }
                recipe.structure(Structure::Value(value));
                recipe.text(": ");
                recipe.copy(value_text);
                pos = self.initial.end(value);
            }
            recipe.flow_gap(index, container);
            recipe.structure(Structure::End {
                node: index,
                kind: container,
                form: Form::Flow,
            });
            recipe.text("}");
        }
        Some((recipe, characters))
    }

    /// A block entry copied into a flow collection, measured as the quote pass spells
    /// it there: as selected, or, for a `written` collection, as written where the
    /// selection is plain only in block context.
    fn flow_entry(
        &self,
        index: usize,
        pos: Boundary,
        written: bool,
    ) -> Option<Fragment> {
        let source = written.then(|| self.initial.unselected());
        let initial = match self.nodes[index].kind() {
            Kind::Scalar => [
                self.flow.map(|flow| self.initial.flow_spelled(flow)),
                Some(self.initial),
                source,
            ]
            .into_iter()
            .flatten()
            .find(|initial| {
                initial
                    .scalar(index)
                    .is_some_and(quoted_strings::Retained::flow_compatible)
            })?,
            Kind::Alias => self.initial,
            _ => return None,
        };
        single(initial.entry(pos, index)).ok()
    }

    /// The outermost collection `index` converted to block or respelled at its final
    /// `place`, and whether it stays in flow style.
    pub(super) fn placed(
        &self,
        index: usize,
        place: &Placement,
    ) -> Option<(Selected, bool)> {
        if self.outermost_candidate(index) {
            let column = self.body_column(index, place);
            return self
                .outermost(index, Some(column))
                .ok()
                .map(|recipe| (recipe, false));
        }
        if self.initial.collection(index).flow() {
            self.respelled(index, place)
        } else {
            None
        }
    }

    /// Whether `index`, keeping its `[` on its key's line for a trailing comment, is the
    /// first value of a mapping that formatting joins onto its `-` line.
    fn joins_dash(&self, index: usize) -> bool {
        let node = &self.nodes[index];
        node.position() == 1
            && !self.shape.dash_on_own_line
            && self.initial.trailing_comment(index)
            && node.parent().is_some_and(|mapping| {
                self.nodes[mapping].kind() == Kind::Mapping
                    && self.nodes[mapping].parent().is_some_and(|item| {
                        self.nodes[item].kind() == Kind::Sequence
                            && !self.initial.collection(item).flow()
                    })
            })
    }

    /// The final column a block body converted from the outermost collection `index`
    /// starts at.
    fn body_column(&self, index: usize, place: &Placement) -> usize {
        if !place.lead {
            return place.column;
        }
        let parent = self.nodes[index]
            .parent()
            .expect("a collection after a key or dash has a parent");
        match self.nodes[parent].kind() {
            Kind::Mapping => place.base + self.body_indent(index),
            Kind::Sequence
                if self.initial.starts_below(index, place.boundary())
                    || self.nodes[index].kind() == Kind::Sequence
                        && !self.initial.outer(index).prefix().compact()
                        && self.initial.properties_start(index)
                            == self.initial.start_of(index) =>
            {
                place.column - 1 + self.indent
            }
            _ => self.item_column(index, place.column - 1),
        }
    }

    /// Records how far the final `column`, where known, lies from the `provisional` one a
    /// block conversion's recipe writes, which re-indenting corrects.
    fn shift_to(&self, column: Option<usize>, provisional: usize) {
        self.shift.set(column.map_or(0, |column| {
            isize::try_from(column).expect("columns fit isize")
                - isize::try_from(provisional).expect("columns fit isize")
        }));
    }

    /// The final column of the provisional column `col` in the current conversion.
    fn at(&self, col: usize) -> usize {
        col.saturating_add_signed(self.shift.get())
    }

    /// How far a block body converted from `index` indents past its key.
    fn body_indent(&self, index: usize) -> usize {
        let node = &self.nodes[index];
        let explicit = node.parent().is_some_and(|parent| {
            self.nodes[parent].kind() == Kind::Mapping
                && !self.initial.collection(parent).flow()
                && !node.position().is_multiple_of(2)
                && self
                    .initial
                    .explicit_key(self.nodes[parent].children()[node.position() - 1])
        });
        if node.kind() == Kind::Sequence && !self.shape.indent_sequences && !explicit {
            0
        } else {
            self.indent
        }
    }

    /// The column a block body converted from sequence item `index` starts at, after
    /// its `-` at `dash`.
    fn item_column(&self, index: usize, dash: usize) -> usize {
        // Properties keep the item's body beside its dash, so `dash-on-own-line` cannot
        // move it.
        if self.nodes[index].kind() == Kind::Mapping
            && self.shape.dash_on_own_line
            && self.initial.properties_start(index) == self.initial.start_of(index)
        {
            dash + self.indent
        } else {
            dash + 2
        }
    }

    /// Whether the `flow` collection `index`, starting at `column`, overflows on one
    /// line.
    fn splits(&self, index: usize, column: usize) -> bool {
        self.target(self.nodes[index].kind()) == Some(CollectionStyleTarget::Flow)
            && self
                .one(index)
                .is_some_and(|characters| column + characters > self.width)
    }

    pub(super) fn outermost(
        &self,
        index: usize,
        column: Option<usize>,
    ) -> Result<Selected, &'static str> {
        let node = &self.nodes[index];
        if self.is_key(index) {
            return Err("it is a mapping key");
        }
        let mut outer = self.initial.outer(index);
        let keyed = node.parent().is_some_and(|parent| {
            self.nodes[parent].kind() == Kind::Mapping
                && self
                    .initial
                    .line(self.nodes[parent].children()[node.position() - 1])
                    .same_as(&self.initial.line(index))
        });
        if outer.ytt_directive() {
            return Err("a trailing ytt directive would bind to another node");
        }
        if outer.comment().present() && (!keyed || outer.extent().multiline()) {
            return Err("a trailing comment has no key line to move to");
        }
        if outer.indicators().widened() {
            return Err("an indicator before it has extra spaces");
        }
        if outer.prefix().compact() {
            let dashes = outer.prefix().dashes();
            let col = if dashes == 0 {
                outer.prefix().width()
            } else {
                dashes + 1
            };
            self.shift_to(column, col);
            let mut recipe = Recipe::new();
            if dashes != 0 {
                recipe.text(" ");
            }
            self.block(index, self.initial.open(index), col, &mut recipe)?;
            return Ok(self.initial.compact_edit(index, &outer).select(recipe));
        }
        let col = node.parent().map_or(0, |parent| {
            self.initial.column(parent).characters()
                + if self.nodes[parent].kind() == Kind::Mapping {
                    self.body_indent(index)
                } else {
                    self.indent
                }
        });
        self.shift_to(column, col);
        let mut recipe = Recipe::new();
        if let Some(comment) = outer.take_comment() {
            recipe.comment_gap(&comment);
            recipe.copy(comment);
        }
        recipe.line(col);
        self.block(index, self.initial.open(index), col, &mut recipe)?;
        Ok(self.initial.block_edit(index, &outer).select(recipe))
    }

    /// Appends `index` in block style at column `col`.
    fn block(
        &self,
        index: usize,
        mut pos: Boundary,
        col: usize,
        recipe: &mut Recipe,
    ) -> Result<(), &'static str> {
        let node = &self.nodes[index];
        if self.initial.has_comment(index) {
            return Err("it holds a comment");
        }
        let container = if node.kind() == Kind::Sequence {
            Container::Sequence
        } else {
            Container::Mapping
        };
        recipe.structure(Structure::Start {
            node: index,
            kind: container,
            form: Form::Block,
        });
        if node.kind() == Kind::Sequence {
            for (entry, &child) in node.children().iter().enumerate() {
                if entry != 0 {
                    recipe.line(col);
                }
                recipe.structure(Structure::Entry(child));
                recipe.text("- ");
                let compact = entry == 0
                    && node.parent().is_some_and(|parent| {
                        self.nodes[parent].kind() == Kind::Sequence
                    });
                self.item(child, pos, (col, compact), recipe)?;
                pos = self.initial.after_entry(child);
            }
        } else {
            for (entry, &[key, value]) in
                node.children().as_chunks::<2>().0.iter().enumerate()
            {
                let kind = self.nodes[key].kind();
                if matches!(kind, Kind::Sequence | Kind::Mapping) {
                    return Err("a key is a collection");
                }
                if self
                    .initial
                    .scalar(key)
                    .is_some_and(quoted_strings::Retained::empty)
                {
                    return Err("a key is empty");
                }
                let key_text = self.copy(key, pos)?;
                if key_text.characters() > 1024 {
                    return Err("a key is longer than 1024 characters");
                }
                let gap = self.initial.key_gap(key);
                let key_column = || {
                    indentation::RetainedColumn::from_position(
                        col + self.initial.property_width(pos, key),
                    )
                };
                if self
                    .initial
                    .scalar(key)
                    .is_some_and(|scalar| !scalar.block_key_compatible(key_column))
                {
                    return Err("a plain key would become a document marker");
                }
                if self
                    .initial
                    .scalar(key)
                    .is_some_and(quoted_strings::Retained::is_plain)
                    && gap.own().present()
                    && !gap.own().separated()
                {
                    return Err("a plain key's `:` has no space after it");
                }
                let colon = col
                    + self.block_key_width(key, pos, &key_text)
                    + usize::from(kind == Kind::Alias)
                    + 1;
                if entry != 0 {
                    recipe.line(col);
                }
                recipe.structure(Structure::Key(key));
                recipe.copy(key_text);
                if kind == Kind::Alias {
                    recipe.text(" ");
                }
                recipe.structure(Structure::Value(value));
                recipe.text(":");
                self.value(value, gap.block_start(), (col, colon), recipe)?;
                pos = self.initial.after_value(value, &gap);
            }
        }
        recipe.structure(Structure::End {
            node: index,
            kind: container,
            form: Form::Block,
        });
        Ok(())
    }

    /// The sequence item `index` after its `-` at column `dash`, `compact` after another
    /// `-` on the same line.
    fn item(
        &self,
        index: usize,
        pos: Boundary,
        (dash, compact): (usize, bool),
        recipe: &mut Recipe,
    ) -> Result<(), &'static str> {
        let site = self.initial.collection(index);
        let col = self.item_column(index, dash);
        if site.flow() && !site.braced() {
            return self.block(index, pos, col, recipe);
        }
        let start = self.at(dash) + 2 + self.initial.property_width(pos, index);
        if !self.to_block(index) && !self.splits(index, start) {
            if !self.respaced(index) {
                recipe.copy(self.copy(index, pos)?);
            } else if compact
                && self.expands(index, self.one(index).map(|width| start + width))
            {
                return Err("a nested collection would spread after a second `-`");
            } else {
                self.member(
                    index,
                    pos,
                    Member {
                        entries: Some(self.at(dash)),
                        prefix: 2,
                        value: false,
                        comma: false,
                    },
                    recipe,
                )
                .ok_or("an entry spans lines")?;
            }
            return Ok(());
        }
        let properties = single(self.initial.properties(pos, index))?;
        if !properties.empty() {
            recipe.copy(properties);
            recipe.line(col);
        }
        self.block(index, self.initial.open(index), col, recipe)
    }

    /// The mapping value `index` of a key at column `col` whose `:` ends at `colon`.
    fn value(
        &self,
        index: usize,
        pos: Boundary,
        (col, colon): (usize, usize),
        recipe: &mut Recipe,
    ) -> Result<(), &'static str> {
        if !self.to_block(index)
            && !self.splits(
                index,
                self.at(colon) + 1 + self.initial.property_width(pos, index),
            )
        {
            if self.respaced(index) {
                return self
                    .member(
                        index,
                        pos,
                        Member {
                            entries: Some(self.at(col)),
                            prefix: colon - col,
                            value: true,
                            comma: false,
                        },
                        recipe,
                    )
                    .ok_or("an entry spans lines");
            }
            let fragment = self.copy(index, pos)?;
            if fragment.empty() {
                recipe.copy(fragment);
            } else {
                recipe.space_copy(fragment);
            }
            return Ok(());
        }
        let properties = single(self.initial.properties(pos, index))?;
        let inner = col + self.body_indent(index);
        if !properties.empty() {
            recipe.space_copy(properties);
        }
        recipe.line(inner);
        self.block(index, self.initial.open(index), inner, recipe)
    }

    fn copy(&self, index: usize, pos: Boundary) -> Result<Fragment, &'static str> {
        if self
            .initial
            .scalar(index)
            .is_some_and(quoted_strings::Retained::starts_question)
        {
            return Err("an entry starts with `?`");
        }
        single(self.initial.entry(pos, index))
    }
}

fn single(fragment: Fragment) -> Result<Fragment, &'static str> {
    if fragment.multiline() {
        Err("an entry spans lines")
    } else {
        Ok(fragment)
    }
}

pub(super) const REINDENT_REFUSAL: &str = "the document cannot be re-indented safely";
