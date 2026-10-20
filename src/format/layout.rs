use std::collections::HashMap;

use granit_parser::{Event, ScalarStyle};

use crate::fix::{PassTracking, Passes};
use crate::rules::{
    braces, brackets, colons, commas, comments, comments_indentation, document_end,
    document_start, empty_lines, hyphens, indentation, new_lines, quoted_strings,
    trailing_spaces,
};
use crate::text_version::Analysis;
use crate::text_version::TextVersion;
use crate::text_version::edits::Edit;

pub(crate) mod dry;
mod pieces;
use pieces::{Piece, PlanEdit, plan_edit};
pub(crate) mod stats;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Fallback {
    Repair,
    Suppression,
    Conversion,
    ScalarInteraction,
    Overlap,
    Refusal,
    Verification,
    DryEdits,
    UnsettledQuote(usize),
}

struct DocumentPlan<'a> {
    input: Analysis<'a>,
    layout: indentation::Layout<'a>,
    targets: indentation::Targets,
    edits: Vec<PlanEdit>,
    boundaries: Vec<Edit>,
    styles: Vec<(usize, ScalarStyle)>,
    rules: Vec<&'static str>,
    newline: Option<&'static str>,
    dirty: bool,
    removed: Vec<std::ops::Range<usize>>,
    folds: Vec<FoldBreak>,
}

struct ScalarFold {
    full: std::ops::Range<usize>,
    style: ScalarStyle,
    first: usize,
    last: usize,
    content: usize,
    indent: usize,
    width: u16,
}

struct FoldBreak {
    line: usize,
    byte: usize,
    indent: usize,
}

pub(crate) fn format(
    source: &mut TextVersion<'_>,
    passes: &Passes<'_>,
    tracking: &mut PassTracking<'_>,
) -> Result<TextVersion<'static>, Fallback> {
    let mut quotes = HashMap::new();
    let (prepared, kept) = {
        let plan = DocumentPlan::new(source.analysis(), passes, &mut quotes)?;
        let kept = plan.layout.kept_dash_lines.clone();
        if !plan.dirty
            && passes.indentation.is_some_and(|cfg| {
                cfg.requires_consistency() && !plan.layout.consistent(&cfg)
            })
        {
            return Err(Fallback::Refusal);
        }
        (
            plan.dirty
                .then(|| plan.emit().map(|text| (text, plan.styles, plan.rules)))
                .transpose()?,
            kept,
        )
    };
    let Some((text, styles, rules)) = prepared else {
        remember(tracking, source, passes.indentation, kept);
        source.mark_strict_valid();
        return Ok(
            std::mem::replace(source, TextVersion::new(String::new())).into_owned()
        );
    };
    source.release_layout();
    let input = source.analysis();
    let mut output = TextVersion::new(text);
    stats::count(stats::Counter::Emit);
    let expected = input.event_iter().map(|(event, span)| match event {
        Event::Scalar(value, style, anchor, tag) => {
            let at =
                crate::rules::support::span_utils::marker_byte_offset(span.start).get();
            let style = styles
                .binary_search_by_key(&at, |(at, _)| *at)
                .map_or(style, |index| styles[index].1);
            Event::Scalar(value, style, anchor, tag)
        }
        Event::DocumentStart(explicit, version) => {
            Event::DocumentStart(explicit || passes.document_start.is_some(), version)
        }
        other => other,
    });
    if !input
        .verify_strict_events(output.analysis(), expected)
        .unwrap_or(false)
        || !crate::fix::format_events_match_with_analysis(input, output.analysis())
    {
        stats::count(stats::Counter::Rollback);
        return Err(Fallback::Verification);
    }
    output.mark_strict_valid();
    source.take_facts();
    dry::prepare(
        output.analysis(),
        &passes.indentation.expect("supported plan has indentation"),
        passes.line_length.is_some(),
    );
    let dry = DocumentPlan::new(output.analysis(), passes, &mut quotes).inspect_err(
        |_| {
            stats::count(stats::Counter::Rollback);
        },
    )?;
    if dry.dirty {
        stats::count(stats::Counter::Rollback);
        return Err(Fallback::DryEdits);
    }
    if let Some(cfg) = passes.indentation
        && cfg.requires_consistency()
        && !dry.layout.consistent(&cfg)
    {
        stats::count(stats::Counter::Rollback);
        return Err(Fallback::Refusal);
    }
    remember(
        tracking,
        &output,
        passes.indentation,
        dry.layout.kept_dash_lines.clone(),
    );
    tracking.edited.extend(rules);
    let plan = source.analysis().plan_version(output);
    source
        .apply(plan)
        .expect("layout plan belongs to the input epoch");
    Ok(std::mem::replace(source, TextVersion::new(String::new())).into_owned())
}

