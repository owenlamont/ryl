use crate::config_schema::CollectionStyleTarget;
use crate::rules::{indentation, quoted_strings};
use crate::text_version::model::{Container, Node, NodeKind as Kind};

use super::input::{Boundary, Initial};
use super::spelling::{Form, Fragment, Recipe, Selected, Structure};

pub(super) struct Planner<'a, 'text> {
    nodes: &'a [Node],
    initial: Initial<'a>,
    geometry: &'a indentation::CollectionGeometry<'text>,
    sequences: Option<CollectionStyleTarget>,
    mappings: Option<CollectionStyleTarget>,
    indent: usize,
    width: usize,
}

impl<'a, 'text> Planner<'a, 'text> {
    pub(super) const fn new(
        nodes: &'a [Node],
        initial: Initial<'a>,
        geometry: &'a indentation::CollectionGeometry<'text>,
        sequences: Option<CollectionStyleTarget>,
        mappings: Option<CollectionStyleTarget>,
        indent: usize,
        width: usize,
    ) -> Self {
        Self {
            nodes,
            initial,
            geometry,
            sequences,
            mappings,
            indent,
            width,
        }
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
            || self.is_key(index)
        {
            return None;
        }
        let site = self.initial.flow_site(index, *node.children().last()?);
        if !site.clear() {
            return None;
        }
        let (body, characters) = self.flow_body(index)?;
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
        let (body, characters) = self.flow_body(index)?;
        (column + characters <= self.width).then_some(body)
    }

    fn flow_body(&self, index: usize) -> Option<(Recipe, usize)> {
        let node = &self.nodes[index];
        let mut pos = self.initial.start(index);
        let mut recipe = Recipe::new();
        let mut characters = 2;
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
                let fragment = self.flow_entry(child, pos)?;
                characters += fragment.characters();
                if entry == 0 {
                    recipe.copy(fragment);
                } else {
                    characters += 2;
                    recipe.comma_copy(fragment);
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
                let key_text = self.flow_entry(key, pos)?;
                let gap = self.initial.key_gap(key);
                let value_text = self.flow_entry(value, gap.flow_start())?;
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

    fn flow_entry(&self, index: usize, pos: Boundary) -> Option<Fragment> {
        let fits = match self.nodes[index].kind() {
            Kind::Scalar => self
                .initial
                .scalar(index)
                .is_some_and(quoted_strings::Retained::flow_compatible),
            Kind::Alias => true,
            _ => false,
        };
        fits.then(|| single(self.initial.entry(pos, index)).ok())
            .flatten()
    }

    pub(super) fn outermost(&self, index: usize) -> Result<Selected, &'static str> {
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
                recipe.append(self.item(child, pos, col + 2)?);
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
                let value_text = self.value(value, gap.block_start(), col)?;
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

    fn item(
        &self,
        index: usize,
        pos: Boundary,
        col: usize,
    ) -> Result<Recipe, &'static str> {
        let site = self.initial.collection(index);
        if site.flow() && !site.braced() {
            return self.block(index, pos, col);
        }
        if !self.to_block(index) {
            let mut recipe = Recipe::new();
            recipe.copy(self.copy(index, pos)?);
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

    fn value(
        &self,
        index: usize,
        pos: Boundary,
        col: usize,
    ) -> Result<Recipe, &'static str> {
        let mut recipe = Recipe::new();
        if !self.to_block(index) {
            let fragment = self.copy(index, pos)?;
            if fragment.empty() {
                recipe.copy(fragment);
            } else {
                recipe.space_copy(fragment);
            }
            return Ok(recipe);
        }
        let properties = single(self.initial.properties(pos, index))?;
        let inner = col + self.indent;
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
