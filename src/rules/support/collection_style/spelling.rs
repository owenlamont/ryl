use crate::rules::support::span_utils::BytePos;
use crate::text_version::model::Container;

#[derive(Clone, Copy)]
pub(crate) enum Form {
    Block,
    Flow,
}

#[derive(Clone, Copy)]
pub(crate) enum Structure {
    Start {
        node: usize,
        kind: Container,
        form: Form,
    },
    End {
        node: usize,
        kind: Container,
        form: Form,
    },
    Entry(usize),
    Key(usize),
    Value(usize),
}

pub(crate) enum FragmentPart {
    Source {
        start: BytePos,
        end: BytePos,
        node: Option<usize>,
    },
    Text(&'static str),
    Spaces(usize),
    Break(usize),
    Structure(Structure),
}

struct Range {
    start: usize,
    end: usize,
    node: Option<usize>,
}

pub(super) struct Fragment {
    range: Range,
    characters: usize,
    multiline: bool,
    empty: bool,
}

impl Fragment {
    pub(super) fn observe(buffer: &str, start: usize, end: usize) -> Self {
        Self {
            range: Range {
                start,
                end,
                node: None,
            },
            characters: buffer[start..end].chars().count(),
            multiline: buffer[start..end].contains(['\n', '\r']),
            empty: start == end,
        }
    }

    pub(super) fn characters(&self) -> usize {
        self.characters
    }

    pub(super) fn multiline(&self) -> bool {
        self.multiline
    }

    pub(super) fn empty(&self) -> bool {
        self.empty
    }

    pub(super) fn selected(
        mut self,
        input: &str,
        edits: &[crate::format::layout::PlanEdit],
    ) -> Self {
        let mut at = self.range.start;
        let mut characters = 0;
        let mut multiline = false;
        let mut observe = |piece: crate::format::layout::Piece<'_>| {
            for ch in piece.chars() {
                characters += 1;
                multiline |= matches!(ch, '\n' | '\r');
            }
        };
        let first = edits.partition_point(|(_, end, _)| end.get() <= at);
        for (from, to, replacement) in edits[first..]
            .iter()
            .take_while(|(from, _, _)| from.get() < self.range.end)
        {
            observe(crate::format::layout::Piece::Text(&input[at..from.get()]));
            observe(replacement.piece());
            at = to.get();
        }
        observe(crate::format::layout::Piece::Text(
            &input[at..self.range.end],
        ));
        self.characters = characters;
        self.multiline = multiline;
        self.empty = characters == 0;
        self
    }

    pub(super) fn for_node(mut self, node: usize) -> Self {
        self.range.node = Some(node);
        self
    }
}

enum Part {
    FlowGap {
        node: usize,
        kind: Container,
        spaces: usize,
    },
    Text(&'static str),
    CommentGap {
        at: BytePos,
        spaces: usize,
    },
    Copy(Range),
    SpaceCopy(Range),
    CommaCopy(Range),
    Break(usize),
    Structure(Structure),
}

pub(super) struct Recipe {
    parts: Vec<Part>,
}

impl Recipe {
    pub(super) const fn new() -> Self {
        Self { parts: Vec::new() }
    }

    pub(super) fn text(&mut self, text: &'static str) {
        self.parts.push(Part::Text(text));
    }

    pub(super) fn comment_gap(&mut self, comment: &Fragment) {
        self.parts.push(Part::CommentGap {
            at: BytePos::new(comment.range.start),
            spaces: 2,
        });
    }

    pub(super) fn flow_gap(&mut self, node: usize, kind: Container) {
        self.parts.push(Part::FlowGap {
            node,
            kind,
            spaces: 0,
        });
    }

    pub(super) fn structure(&mut self, structure: Structure) {
        self.parts.push(Part::Structure(structure));
    }

    fn fragment(&mut self, fragment: Fragment, part: impl FnOnce(Range) -> Part) {
        self.parts.push(part(fragment.range));
    }