fn remember(
    tracking: &mut PassTracking<'_>,
    text: &str,
    target: Option<indentation::Config>,
    kept_dash_lines: Vec<(usize, usize)>,
) {
    if let (Some(slot), Some(target)) = (tracking.reindented.as_deref_mut(), target) {
        *slot = Some((
            text.to_string(),
            target,
            indentation::Reindented {
                text: text.to_string(),
                refused: Vec::new(),
                kept_dash_lines,
            },
        ));
    }
}

struct Presentation {
    edits: Vec<Edit>,
    styles: Vec<(usize, ScalarStyle)>,
    rules: Vec<&'static str>,
}

impl Presentation {
    fn new(
        input: Analysis<'_>,
        passes: &Passes<'_>,
        layout: &mut indentation::Layout<'_>,
        cache: &mut HashMap<String, Option<bool>>,
    ) -> Result<Self, Fallback> {
        let mut rules = Vec::new();
        let mut edits = Vec::new();
        macro_rules! collect {
            ($rule:ident) => {
                if let Some(cfg) = passes.$rule.as_ref() {
                    let next = $rule::edits(input, cfg);
                    if !next.is_empty() {
                        rules.push($rule::ID);
                    }
                    edits.extend(next);
                }
            };
        }
        collect!(comments);
        if let Some(cfg) = passes.comments_indentation {
            let prefixes = comments_indentation::edits(input, cfg);
            if !prefixes.is_empty() {
                rules.push(comments_indentation::ID);
            }
            for (start, _, text) in prefixes {
                let original = input
                    .byte_line_starts()
                    .partition_point(|&at| at <= start.get())
                    - 1;
                let index = layout.project_line(original, start.get());
                let line = &mut layout.lines[index];
                line.prefix = text.len();
            }
        }
        collect!(commas);
        collect!(braces);
        collect!(brackets);
        collect!(colons);
        collect!(hyphens);

        let quoted = Self::quotes(input, passes, cache)?;
        rules.extend(quoted.rules);
        edits.extend(quoted.edits);
        let styles = quoted.styles;
        edits.sort_by_key(|(start, end, _)| (start.get(), end.get()));
        if edits
            .windows(2)
            .any(|pair| pair[0].1 > pair[1].0 || pair[0].0 == pair[1].0)
        {
            return Err(Fallback::Overlap);
        }

        Ok(Self {
            edits,
            styles,
            rules,
        })
    }

    fn quotes(
        input: Analysis<'_>,
        passes: &Passes<'_>,
        quotes: &mut HashMap<String, Option<bool>>,
    ) -> Result<Self, Fallback> {
        let mut edits = Vec::new();
        let mut styles = Vec::new();
        if let Some(cfg) = passes.quoted_strings.as_ref() {
            let (next, settled, unsettled) =
                quoted_strings::format_replacements(input, cfg, quotes, true);
            if !settled {
                return Err(Fallback::UnsettledQuote(
                    unsettled.expect("unsettled scalar has a source offset"),
                ));
            }
            for (start, _, text) in &next {
                let style = match text.as_bytes().first() {
                    Some(b'\'') => ScalarStyle::SingleQuoted,
                    Some(b'"') => ScalarStyle::DoubleQuoted,
                    _ => ScalarStyle::Plain,
                };
                styles.push((start.get(), style));
            }
            for (start, end, text) in next {
                if text.contains(['\n', '\r']) {
                    edits.extend(
                        multiline_edits(input.text(), start, end, &text)
                            .ok_or(Fallback::ScalarInteraction)?,
                    );
                } else {
                    edits.push((start, end, text));
                }
            }
        }

        let rules = if edits.is_empty() {
            Vec::new()
        } else {
            vec![quoted_strings::ID]
        };
        Ok(Self {
            edits,
            styles,
            rules,
        })
    }
}

