mod input;

use super::Restyled;
use crate::text_version::Analysis;
use input::{Block, Blocks};
use std::collections::{HashMap, HashSet};

pub(crate) struct Protection {
    lines: HashSet<usize>,
    keep_blank_eof: bool,
}

impl Protection {
    pub(crate) fn line(&self, line: usize) -> bool {
        self.lines.contains(&line)
    }
    pub(crate) const fn keep_blank_eof(&self) -> bool {
        self.keep_blank_eof
    }
}

fn scanner_indent(block: &Block<'_>) -> usize {
    block.indent().unwrap_or_else(|| {
        if block.empty() || block.start_line() == block.header() {
            usize::MAX
        } else {
            block.start_column()
        }
    })
}

fn root_indents(blocks: &Blocks<'_>) -> HashMap<usize, usize> {
    blocks
        .blocks()
        .filter(input::Block::root)
        .filter_map(|block| {
            block
                .indicator()
                .and_then(|digit| digit.checked_sub(1))
                .map(|indent| (block.start(), indent as usize))
        })
        .collect()
}

pub(crate) fn protection(
    source: Analysis<'_>,
    restyled: &Restyled,
) -> Option<Protection> {
    select_protection(&Blocks::new(source), restyled)
}

fn select_protection(blocks: &Blocks<'_>, restyled: &Restyled) -> Option<Protection> {
    if !blocks.valid() {
        return None;
    }
    let mut protected = HashSet::new();
    for (scalar, &(start, last)) in blocks.scalars().zip(blocks.scalar_ranges()) {
        if scalar.block
            || (scalar.escaped && scalar.multiline && !restyled.contains(scalar.start))
        {
            protected.extend(start..=last);
        }
    }
    let roots = root_indents(blocks);
    let mut keep_blank_eof = false;
    for block in blocks.blocks() {
        protected.remove(&block.header());
        let root = roots.get(&block.start()).copied();
        let indent = root.unwrap_or_else(|| scanner_indent(&block));
        for line in block.header() + 1..=block.end_line() {
            if blocks.line(line).is_some_and(|text| {
                text.bytes() <= indent && text.spaces() && !blocks.content_blank(line)
            }) {
                protected.remove(&line);
                if block.chomp() == Some('+')
                    && line == blocks.count()
                    && !blocks.ends_in_break()
                {
                    keep_blank_eof = true;
                }
            } else if (block.chomp() == Some('+')
                && blocks.line(line).is_some_and(|text| text.blank_tabs()))
                || (root.is_some()
                    && blocks.line(line).is_some_and(|text| text.spaces()))
            {
                protected.insert(line);
            }
        }
    }
    Some(Protection {
        lines: protected,
        keep_blank_eof,
    })
}

pub(crate) fn content_blank_lines(source: Analysis<'_>) -> HashSet<usize> {
    select_content_blanks(&Blocks::new(source))
}