    pub(super) fn copy(&mut self, fragment: Fragment) {
        self.fragment(fragment, Part::Copy);
    }

    pub(super) fn space_copy(&mut self, fragment: Fragment) {
        self.fragment(fragment, Part::SpaceCopy);
    }

    pub(super) fn comma_copy(&mut self, fragment: Fragment) {
        self.fragment(fragment, Part::CommaCopy);
    }

    pub(super) fn line(&mut self, column: usize) {
        self.parts.push(Part::Break(column));
    }

    pub(super) fn append(&mut self, mut other: Self) {
        self.parts.append(&mut other.parts);
    }
}

pub(super) struct EditSite(Range);

impl EditSite {
    pub(super) const fn new(start: usize, end: usize) -> Self {
        Self(Range {
            start,
            end,
            node: None,
        })
    }

    pub(super) fn select(self, recipe: Recipe) -> Selected {
        Selected {
            site: self.0,
            recipe,
        }
    }
}

pub(crate) struct Selected {
    site: Range,
    recipe: Recipe,
}

impl Selected {
    pub(crate) fn range(&self) -> std::ops::Range<BytePos> {
        BytePos::new(self.site.start)..BytePos::new(self.site.end)
    }

    pub(crate) fn fragments(&self) -> impl Iterator<Item = FragmentPart> + '_ {
        self.recipe.parts.iter().flat_map(|part| {
            let source = |range: &Range| FragmentPart::Source {
                start: BytePos::new(range.start),
                end: BytePos::new(range.end),
                node: range.node,
            };
            let (first, second) = match part {
                Part::Text(" ") => (FragmentPart::Spaces(1), None),
                Part::Text(text) => (FragmentPart::Text(text), None),
                Part::CommentGap { spaces, .. } | Part::FlowGap { spaces, .. } => {
                    (FragmentPart::Spaces(*spaces), None)
                }
                Part::Copy(range) => (source(range), None),
                Part::SpaceCopy(range) => {
                    (FragmentPart::Spaces(1), Some(source(range)))
                }
                Part::CommaCopy(range) => {
                    (FragmentPart::Text(", "), Some(source(range)))
                }
                Part::Break(column) => (FragmentPart::Break(*column), None),
                Part::Structure(structure) => {
                    (FragmentPart::Structure(*structure), None)
                }
            };
            [Some(first), second].into_iter().flatten()
        })
    }

    pub(super) fn comment_spacing(
        &mut self,
        cfg: Option<&crate::rules::comments::Config>,
        permission: crate::format::layout::Permission<'_>,
    ) -> bool {
        let mut changed = false;
        for part in &mut self.recipe.parts {
            if let Part::CommentGap { at, spaces } = part {
                let target = crate::rules::comments::attachment_gap(
                    cfg, *spaces, *at, permission,
                );
                changed |= *spaces != target;
                *spaces = target;
            }
        }
        changed
    }

    pub(super) fn flow_spacing(
        &mut self,
        mut target: impl FnMut(usize, Container, usize) -> usize,
    ) -> bool {
        let mut changed = false;
        for part in &mut self.recipe.parts {
            if let Part::FlowGap { node, kind, spaces } = part {
                let selected = target(*node, *kind, *spaces);
                changed |= selected != *spaces;
                *spaces = selected;
            }
        }
        changed
    }

    pub(super) fn replace_bodies(
        &mut self,
        replacements: &mut std::collections::HashMap<usize, Recipe>,
    ) {
        let mut parts = std::mem::take(&mut self.recipe.parts).into_iter();
        while let Some(part) = parts.next() {
            if let Part::Structure(Structure::Start { node, .. }) = &part
                && let Some(mut body) = replacements.remove(node)
            {
                parts.by_ref().find(|part| matches!(part, Part::Structure(Structure::End { node: end, .. }) if end == node));
                self.recipe.parts.append(&mut body.parts);
            } else {
                self.recipe.parts.push(part);
            }
        }
    }
}