impl<'a> DocumentPlan<'a> {
    fn new(
        input: Analysis<'a>,
        passes: &Passes<'_>,
        quotes: &mut HashMap<String, Option<bool>>,
    ) -> Result<Self, Fallback> {
        stats::count(stats::Counter::Plan);
        if passes.collection_style.is_some() {
            return Err(Fallback::Conversion);
        }
        if input.parsed().document_starts.is_empty() {
            return Err(Fallback::Refusal);
        }
        if crate::directives::disables_file(input.text())
            || !passes.per_line.is_empty()
            || super::FORMAT_RULE_IDS
                .iter()
                .any(|rule| input.directives().disables_any(rule))
        {
            return Err(Fallback::Suppression);
        }
        if crate::rules::block_scalar_chomping::ambiguous_blank_body_line_with_analysis(
            input,
        )
        .is_some()
        {
            return Err(Fallback::ScalarInteraction);
        }
        let Some(cfg) = passes.indentation else {
            return Err(Fallback::Refusal);
        };
        let mut layout = indentation::Layout::new(input, &cfg);
        let Presentation {
            edits,
            styles,
            mut rules,
        } = Presentation::new(input, passes, &mut layout, quotes)?;
        if input.layout_facts().is_some() && !rules.is_empty() {
            return Err(Fallback::DryEdits);
        }
        if !layout.collapse_scalars(&edits) {
            return Err(Fallback::ScalarInteraction);
        }
        let targets = layout.targets(&cfg, &edits, passes.line_length.is_some());
        if targets.refused.iter().any(Option::is_some) {
            return Err(Fallback::Refusal);
        }
        if passes.line_length.is_some()
            && folded_tail_interacts(input, &layout, &targets)
        {
            return Err(Fallback::ScalarInteraction);
        }
        if targets.deltas.iter().any(|&delta| delta != 0)
            || !targets.gaps.is_empty()
            || layout.shaped
        {
            rules.push(indentation::ID);
        }
        let newline = passes.new_lines.map(|cfg| match cfg.kind {
            new_lines::LineKind::Unix => "\n",
            new_lines::LineKind::Dos => "\r\n",
            new_lines::LineKind::Platform => new_lines::platform_newline(),
        });
        if newline.is_some_and(|newline| {
            layout
                .lines
                .iter()
                .any(|line| !line.ending.is_empty() && line.ending != newline)
        }) {
            rules.push(new_lines::ID);
        }
        let mut plan = Self {
            input,
            layout,
            targets,
            edits: edits.into_iter().map(plan_edit).collect(),
            boundaries: Vec::new(),
            styles,
            rules,
            newline,
            dirty: false,
            removed: Vec::new(),
            folds: Vec::new(),
        };
        plan.finish(passes)?;
        Ok(plan)
    }

    fn trim(&mut self, passes: &Passes<'_>) {
        let input = self.input;
        let layout = &mut self.layout;
        let edits = &mut self.edits;
        let boundaries = &mut self.boundaries;
        let rules = &mut self.rules;
        let scalar_owners: Vec<_> =
            layout.lines.iter().filter_map(|line| line.bridge).collect();
        if passes.trailing_spaces {
            let next = trailing_spaces::edits(input);
            if !next.is_empty() {
                rules.push(trailing_spaces::ID);
            }
            for edit in next {
                let owner = scalar_owners
                    .get(scalar_owners.partition_point(|(_, end)| *end < edit.0.get()));
                if owner.is_some_and(|(start, end)| {
                    *start <= edit.0.get() && edit.1.get() <= *end
                }) {
                    continue;
                }
                if edit.0 == edit.1
                    && passes.final_newline.is_some()
                    && edit.0.get() == input.text().len()
                    && !input.text().ends_with(['\n', '\r'])
                {
                    continue;
                }
                if edit.0 == edit.1 {
                    boundaries.push(edit);
                    continue;
                }
                let original = input
                    .byte_line_starts()
                    .partition_point(|&at| at <= edit.0.get())
                    - 1;
                let index = layout.project_line(original, edit.0.get());
                let line = &mut layout.lines[index];
                if line.body.is_empty() && line.tail.is_empty() {
                    line.prefix = 0;
                    continue;
                }
                edits.push(plan_edit(edit));
            }
        }
    }

    fn markers(&mut self, passes: &Passes<'_>) {
        let input = self.input;
        let edits = &mut self.edits;
        let boundaries = &mut self.boundaries;
        let rules = &mut self.rules;
        for (rule, next) in [
            (
                document_start::ID,
                passes
                    .document_start
                    .map_or_else(Vec::new, |cfg| document_start::edits(input, cfg)),
            ),
            (
                document_end::ID,
                passes
                    .document_end
                    .map_or_else(Vec::new, |cfg| document_end::edits(input, cfg)),
            ),
        ] {
            if !next.is_empty() {
                rules.push(rule);
            }
            for mut edit in next {
                if edit.0 != edit.1 {
                    edits.push(plan_edit(edit));
                    continue;
                }
                if rule == document_start::ID {
                    attach_marker_bom(input, &mut self.layout, edit.0);
                }
                if rule == document_end::ID
                    && passes.final_newline.is_some()
                    && edit.0.get() == input.text().len()
                    && !input.text().ends_with(['\n', '\r'])
                {
                    let ending = crate::rules::support::line_syntax::buffer_newline(
                        input.text(),
                    );
                    edit.2 = edit.2.strip_prefix(ending).unwrap_or(&edit.2).to_string();
                }
                boundaries.push(edit);
            }
        }
    }