fn select_content_blanks(blocks: &Blocks<'_>) -> HashSet<usize> {
    let mut content = HashSet::new();
    let roots = root_indents(blocks);
    for block in blocks.blocks() {
        let indicator = block.indicator();
        let blank_only = block.followed()
            && block.header() < block.end_line()
            && block.preceding_spaces();
        let indent = if let Some(indent) = roots.get(&block.start()) {
            *indent
        } else if blank_only {
            block.compact_indent() + indicator.map_or(1, |digit| digit as usize)
        } else {
            continue;
        };
        for line in block.header() + 1..=block.end_line() {
            if blocks
                .line(line)
                .is_some_and(|text| text.bytes() > indent && text.spaces())
            {
                content.insert(line);
            }
        }
    }
    content
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FinalBlank {
    Keep,
    Trim,
    Refuse,
}

pub(crate) fn final_blank(source: Analysis<'_>) -> FinalBlank {
    select_final_blank(&Blocks::new(source))
}

fn select_final_blank(blocks: &Blocks<'_>) -> FinalBlank {
    let roots = root_indents(blocks);
    let line = blocks
        .line(blocks.count())
        .expect("a document with a missing end is nonempty");
    blocks
        .blocks()
        .find_map(|block| {
            if block.header() >= blocks.count()
                || block
                    .end_line()
                    .saturating_sub(usize::from(block.end_column() == 0))
                    != blocks.count()
                || line.empty()
                || !line.spaces()
            {
                return None;
            }
            let root = roots.get(&block.start()).copied();
            let indent = root.unwrap_or_else(|| scanner_indent(&block));
            Some(if line.bytes() <= indent {
                FinalBlank::Trim
            } else if root.is_some() && block.chomp().is_some() {
                FinalBlank::Refuse
            } else {
                FinalBlank::Keep
            })
        })
        .unwrap_or(FinalBlank::Keep)
}

pub(crate) struct Preserved(Vec<bool>);

impl Preserved {
    pub(crate) fn line(&self, line: usize) -> bool {
        self.0[line]
    }
}

pub(crate) fn preserved_lines(
    source: Analysis<'_>,
    empty: &crate::rules::empty_lines::BlankLines<'_>,
) -> Option<Preserved> {
    select_preserved(&Blocks::new(source), empty)
}

fn select_preserved(
    blocks: &Blocks<'_>,
    empty: &crate::rules::empty_lines::BlankLines<'_>,
) -> Option<Preserved> {
    if !blocks.valid() {
        return None;
    }
    let mut protected = vec![false; blocks.count()];
    for &(start, last) in blocks.scalar_ranges() {
        let end = last.min(protected.len());
        let start = start.saturating_sub(1).min(end);
        protected[start..end].fill(true);
    }
    for block in blocks.blocks() {
        let blank_only_literal =
            block.literal() && block.chomp().is_none() && block.following_blanks() >= 2;
        if block.chomp() == Some('+') || blank_only_literal {
            continue;
        }
        let last = block.end_line()
            - usize::from(
                block.end_column() == 0 || block.empty() || block.next_token(),
            );
        for line in (block.header() + 1..=last)
            .rev()
            .take_while(|line| empty.empty(*line - 1))
        {
            *protected
                .get_mut(line - 1)
                .expect("parsed scalar line belongs to the source line table") = false;
        }
    }
    Some(Preserved(protected))
}

pub(crate) fn may_turn_ambiguous(source: Analysis<'_>) -> bool {
    ambiguity(&Blocks::new(source))
}

fn ambiguity(blocks: &Blocks<'_>) -> bool {
    blocks
        .blocks()
        .any(|block| block.chomp() == Some('+') || block.next_line_blank())
}

pub(crate) fn drops_final_break(source: Analysis<'_>) -> bool {
    final_break(&Blocks::new(source))
}

fn final_break(blocks: &Blocks<'_>) -> bool {
    let Some(width) = blocks.final_whitespace_width() else {
        return false;
    };
    blocks.last_block_rooted()
        && blocks.last().is_some_and(|block| {
            block.chomp() == Some('+')
                && block.header() + 1 < blocks.count()
                && block
                    .break_indicator()
                    .is_some_and(|indent| width <= indent)
        })
}

pub(crate) fn folded_tail(
    source: Analysis<'_>,
) -> Option<crate::rules::support::span_utils::BytePos> {
    select_folded_tail(&Blocks::new(source))
}

fn select_folded_tail(
    blocks: &Blocks<'_>,
) -> Option<crate::rules::support::span_utils::BytePos> {
    if blocks.ends_in_break() {
        return None;
    }
    let blank = blocks
        .line(blocks.count().max(1))
        .is_some_and(|line| line.spaces());
    if !blank || blocks.trailing_blanks() != 2 {
        return None;
    }
    let block = blocks.last()?;
    block.folded().then(|| block.header_position())
}
