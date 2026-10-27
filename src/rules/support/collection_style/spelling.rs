use crate::format::layout::projection::RetainedFragment;
use crate::rules::support::span_utils::BytePos;

struct Range {
    start: usize,
    end: usize,
}

pub(super) struct Fragment<'a> {
    range: Range,
    metrics: RetainedFragment<'a>,
}

impl<'a> Fragment<'a> {
    pub(super) fn observe(buffer: &'a str, start: usize, end: usize) -> Self {
        Self {
            range: Range { start, end },
            metrics: RetainedFragment::observe(&buffer[start..end]),
        }
    }

    pub(super) fn characters(&self) -> usize {
        self.metrics.characters()
    }

    pub(super) fn multiline(&self) -> bool {
        self.metrics.multiline()
    }

    pub(super) fn empty(&self) -> bool {
        self.metrics.empty()
    }
}

enum Part {
    Text(&'static str),
    Copy(Range),
    SpaceCopy(Range),
    CommaCopy(Range),
    ColonCopy(Range),
    AliasColonCopy(Range),
    Break(usize),
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

    fn fragment(&mut self, fragment: Fragment<'_>, part: impl FnOnce(Range) -> Part) {
        self.parts.push(part(fragment.range));
    }

    pub(super) fn copy(&mut self, fragment: Fragment<'_>) {
        self.fragment(fragment, Part::Copy);
    }

    pub(super) fn space_copy(&mut self, fragment: Fragment<'_>) {
        self.fragment(fragment, Part::SpaceCopy);
    }

    pub(super) fn comma_copy(&mut self, fragment: Fragment<'_>) {
        self.fragment(fragment, Part::CommaCopy);
    }

    pub(super) fn colon_copy(&mut self, fragment: Fragment<'_>, alias: bool) {
        if alias {
            self.fragment(fragment, Part::AliasColonCopy);
        } else {
            self.fragment(fragment, Part::ColonCopy);
        }
    }

    pub(super) fn line(&mut self, column: usize) {
        self.parts.push(Part::Break(column));
    }

    pub(super) fn append(&mut self, mut other: Self) {
        self.parts.append(&mut other.parts);
    }

    pub(super) fn materialize(self, buffer: &str, newline: &str) -> String {
        let size: usize = self
            .parts
            .iter()
            .map(|part| match part {
                Part::Text(text) => text.len(),
                Part::Copy(range) => range.end - range.start,
                Part::SpaceCopy(range) => 1 + range.end - range.start,
                Part::CommaCopy(range) | Part::ColonCopy(range) => {
                    2 + range.end - range.start
                }
                Part::AliasColonCopy(range) => 3 + range.end - range.start,
                Part::Break(column) => newline.len() + column,
            })
            .sum();
        let mut output = String::with_capacity(size);
        for part in self.parts {
            match part {
                Part::Text(text) => output.push_str(text),
                Part::Break(column) => {
                    output.push_str(newline);
                    output.extend(std::iter::repeat_n(' ', column));
                }
                Part::Copy(range) => write_copy(&mut output, buffer, "", &range),
                Part::SpaceCopy(range) => write_copy(&mut output, buffer, " ", &range),
                Part::CommaCopy(range) => write_copy(&mut output, buffer, ", ", &range),
                Part::ColonCopy(range) => write_copy(&mut output, buffer, ": ", &range),
                Part::AliasColonCopy(range) => {
                    write_copy(&mut output, buffer, " : ", &range);
                }
            }
        }
        output
    }
}

pub(super) struct EditSite(Range);

impl EditSite {
    pub(super) const fn new(start: usize, end: usize) -> Self {
        Self(Range { start, end })
    }

    pub(super) fn select(self, recipe: Recipe) -> Selected {
        Selected {
            site: self.0,
            recipe,
        }
    }
}

pub(super) struct Selected {
    site: Range,
    recipe: Recipe,
}

impl Selected {
    pub(super) fn materialize(self, buffer: &str, newline: &str) -> super::Edit {
        (
            BytePos::new(self.site.start),
            BytePos::new(self.site.end),
            self.recipe.materialize(buffer, newline),
        )
    }
}

fn write_copy(output: &mut String, buffer: &str, prefix: &str, range: &Range) {
    output.push_str(prefix);
    output.push_str(&buffer[range.start..range.end]);
}