    fn finish(&mut self, passes: &Passes<'_>) -> Result<(), Fallback> {
        let final_newline = passes.final_newline.is_some()
            && self
                .layout
                .lines
                .iter()
                .enumerate()
                .rev()
                .find(|(index, line)| {
                    !line.ending.is_empty()
                        || !line.body.is_empty()
                        || !line.tail.is_empty()
                        || line.bom
                        || line
                            .prefix
                            .saturating_add_signed(self.targets.deltas[*index])
                            > 0
                })
                .is_some_and(|(_, line)| line.ending.is_empty());
        if final_newline {
            let last = self
                .layout
                .lines
                .last_mut()
                .expect("nonempty input has a line");
            last.ending = self.newline.unwrap_or("\n");
            self.rules.push(crate::rules::new_line_at_end_of_file::ID);
        }
        self.trim(passes);
        self.markers(passes);
        self.boundaries.sort_by_key(|(start, _, _)| start.get());
        if let Some(cfg) = passes.empty_lines {
            self.empty_lines(&cfg);
        }
        let edits = &mut self.edits;
        let boundaries = &mut self.boundaries;
        let styles = &mut self.styles;
        let rules = &mut self.rules;
        edits.sort_by_key(|(start, end, _)| (start.get(), end.get()));
        edits.dedup();
        if edits
            .windows(2)
            .any(|pair| pair[0].1 > pair[1].0 || pair[0].0 == pair[1].0)
        {
            return Err(Fallback::Overlap);
        }
        boundaries.sort_by_key(|(start, _, _)| start.get());
        styles.sort_by_key(|(start, _)| *start);
        self.dirty = !rules.is_empty();
        if let Some(cfg) = passes.line_length {
            self.plan_folds(cfg)?;
        }
        if !self.folds.is_empty() {
            self.rules.push(crate::rules::line_length::ID);
            self.dirty = true;
        }
        Ok(())
    }

    fn empty_lines(&mut self, cfg: &empty_lines::Config) {
        let mut empty: Vec<_> = self
            .input
            .lines()
            .map(|(_, text, _)| text.is_empty())
            .collect();
        for line in &self.layout.lines {
            empty[line.origin] = line.is_empty();
        }
        let protected =
            empty_lines::preserved_scalar_lines_with_empty(self.input, |line| {
                empty[line]
            })
            .expect("layout input has a complete parse");
        let mut run = Vec::new();
        let mut seen = false;
        let mut boundary = 0;
        for (index, line) in self.layout.lines.iter().enumerate() {
            let at =
                self.input.byte_line_starts()[line.origin] + usize::from(line.bom) * 3;
            let marker = self
                .boundaries
                .get(boundary)
                .is_some_and(|(start, _, _)| start.get() <= at);
            if marker {
                remove_blanks(
                    self.input,
                    &self.layout.lines,
                    &mut self.removed,
                    &mut run,
                    if seen { cfg.max() } else { cfg.max_start() },
                );
                seen = true;
                while self
                    .boundaries
                    .get(boundary)
                    .is_some_and(|(start, _, _)| start.get() <= at)
                {
                    boundary += 1;
                }
            }
            if line.is_empty() && !protected.contains(&(line.origin + 1)) {
                if line.ending.is_empty() {
                    continue;
                }
                run.push(index);
                continue;
            }
            remove_blanks(
                self.input,
                &self.layout.lines,
                &mut self.removed,
                &mut run,
                if seen { cfg.max() } else { cfg.max_start() },
            );
            seen |= !line.is_empty();
        }
        let maximum = if boundary < self.boundaries.len() {
            if seen { cfg.max() } else { cfg.max_start() }
        } else {
            cfg.max_end()
        };
        remove_blanks(
            self.input,
            &self.layout.lines,
            &mut self.removed,
            &mut run,
            maximum,
        );
        if !self.removed.is_empty() {
            self.rules.push(empty_lines::ID);
        }
    }

