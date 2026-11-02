use std::cell::{Cell, OnceCell};

use crate::config_schema::{CollectionStyleTarget, ExpandTarget};
use crate::rules::{indentation, quoted_strings};
use crate::text_version::model::{Container, Node, NodeKind as Kind};

use super::Shape;
use super::input::{Boundary, Initial, Placement};
use super::spelling::{Form, Fragment, Recipe, Selected, Structure};

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
        }
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
                [key, value] => {
                    let kind = self.nodes[key].kind();
                    if matches!(kind, Kind::Sequence | Kind::Mapping) {
                        return None;
                    }
                    let key_width = self.copy(key, pos).ok()?.characters()
                        + usize::from(kind == Kind::Alias);
                    let gap = self.initial.key_gap(key);
                    if gap.own().present() {
                        key_width
                            + 1
                            + self.measure_member(
                                value,
                                gap.flow_start(),
                                true,
                                facts,
                            )?
                    } else {
                        key_width
                    }
                }
                _ => self.measure_member(members[0], pos, false, facts)?,
            };
            pos = self.initial.after_entry(members[members.len() - 1]);
        }
        Some(characters)
    }

    fn measure_member(
        &self,
        index: usize,
        pos: Boundary,
        value: bool,
        facts: &[Facts],
    ) -> Option<usize> {
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
        self.initial.collection(index).braced()
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
            && self.initial.comment_near(index)
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
        if self.initial.comment_near(index) {
            return None;
        }
        let properties =
            single(self.initial.properties(place.boundary(), index)).ok()?;
        let start = place.column
            + usize::from(place.lead)
            + properties.characters()
            + usize::from(!properties.empty());
        let end = self.one(index).map(|characters| start + characters);
        if self.target(node.kind()) == Some(CollectionStyleTarget::Flow)
            && end.is_some_and(|end| end > self.width)
        {
            return self
                .outermost(index, Some(self.body_column(index, place)))
                .ok()
                .map(|block| (block, false));
        }
        if !self.governed(index) {
            return None;
        }
        let expanded = self.expands(index, end);
        if expanded && place.nested_dash
            || !expanded && !self.initial.spans_lines(index)
        {
            return None;
        }
        let value = node
            .parent()
            .is_some_and(|parent| self.nodes[parent].kind() == Kind::Mapping);
        let mut recipe = Recipe::new();
        if expanded && value {
            recipe.line(place.base + self.indent);
        } else if place.lead {
            recipe.text(" ");
        }
        if !properties.empty() {
            recipe.copy(properties);
            recipe.text(" ");
        }
        let entries =
            expanded.then(|| place.base + self.indent * (1 + usize::from(value)));
        recipe.append(self.spell(index, entries)?);
        let selected = place.edit(self.initial.end(index)).select(recipe);
        (!selected.written(self.initial.text())).then_some((selected, true))
    }

    /// The flow collection `index` on one line, or one entry per line at column
    /// `entries`; `None` where [`Self::one`] has no width for a one-line form.
    fn spell(&self, index: usize, entries: Option<usize>) -> Option<Recipe> {
        let node = &self.nodes[index];
        let mapping = node.kind() == Kind::Mapping;
        let (container, open, close) = if mapping {
            (Container::Mapping, "{", "}")
        } else {
            (Container::Sequence, "[", "]")
        };
        let mut recipe = Recipe::new();
        recipe.structure(Structure::Start {
            node: index,
            kind: container,
            form: Form::Flow,
        });
        recipe.text(open);
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
                [key, value] => self.pair(key, value, pos, entries, &mut recipe)?,
                _ => self.member(members[0], pos, (entries, 1, false), &mut recipe)?,
            }
            if entries.is_some() {
                recipe.text(",");
            }
            pos = self.initial.after_entry(members[members.len() - 1]);
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
        Some(recipe)
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
        let alias = kind == Kind::Alias;
        let width = key_text.characters() + usize::from(alias);
        recipe.structure(Structure::Key(key));
        recipe.copy(key_text);
        if alias {
            recipe.text(" ");
        }
        let gap = self.initial.key_gap(key);
        if !gap.own().present() {
            return Some(());
        }
        recipe.structure(Structure::Value(value));
        recipe.text(":");
        self.member(value, gap.flow_start(), (entries, width + 2, true), recipe)
    }

    /// Appends `index`, a sequence entry or (`value`) a mapping value, on a line holding
    /// `prefix` characters besides it and its properties from column `entries`.
    fn member(
        &self,
        index: usize,
        pos: Boundary,
        (entries, prefix, value): (Option<usize>, usize, bool),
        recipe: &mut Recipe,
    ) -> Option<()> {
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
        let lead = usize::from(value)
            + properties.characters()
            + usize::from(!properties.empty());
        let expanded = entries.filter(|&column| {
            let end = self
                .one(index)
                .map(|characters| column + prefix + lead + characters);
            self.expands(index, end)
        });
        match expanded {
            Some(column) if value => recipe.line(column + self.indent),
            None if value => recipe.text(" "),
            _ => {}
        }
        if !properties.empty() {
            recipe.copy(properties);
            recipe.text(" ");
        }
        let entries =
            expanded.map(|column| column + self.indent * (1 + usize::from(value)));
        recipe.append(self.spell(index, entries)?);
        Some(())
    }

    fn target(&self, kind: Kind) -> Option<CollectionStyleTarget> {
        if kind == Kind::Sequence {
            self.sequences
        } else {
            self.mappings
        }
    }

    fn to_block(&self, index: usize) -> bool {
        self.initial.collection(index).braced()
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
        if self.is_key(index) {
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
        if self.expand_target(Kind::Mapping) == ExpandTarget::Always {
            return None;
        }
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
        let initial =
            match self.nodes[index].kind() {
                Kind::Scalar => [Some(self.initial), source]
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

    /// The final column a block body converted from the outermost collection `index`
    /// starts at.
    fn body_column(&self, index: usize, place: &Placement) -> usize {
        let properties = self.initial.properties(place.boundary(), index);
        match self.nodes[index]
            .parent()
            .map(|parent| self.nodes[parent].kind())
        {
            // Re-indenting puts a body after an anchor and a tag at their column.
            _ if properties.paired(self.initial.text()) => {
                place.column + usize::from(place.lead)
            }
            Some(Kind::Mapping) => place.base + self.body_indent(index),
            Some(_) => self.item_column(index, place.column - 1),
            None => 0,
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
        if self.nodes[index].kind() == Kind::Sequence && !self.shape.indent_sequences {
            0
        } else {
            self.indent
        }
    }

    /// The column a block body converted from sequence item `index` starts at, after
    /// its `-` at `dash`.
    fn item_column(&self, index: usize, dash: usize) -> usize {
        if self.nodes[index].kind() == Kind::Mapping && self.shape.dash_on_own_line {
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
            let body = self.block(index, self.initial.open(index), col)?;
            let mut recipe = Recipe::new();
            if dashes != 0 {
                recipe.text(" ");
            }
            recipe.append(body);
            return Ok(self.initial.compact_edit(index, &outer).select(recipe));
        }
        let col = node.parent().map_or(0, |parent| {
            self.initial.column(parent).characters() + self.indent
        });
        self.shift_to(column, col);
        let body = self.block(index, self.initial.open(index), col)?;
        let mut recipe = Recipe::new();
        if let Some(comment) = outer.take_comment() {
            recipe.comment_gap(&comment);
            recipe.copy(comment);
        }
        recipe.line(col);
        recipe.append(body);
        Ok(self.initial.block_edit(index, &outer).select(recipe))
    }

    fn block(
        &self,
        index: usize,
        mut pos: Boundary,
        col: usize,
    ) -> Result<Recipe, &'static str> {
        let node = &self.nodes[index];
        if self.initial.has_comment(index) {
            return Err("it holds a comment");
        }
        let mut recipe = Recipe::new();
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
                recipe.append(self.item(child, pos, (col, compact))?);
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
                let colon =
                    col + key_text.characters() + usize::from(kind == Kind::Alias) + 1;
                let value_text = self.value(value, gap.block_start(), (col, colon))?;
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
                recipe.append(value_text);
                pos = self.initial.after_value(value, &gap);
            }
        }
        recipe.structure(Structure::End {
            node: index,
            kind: container,
            form: Form::Block,
        });
        Ok(recipe)
    }

    /// The sequence item `index` after its `-` at column `dash`, `compact` after another
    /// `-` on the same line.
    fn item(
        &self,
        index: usize,
        pos: Boundary,
        (dash, compact): (usize, bool),
    ) -> Result<Recipe, &'static str> {
        let site = self.initial.collection(index);
        let col = self.item_column(index, dash);
        if site.flow() && !site.braced() {
            return self.block(index, pos, col);
        }
        let start = self.at(dash) + 2 + self.initial.property_width(pos, index);
        if !self.to_block(index) && !self.splits(index, start) {
            let mut recipe = Recipe::new();
            if !self.respaced(index) {
                recipe.copy(self.copy(index, pos)?);
            } else if compact
                && self.expands(index, self.one(index).map(|width| start + width))
            {
                return Err("a nested collection would spread after a second `-`");
            } else {
                self.member(index, pos, (Some(self.at(dash)), 2, false), &mut recipe)
                    .ok_or("an entry spans lines")?;
            }
            return Ok(recipe);
        }
        let properties = single(self.initial.properties(pos, index))?;
        let body = self.block(index, self.initial.open(index), col)?;
        if properties.empty() {
            return Ok(body);
        }
        let mut recipe = Recipe::new();
        recipe.copy(properties);
        recipe.line(col);
        recipe.append(body);
        Ok(recipe)
    }

    /// The mapping value `index` of a key at column `col` whose `:` ends at `colon`.
    fn value(
        &self,
        index: usize,
        pos: Boundary,
        (col, colon): (usize, usize),
    ) -> Result<Recipe, &'static str> {
        let mut recipe = Recipe::new();
        if !self.to_block(index)
            && !self.splits(
                index,
                self.at(colon) + 1 + self.initial.property_width(pos, index),
            )
        {
            if self.respaced(index) {
                self.member(
                    index,
                    pos,
                    (Some(self.at(col)), colon - col, true),
                    &mut recipe,
                )
                .ok_or("an entry spans lines")?;
                return Ok(recipe);
            }
            let fragment = self.copy(index, pos)?;
            if fragment.empty() {
                recipe.copy(fragment);
            } else {
                recipe.space_copy(fragment);
            }
            return Ok(recipe);
        }
        let properties = single(self.initial.properties(pos, index))?;
        let inner = col + self.body_indent(index);
        let body = self.block(index, self.initial.open(index), inner)?;
        if !properties.empty() {
            recipe.space_copy(properties);
        }
        recipe.line(inner);
        recipe.append(body);
        Ok(recipe)
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