    fn emit(&self) -> Result<String, Fallback> {
        let mut output = String::with_capacity(self.input.text().len());
        let source = self.input.text();
        let fold_newline = self.newline.unwrap_or_else(|| {
            crate::rules::support::line_syntax::buffer_newline(source)
        });
        let mut boundary = 0;
        for (index, line) in self.layout.lines.iter().enumerate() {
            let line_start = self.input.byte_line_starts()[line.origin];
            if line.bom {
                output.push('\u{feff}');
            }
            while self.boundaries.get(boundary).is_some_and(|edit| {
                edit.0.get() <= line_start + usize::from(line.bom) * 3
            }) {
                let text = &self.boundaries[boundary].2;
                output.push_str(&self.ending(text));
                boundary += 1;
            }
            let removed = self
                .removed
                .get(
                    self.removed
                        .partition_point(|range| range.end <= line_start),
                )
                .is_some_and(|range| range.start <= line_start);
            if removed {
                continue;
            }
            let prefix = line
                .prefix
                .saturating_add_signed(self.targets.deltas[index]);
            output.extend(std::iter::repeat_n(
                ' ',
                prefix * usize::from(!line.is_empty()),
            ));
            let first = self.folds.partition_point(|site| site.line < index);
            let last = self.folds.partition_point(|site| site.line <= index);
            let mut fold = first;
            let mut position = prefix + usize::from(line.bom) * 3;
            self.visit_body(index, |text| {
                let mut from = 0;
                while fold < last && self.folds[fold].byte < position + text.len() {
                    let site = &self.folds[fold];
                    let at = site.byte - position;
                    text.slice(from..at).append(&mut output);
                    output.push_str(fold_newline);
                    output.extend(std::iter::repeat_n(' ', site.indent));
                    from = at + 1;
                    fold += 1;
                }
                text.slice(from..text.len()).append(&mut output);
                position += text.len();
            })?;
            output.push_str(
                self.newline
                    .filter(|_| !line.ending.is_empty())
                    .unwrap_or(line.ending),
            );
        }
        for (_, _, text) in &self.boundaries[boundary..] {
            output.push_str(&self.ending(text));
        }
        Ok(output)
    }

    fn visit_body<'s>(
        &'s self,
        index: usize,
        emit: impl FnMut(Piece<'s>),
    ) -> Result<(), Fallback> {
        let first = self.targets.gaps.partition_point(|gap| gap.line < index);
        let last = self.targets.gaps.partition_point(|gap| gap.line <= index);
        visit_body(
            self.input.text(),
            &self.layout.lines[index],
            &self.targets.gaps[first..last],
            &self.edits,
            emit,
        )
    }

    fn ending<'b>(&self, text: &'b str) -> std::borrow::Cow<'b, str> {
        self.newline.map_or_else(
            || text.into(),
            |newline| {
                text.replace("\r\n", "\n")
                    .replace('\r', "\n")
                    .replace('\n', newline)
                    .into()
            },
        )
    }

    fn prefix(&self, index: usize) -> usize {
        let line = &self.layout.lines[index];
        line.prefix
            .saturating_add_signed(self.targets.deltas[index])
            * usize::from(!line.is_empty())
    }

    fn byte_on_line(&self, index: usize, byte: usize) -> Option<usize> {
        let line = &self.layout.lines[index];
        let source = self.input.text();
        let mut position = self.prefix(index) + usize::from(line.bom) * 3;
        for (piece, part) in [line.body, line.tail].into_iter().enumerate() {
            if piece == 1
                && let Some((start, end)) = line.bridge
            {
                if byte < end {
                    return None;
                }
                position += self.edits[self
                    .edits
                    .partition_point(|(from, _, _)| from.get() < start)]
                .2
                .len();
            }
            if part.is_empty() {
                continue;
            }
            let start = part.as_ptr() as usize - source.as_ptr() as usize;
            let end = start + part.len();
            if byte < start {
                return None;
            }
            let stop = byte.min(end);
            position += stop - start;
            let first = self.edits.partition_point(|(_, end, _)| end.get() < start);
            for (from, to, text) in self.edits[first..]
                .iter()
                .take_while(|(_, to, _)| to.get() <= stop)
            {
                if from.get() < start {
                    continue;
                }
                position = position + text.len() - (to.get() - from.get());
            }
            if byte <= end {
                let raw = position.saturating_add_signed(-self.targets.deltas[index]);
                let first = self.targets.gaps.partition_point(|gap| gap.line < index);
                for gap in self.targets.gaps[first..]
                    .iter()
                    .take_while(|gap| gap.line == index)
                {
                    position -= gap.removed.min(raw.saturating_sub(gap.column + 2));
                }
                return Some(position);
            }
        }
        None
    }

    fn plan_folds(
        &mut self,
        cfg: crate::rules::line_length::Fold,
    ) -> Result<(), Fallback> {
        for scalar in crate::rules::line_length::fold::foldable_scalars(self.input) {
            self.plan_scalar_fold(scalar.content, scalar.style, cfg)?;
        }
        self.folds.sort_by_key(|site| (site.line, site.byte));
        Ok(())
    }

    fn plan_scalar_fold(
        &mut self,
        range: std::ops::Range<usize>,
        original_style: ScalarStyle,
        cfg: crate::rules::line_length::Fold,
    ) -> Result<(), Fallback> {
        let starts = self.input.byte_line_starts();
        let line_of = |byte| {
            starts
                .partition_point(|&start| start <= byte)
                .saturating_sub(1)
        };
        let original_quotes = usize::from(matches!(
            original_style,
            ScalarStyle::SingleQuoted | ScalarStyle::DoubleQuoted
        ));
        let full = range.start - original_quotes..range.end + original_quotes;
        if full.is_empty() {
            return Ok(());
        }
        let style = self
            .styles
            .binary_search_by_key(&full.start, |(at, _)| *at)
            .map_or(original_style, |index| self.styles[index].1);
        let quotes = usize::from(matches!(
            style,
            ScalarStyle::SingleQuoted | ScalarStyle::DoubleQuoted
        ));
        let first = self.layout.project_line(line_of(full.start), full.start);
        let last = self
            .layout
            .project_line(line_of(full.end - 1), full.end - 1);
        let Some(content) = self.byte_on_line(first, full.start).map(|at| at + quotes)
        else {
            return Err(Fallback::ScalarInteraction);
        };
        let owner = self
            .targets
            .scalar_owners
            .partition_point(|&(at, _)| at <= full.start)
            .checked_sub(1)
            .map_or(0, |index| self.targets.scalar_owners[index].1);
        let folded = style == ScalarStyle::Folded;
        if folded && content <= owner {
            return Ok(());
        }
        let min_step = if style == ScalarStyle::Plain { 1 } else { 2 };
        let existing = (first + 1..=last)
            .find(|&line| !self.layout.lines[line].body.trim().is_empty())
            .map(|line| self.prefix(line))
            .filter(|&prefix| prefix > 0);
        if !folded && existing.is_some_and(|indent| indent < owner + min_step) {
            return Ok(());
        }
        let indent = if folded {
            content
        } else {
            existing.unwrap_or(owner + usize::from(cfg.indent).max(min_step))
        };
        let scalar = ScalarFold {
            full,
            style,
            first,
            last,
            content,
            indent,
            width: cfg.width,
        };
        for index in first..=last {
            self.plan_fold_line(index, &scalar)?;
        }
        Ok(())
    }

    fn plan_fold_line(
        &mut self,
        index: usize,
        scalar: &ScalarFold,
    ) -> Result<(), Fallback> {
        use crate::rules::line_length::fold::breaks_from_chars;
        let width = scalar.width;
        let source = self.input.text();
        let full = &scalar.full;
        let (first, last, content, indent, style) = (
            scalar.first,
            scalar.last,
            scalar.content,
            scalar.indent,
            scalar.style,
        );
        let folded = style == ScalarStyle::Folded;
        let quotes = usize::from(matches!(
            style,
            ScalarStyle::SingleQuoted | ScalarStyle::DoubleQuoted
        ));
        let line = &self.layout.lines[index];
        if folded
            && (self.prefix(index) != indent
                || line.body.starts_with([' ', '\t'])
                || line.body.is_empty())
        {
            return Ok(());
        }
        let mut parts = Vec::new();
        self.visit_body(index, |part| parts.push(part))?;
        let prefix = self.prefix(index);
        let bom = if line.bom { "\u{feff}" } else { "" };
        let length =
            prefix + bom.len() + parts.iter().map(|part| part.len()).sum::<usize>();
        let scalar = if folded {
            indent..length
        } else {
            let start = if index == first { content } else { 0 };
            let end = if index == last {
                self.byte_on_line(index, full.end)
                    .ok_or(Fallback::ScalarInteraction)?
                    - quotes
            } else {
                length
            };
            start..end
        };
        if index == last
            && source[full.end..]
                .split(['\r', '\n'])
                .next()
                .and_then(|rest| rest.split_once('#'))
                .is_some_and(|(_, comment)| {
                    crate::directives::directive_scope(comment).is_some()
                })
        {
            return Ok(());
        }
        let chars = bom
            .chars()
            .chain(std::iter::repeat_n(' ', prefix))
            .chain(parts.iter().flat_map(|part| part.chars()));
        let mut byte = 0;
        let chars = chars.map(|ch| {
            let at = byte;
            byte += ch.len_utf8();
            (at, ch)
        });
        let breaks =
            breaks_from_chars(chars, &scalar, style, indent, usize::from(width));
        self.folds.extend(breaks.into_iter().map(|byte| FoldBreak {
            line: index,
            byte,
            indent,
        }));

        Ok(())
    }
}

fn attach_marker_bom(
    input: Analysis<'_>,
    layout: &mut indentation::Layout<'_>,
    at: crate::rules::support::span_utils::BytePos,
) {
    let original = input
        .byte_line_starts()
        .partition_point(|&byte| byte <= at.get())
        - 1;
    let index = layout.project_line(original, at.get());
    let line = &mut layout.lines[index];
    if !line.bom && at.get() == input.byte_line_starts()[original] + 3 {
        line.body = line
            .body
            .strip_prefix('\u{feff}')
            .expect("document marker follows a leading BOM");
        line.bom = true;
    }
}

fn folded_tail_interacts(
    input: Analysis<'_>,
    layout: &indentation::Layout<'_>,
    targets: &indentation::Targets,
) -> bool {
    if input.text().ends_with(['\n', '\r']) {
        return false;
    }
    let lines = input.line_contents();
    lines
        .last()
        .is_some_and(|line| line.bytes().all(|byte| byte == b' '))
        && lines
            .iter()
            .rev()
            .take_while(|line| line.trim_matches([' ', '\t']).is_empty())
            .take(2)
            .count()
            == 2
        && input.headers().iter().any(|header| {
            let line = header.span.start.line() - 1;
            lines[header.line - 1].chars().nth(header.column - 1) == Some('>')
                && targets.deltas[layout.project_line(
                    line,
                    crate::rules::support::span_utils::marker_byte_offset(
                        header.span.start,
                    )
                    .get(),
                )] < 0
        })
}

pub(crate) fn column(
    input: Analysis<'_>,
    edits: &[Edit],
    line: usize,
    byte: usize,
    column: usize,
) -> usize {
    let start = input.byte_line_starts()[line];
    let first = edits.partition_point(|(_, end, _)| end.get() < start);
    let last = edits.partition_point(|(_, end, _)| end.get() <= byte);
    edits[first..last]
        .iter()
        .fold(column, |column, (from, to, text)| {
            column + text.chars().count()
                - input.text()[from.get()..to.get()].chars().count()
        })
}

fn multiline_edits(
    source: &str,
    start: crate::rules::support::span_utils::BytePos,
    end: crate::rules::support::span_utils::BytePos,
    replacement: &str,
) -> Option<Vec<Edit>> {
    use crate::rules::support::{
        line_syntax::split_lines_preserve_endings, span_utils::BytePos,
    };
    let original = &source[start.get()..end.get()];
    let mut before = split_lines_preserve_endings(original);
    let mut after = split_lines_preserve_endings(replacement);
    let mut offset = start.get();
    let mut edits = Vec::new();
    loop {
        match (before.next(), after.next()) {
            (None, None) => return Some(edits),
            (Some((_, old, ending)), Some((_, new, _))) => {
                if old != new {
                    let prefix: usize = old
                        .chars()
                        .zip(new.chars())
                        .take_while(|(a, b)| a == b)
                        .map(|(ch, _)| ch.len_utf8())
                        .sum();
                    let suffix: usize = old[prefix..]
                        .chars()
                        .rev()
                        .zip(new[prefix..].chars().rev())
                        .take_while(|(a, b)| a == b)
                        .map(|(ch, _)| ch.len_utf8())
                        .sum();
                    edits.push((
                        BytePos::new(offset + prefix),
                        BytePos::new(offset + old.len() - suffix),
                        new[prefix..new.len() - suffix].to_string(),
                    ));
                }
                offset += old.len() + ending.len();
            }
            _ => return None,
        }
    }
}

fn remove_blanks(
    input: Analysis<'_>,
    lines: &[VirtualLine<'_>],
    removed: &mut Vec<std::ops::Range<usize>>,
    run: &mut Vec<usize>,
    maximum: i64,
) {
    let keep = usize::try_from(maximum).unwrap_or(0);
    if let Some(&first) = run.get(keep) {
        let first = lines[first].origin;
        let last = lines[*run.last().expect("a removed blank run is nonempty")].origin;
        let start = input.byte_line_starts()[first];
        let end = input
            .byte_line_starts()
            .get(last + 1)
            .copied()
            .unwrap_or(input.text().len());
        removed.push(start..end);
    }
    run.clear();
}

pub(crate) struct VirtualLine<'a> {
    pub(crate) origin: usize,
    pub(crate) prefix: usize,
    pub(crate) body: &'a str,
    pub(crate) tail: &'a str,
    pub(crate) ending: &'a str,
    pub(crate) bom: bool,
    pub(crate) bridge: Option<(usize, usize)>,
}

impl<'a> VirtualLine<'a> {
    pub(crate) fn new(
        origin: usize,
        content: &'a str,
        ending: &'a str,
        bom: bool,
    ) -> Self {
        let content = if bom {
            content
                .strip_prefix('\u{feff}')
                .expect("normalized line starts after the BOM")
        } else {
            content
        };
        let prefix = content.len() - content.trim_start_matches(' ').len();
        Self {
            origin,
            prefix,
            body: &content[prefix..],
            tail: "",
            ending,
            bom,
            bridge: None,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        !self.bom && self.prefix == 0 && self.body.is_empty() && self.tail.is_empty()
    }

    pub(crate) fn comment_column(&self) -> Option<usize> {
        self.body
            .trim_start()
            .starts_with('#')
            .then(|| self.prefix + self.body.len() - self.body.trim_start().len())
    }

    pub(crate) fn spaces_after(&self, column: usize) -> usize {
        std::iter::repeat_n(' ', self.prefix)
            .chain(self.body.chars())
            .chain(self.tail.chars())
            .skip(column + 1)
            .take_while(|&ch| ch == ' ')
            .count()
    }
}

pub(crate) fn lines<'a>(input: Analysis<'a>, starts: &[usize]) -> Vec<VirtualLine<'a>> {
    input
        .lines()
        .map(|(line, content, ending)| {
            VirtualLine::new(
                line,
                content,
                ending,
                starts[line] > input.byte_line_starts()[line],
            )
        })
        .collect()
}

fn visit_body<'a>(
    source: &'a str,
    line: &VirtualLine<'a>,
    gaps: &[indentation::Gap],
    edits: &'a [PlanEdit],
    mut write: impl FnMut(Piece<'a>),
) -> Result<(), Fallback> {
    let first = 0;
    let last = gaps.len();
    let mut gap = first;
    let mut column = line.prefix;
    let mut append = |text: Piece<'a>| -> Result<(), Fallback> {
        let mut from = 0;
        while gap < last && gaps[gap].column + 2 < column + text.len() {
            let site = &gaps[gap];
            let start = (site.column + 2).saturating_sub(column);
            let end = (site.column + 2 + site.removed)
                .saturating_sub(column)
                .min(text.len());
            if from > start
                || !text.is_char_boundary(start)
                || !text.is_char_boundary(end)
            {
                return Err(Fallback::Overlap);
            }
            write(text.slice(from..start));
            from = end;
            if column + end == site.column + 2 + site.removed {
                gap += 1;
            } else {
                break;
            }
        }
        write(text.slice(from..text.len()));
        column += text.len();
        Ok(())
    };
    for (piece, part) in [line.body, line.tail].into_iter().enumerate() {
        if piece == 1
            && let Some((start, end)) = line.bridge
        {
            let edit = &edits[edits.partition_point(|(from, _, _)| from.get() < start)];
            if (edit.0.get(), edit.1.get()) != (start, end) {
                return Err(Fallback::Overlap);
            }
            append(edit.2.piece())?;
        }
        if part.is_empty() {
            continue;
        }
        let start = part.as_ptr() as usize - source.as_ptr() as usize;
        let end = start + part.len();
        let mut at = start;
        let first = edits.partition_point(|(from, end, _)| {
            end.get() < start || (end.get() == start && from < end)
        });
        for (from, to, text) in edits[first..]
            .iter()
            .take_while(|(from, _, _)| from.get() <= end)
        {
            if line.bridge == Some((from.get(), to.get())) {
                continue;
            }
            if from.get() < start || to.get() > end {
                return Err(Fallback::Overlap);
            }
            append(Piece::Text(&source[at..from.get()]))?;
            append(text.piece())?;
            at = to.get();
        }
        append(Piece::Text(&source[at..end]))?;
    }
    Ok(())
}

pub(crate) fn emit_line(
    text: &mut String,
    source: &str,
    line: &VirtualLine<'_>,
    delta: isize,
    gaps: &[indentation::Gap],
) {
    if line.bom {
        text.push('\u{feff}');
    }
    let prefix =
        line.prefix.saturating_add_signed(delta) * usize::from(!line.is_empty());
    text.extend(std::iter::repeat_n(' ', prefix));
    visit_body(source, line, gaps, &[], |part| part.append(text))
        .expect("indentation gaps belong to their virtual line");
    text.push_str(line.ending);
}
