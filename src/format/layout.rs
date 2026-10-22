use std::collections::HashMap;

use ryl_yaml_parser::{Event, ScalarStyle};

use crate::fix::{PassTracking, Passes};
use crate::rules::support::collection_style;
use crate::rules::{
    braces, brackets, colons, commas, comments, comments_indentation, document_end,
    document_start, empty_lines, hyphens, indentation, new_lines, quoted_strings,
    trailing_spaces,
};
use crate::text_version::Analysis;
use crate::text_version::TextVersion;
use crate::text_version::edits::Edit;

pub(crate) mod facts;
mod pieces;
pub(crate) mod projection;
pub(crate) use pieces::PlanEdit;
use pieces::{Piece, plan_edit};
pub(crate) mod stats;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Fallback {
    Repair,
    Suppression,
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sink {
    Emit,
    Validate,
}

pub(crate) fn format(
    source: &mut TextVersion<'_>,
    passes: &Passes<'_>,
    tracking: &mut PassTracking<'_>,
) -> Result<TextVersion<'static>, Fallback> {
    let Some(cfg) = passes.collection_style else {
        if let Some(mut quoted) = prepare_quotes(source.analysis(), passes)? {
            let output = format_document(&mut quoted, passes, tracking)?;
            tracking.edited.push(quoted_strings::ID);
            return Ok(output);
        }
        return format_document(source, passes, tracking);
    };
    format_collections(source, passes, tracking, cfg)
}

fn format_collections(
    source: &mut TextVersion<'_>,
    passes: &Passes<'_>,
    tracking: &mut PassTracking<'_>,
    cfg: collection_style::Config,
) -> Result<TextVersion<'static>, Fallback> {
    let mut current = None;
    let mut prepared =
        collection_style::prepare(source.analysis(), cfg, passes.indentation.as_ref());
    let mut result = Err(Fallback::DryEdits);
    for _ in 0..crate::fix::FIX_PIPELINE_MAX_PASSES {
        let input = current.as_mut().unwrap_or(source);
        let rejected = prepared
            .is_err()
            .then(|| input.analysis().text().to_owned());
        let (mut converted, rules) =
            prepared.unwrap_or_default().unwrap_or_else(|| {
                (
                    std::mem::replace(input, TextVersion::new(String::new()))
                        .into_owned(),
                    Vec::new(),
                )
            });
        if let Some(quoted) = prepare_quotes(converted.analysis(), passes)? {
            converted = quoted;
            tracking.edited.push(quoted_strings::ID);
        }
        let output = format_document(&mut converted, passes, tracking)?;
        tracking.edited.extend(rules);
        if rejected.as_deref() == Some(output.analysis().text()) {
            return Err(Fallback::Verification);
        }
        prepared = collection_style::prepare(
            output.analysis(),
            cfg,
            passes.indentation.as_ref(),
        );
        if matches!(prepared, Ok(None)) {
            result = Ok(output);
            break;
        }
        current = Some(output);
    }
    result
}

fn format_document(
    source: &mut TextVersion<'_>,
    passes: &Passes<'_>,
    tracking: &mut PassTracking<'_>,
) -> Result<TextVersion<'static>, Fallback> {
    let mut quotes = HashMap::new();
    let (prepared, kept) =
        DocumentPlan::new(source.analysis(), passes, &mut quotes, Sink::Emit)
            .and_then(|plan| {
                let consistent = plan.dirty
                    || !passes.indentation.is_some_and(|cfg| {
                        cfg.requires_consistency() && !plan.layout.consistent(&cfg)
                    });
                let kept = plan.layout.kept_dash_lines.clone();
                consistent
                    .then_some(())
                    .ok_or(Fallback::Refusal)
                    .and_then(|()| {
                        plan.dirty
                            .then(|| {
                                plan.emit().map(|text| (text, plan.styles, plan.rules))
                            })
                            .transpose()
                            .map(|prepared| (prepared, kept))
                    })
            })?;
    let Some((text, styles, rules)) = prepared else {
        remember(tracking, source, passes.indentation, kept);
        source.release_tokens();
        source.mark_strict_valid();
        return Ok(
            std::mem::replace(source, TextVersion::new(String::new())).into_owned()
        );
    };
    source.release_layout();
    let input = source.analysis();
    let mut output = TextVersion::new(text);
    stats::count(stats::Counter::Emit);
    let expected = expected_events(input, &styles, passes.document_start.is_some());
    if !input
        .verify_format_events(output.analysis(), expected)
        .unwrap_or(false)
    {
        stats::count(stats::Counter::Rollback);
        return Err(Fallback::Verification);
    }
    output.mark_strict_valid();
    source.take_facts();
    let dry = DocumentPlan::new(output.analysis(), passes, &mut quotes, Sink::Validate)
        .inspect_err(|_| {
            stats::count(stats::Counter::Rollback);
        })?;
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
    drop(dry);
    output.release_tokens();
    tracking.edited.extend(rules);
    let plan = source.analysis().plan_version(output);
    source
        .apply(plan)
        .expect("layout plan belongs to the input epoch");
    Ok(std::mem::replace(source, TextVersion::new(String::new())).into_owned())
}

fn expected_events<'a>(
    input: Analysis<'a>,
    styles: &'a [(usize, ScalarStyle)],
    document_start: bool,
) -> impl Iterator<Item = Event<'a>> {
    input.event_iter().map(move |(event, span)| match event {
        Event::Scalar(value, style, anchor, tag) => {
            let at =
                crate::rules::support::span_utils::marker_byte_offset(span.start).get();
            let style = styles
                .binary_search_by_key(&at, |(at, _)| *at)
                .map_or(style, |index| styles[index].1);
            Event::Scalar(value, style, anchor, tag)
        }
        Event::DocumentStart(explicit, version) => {
            Event::DocumentStart(explicit || document_start, version)
        }
        other => other,
    })
}

fn prepare_quotes(
    input: Analysis<'_>,
    passes: &Passes<'_>,
) -> Result<Option<TextVersion<'static>>, Fallback> {
    if passes.line_length.is_none() {
        return Ok(None);
    }
    let mut presentation = Presentation::quotes(input, passes, &mut HashMap::new())?;
    presentation.settle()?;
    if presentation.edits.is_empty() {
        return Ok(None);
    }
    let mut text = String::with_capacity(input.text().len());
    let mut position = 0;
    for (from, to, replacement) in &presentation.edits {
        text.push_str(&input.text()[position..from.get()]);
        replacement.piece().append(&mut text);
        position = to.get();
    }
    text.push_str(&input.text()[position..]);
    let mut output = TextVersion::new(text);
    let expected = expected_events(input, &presentation.styles, false);
    if !input
        .verify_format_events(output.analysis(), expected)
        .unwrap_or(false)
    {
        stats::count(stats::Counter::Rollback);
        return Err(Fallback::Verification);
    }
    output.mark_strict_valid();
    Ok(Some(output))
}

fn remember(
    tracking: &mut PassTracking<'_>,
    text: &TextVersion<'_>,
    target: Option<indentation::Config>,
    kept_dash_lines: Vec<(usize, usize)>,
) {
    if let (Some(slot), Some(target)) = (tracking.reindented.as_deref_mut(), target) {
        *slot = Some(crate::fix::ReindentEvidence::new(
            text.analysis(),
            target,
            Vec::new(),
            kept_dash_lines,
        ));
    }
}

struct Presentation {
    edits: Vec<PlanEdit>,
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
        let mut presentation = Self::pre_scan(input, passes, cache)?;
        presentation.comments(input, passes, layout);
        for (rule, edits) in [
            (
                colons::ID,
                passes
                    .colons
                    .as_ref()
                    .map_or_else(Vec::new, |cfg| colons::edits(input, cfg)),
            ),
            (
                hyphens::ID,
                passes
                    .hyphens
                    .as_ref()
                    .map_or_else(Vec::new, |cfg| hyphens::edits(input, cfg)),
            ),
        ] {
            presentation.extend(rule, edits);
        }
        presentation.settle().map(|()| presentation)
    }

    fn pre_scan(
        input: Analysis<'_>,
        passes: &Passes<'_>,
        cache: &mut HashMap<String, Option<bool>>,
    ) -> Result<Self, Fallback> {
        let mut presentation = Self::quotes(input, passes, cache)?;
        let mut braces = passes
            .braces
            .as_ref()
            .and_then(|cfg| braces::edit_sites(cfg, input.text()));
        let templates = input.text().contains("{{");
        let mut brackets = passes
            .brackets
            .as_ref()
            .filter(|_| !templates)
            .and_then(|cfg| brackets::edit_sites(cfg, input.text()));
        let mut comma_edits = Vec::new();
        let mut depth = 0usize;
        for &(at, ch) in input.flow_sites() {
            if let Some(edits) = &mut braces {
                edits.push(at, ch);
            }
            if let Some(edits) = &mut brackets {
                edits.push(at, ch);
            }
            match ch {
                b'[' | b'{' => depth += 1,
                b']' | b'}' => depth = depth.saturating_sub(1),
                b',' if depth > 0 => {
                    if let Some(cfg) = &passes.commas {
                        commas::collect_comma_fixes(
                            cfg,
                            input.text().as_bytes(),
                            at,
                            &mut comma_edits,
                        );
                    }
                }
                _ => {}
            }
        }
        presentation.extend_spaces(commas::ID, comma_edits);
        if let Some(edits) = braces {
            presentation.extend_spaces(braces::ID, edits.finish());
        }
        if let Some(edits) = brackets {
            presentation.extend_spaces(brackets::ID, edits.finish());
        }
        if templates && let Some(cfg) = &passes.brackets {
            presentation.extend(brackets::ID, brackets::edits(input, cfg));
        }
        presentation.settle()?;
        Ok(presentation)
    }

    fn extend(&mut self, rule: &'static str, edits: Vec<Edit>) {
        if !edits.is_empty() {
            self.rules.push(rule);
        }
        self.edits.extend(edits.into_iter().map(plan_edit));
    }

    fn extend_spaces(
        &mut self,
        rule: &'static str,
        edits: Vec<crate::rules::support::token_spacing::SpaceEdit>,
    ) {
        if !edits.is_empty() {
            self.rules.push(rule);
        }
        self.edits.extend(
            edits.into_iter().map(|(from, to, spaces)| {
                (from, to, pieces::Replacement::Spaces(spaces))
            }),
        );
    }

    fn comments(
        &mut self,
        input: Analysis<'_>,
        passes: &Passes<'_>,
        layout: &mut indentation::Layout<'_>,
    ) {
        if let Some(cfg) = &passes.comments {
            self.extend(comments::ID, comments::edits(input, cfg));
        }
        if let Some(cfg) = passes.comments_indentation {
            let prefixes = comments_indentation::edits(input, cfg);
            if !prefixes.is_empty() {
                self.rules.push(comments_indentation::ID);
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
    }

    fn settle(&mut self) -> Result<(), Fallback> {
        self.edits
            .sort_by_key(|(start, end, _)| (start.get(), end.get()));
        if self
            .edits
            .windows(2)
            .any(|pair| pair[0].1 > pair[1].0 || pair[0].0 == pair[1].0)
        {
            return Err(Fallback::Overlap);
        }

        self.rules.sort_by_key(|rule| {
            super::FORMAT_RULE_IDS
                .iter()
                .position(|candidate| candidate == rule)
        });
        self.rules.dedup();
        Ok(())
    }

    fn stream<'a>(
        input: Analysis<'a>,
        passes: &Passes<'_>,
        cache: &mut HashMap<String, Option<bool>>,
        cfg: &indentation::Config,
    ) -> Result<(indentation::Layout<'a>, indentation::Targets, Self), Fallback> {
        let mut presentation = Self::pre_scan(input, passes, cache)?;
        let mut layout = indentation::Layout::format(input, cfg);
        if !layout.collapse_scalars(&presentation.edits) {
            return Err(Fallback::ScalarInteraction);
        }
        layout
            .stream_targets(cfg, &presentation.edits, passes)
            .and_then(|(targets, spacing)| {
                for site in spacing {
                    let dash = site.indicator
                        == crate::rules::support::token_spacing::Indicator::Dash;
                    presentation.rules.push(if dash {
                        hyphens::ID
                    } else {
                        colons::ID
                    });
                    let (from, to, spaces) = site.edit;
                    presentation.edits.push((
                        from,
                        to,
                        pieces::Replacement::Spaces(spaces),
                    ));
                }
                presentation.comments(input, passes, &mut layout);
                presentation.settle().map(|()| {
                    let targets = targets.finish(&layout);
                    (layout, targets, presentation)
                })
            })
    }

    fn quotes(
        input: Analysis<'_>,
        passes: &Passes<'_>,
        quotes: &mut HashMap<String, Option<bool>>,
    ) -> Result<Self, Fallback> {
        let mut edits = Vec::new();
        let mut styles = Vec::new();
        let result = passes.quoted_strings.as_ref().map_or(Ok(()), |cfg| {
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
            next.into_iter().try_for_each(|(start, end, text)| {
                if text.contains(['\n', '\r']) {
                    multiline_edits(input.text(), start, end, &text)
                        .map(|next| edits.extend(next))
                        .ok_or(Fallback::ScalarInteraction)
                } else {
                    edits.push((start, end, text));
                    Ok(())
                }
            })
        });
        result.map(|()| {
            let rules = if edits.is_empty() {
                Vec::new()
            } else {
                vec![quoted_strings::ID]
            };
            Self {
                edits: edits.into_iter().map(plan_edit).collect(),
                styles,
                rules,
            }
        })
    }
}

impl<'a> DocumentPlan<'a> {
    fn new(
        input: Analysis<'a>,
        passes: &Passes<'_>,
        quotes: &mut HashMap<String, Option<bool>>,
        sink: Sink,
    ) -> Result<Self, Fallback> {
        stats::count(stats::Counter::Plan);
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
        let Some(cfg) = passes.indentation else {
            return Err(Fallback::Refusal);
        };
        let (layout, targets, presentation) = if input.text().contains('\u{feff}') {
            facts::prepare_input(input, passes.line_length.is_some()).and_then(|()| {
                let mut layout = indentation::Layout::new(input, &cfg);
                let presentation =
                    Presentation::new(input, passes, &mut layout, quotes)?;
                if !layout.collapse_scalars(&presentation.edits) {
                    return Err(Fallback::ScalarInteraction);
                }
                let targets = layout.targets(
                    &cfg,
                    &presentation.edits,
                    passes.line_length.is_some(),
                );
                Ok((layout, targets, presentation))
            })
        } else {
            if sink == Sink::Validate {
                stats::count(stats::Counter::DryScan);
            }
            Presentation::stream(input, passes, quotes, &cfg)
        }?;
        if crate::rules::block_scalar_chomping::ambiguous_blank_body_line_with_analysis(
            input,
        )
        .is_some()
        {
            return Err(Fallback::ScalarInteraction);
        }
        let Presentation {
            edits,
            styles,
            mut rules,
        } = presentation;
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
            edits,
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
        let scalar_owners: Vec<_> = layout
            .lines
            .iter()
            .filter_map(|line| {
                line.bridge
                    .as_ref()
                    .map(|bridge| (bridge.start, bridge.end))
            })
            .collect();
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
        let disjoint = !edits
            .windows(2)
            .any(|pair| pair[0].1 > pair[1].0 || pair[0].0 == pair[1].0);
        boundaries.sort_by_key(|(start, _, _)| start.get());
        styles.sort_by_key(|(start, _)| *start);
        self.dirty = !rules.is_empty();
        disjoint
            .then_some(())
            .ok_or(Fallback::Overlap)
            .and_then(|()| {
                if let Some(cfg) = passes.line_length {
                    self.plan_folds(cfg)?;
                }
                if !self.folds.is_empty() {
                    self.rules.push(crate::rules::line_length::ID);
                    self.dirty = true;
                }
                Ok(())
            })
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
        let maximum = |seen| if seen { cfg.max() } else { cfg.max_start() };
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
                    maximum(seen),
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
            if line.is_empty() && !protected[line.origin] {
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
                maximum(seen),
            );
            seen |= !line.is_empty();
        }
        let maximum = if boundary < self.boundaries.len() {
            maximum(seen)
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

    fn byte_on_line(
        &self,
        index: usize,
        byte: usize,
        projection: &projection::Projection,
    ) -> Option<usize> {
        let line = &self.layout.lines[index];
        let source = self.input.text();
        let mut position = self.prefix(index) + usize::from(line.bom) * 3;
        let mut valid = true;
        [line.body, line.tail]
            .into_iter()
            .enumerate()
            .find_map(|(piece, part)| {
                valid
                    .then(|| {
                        let bridge = pieces::bridge_width(
                            line.bridge.as_deref().filter(|_| piece == 1),
                            byte,
                        );
                        valid &= bridge.is_some();
                        position += bridge.unwrap_or_default();
                        if part.is_empty() {
                            return None;
                        }
                        let start = part.as_ptr() as usize - source.as_ptr() as usize;
                        let end = start + part.len();
                        valid &= byte >= start;
                        valid
                            .then(|| {
                                position = projection.byte_position(
                                    self.input,
                                    start,
                                    byte.min(end),
                                    position,
                                );
                                (byte <= end).then(|| self.close_gaps(index, position))
                            })
                            .flatten()
                    })
                    .flatten()
            })
    }

    fn close_gaps(&self, index: usize, mut position: usize) -> usize {
        let raw = position.saturating_add_signed(-self.targets.deltas[index]);
        let first = self.targets.gaps.partition_point(|gap| gap.line < index);
        for gap in self.targets.gaps[first..]
            .iter()
            .take_while(|gap| gap.line == index)
        {
            position -= gap.removed.min(raw.saturating_sub(gap.column + 2));
        }
        position
    }

    fn plan_folds(
        &mut self,
        cfg: crate::rules::line_length::Fold,
    ) -> Result<(), Fallback> {
        let projection = projection::Projection::new(self.input, &self.edits);
        for scalar in crate::rules::line_length::fold::foldable_scalars(self.input) {
            self.plan_scalar_fold(scalar.content, scalar.style, cfg, &projection)?;
        }
        self.folds.sort_by_key(|site| (site.line, site.byte));
        Ok(())
    }

    fn plan_scalar_fold(
        &mut self,
        range: std::ops::Range<usize>,
        original_style: ScalarStyle,
        cfg: crate::rules::line_length::Fold,
        projection: &projection::Projection,
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
        self.byte_on_line(first, full.start, projection)
            .map(|at| at + quotes)
            .ok_or(Fallback::ScalarInteraction)
            .and_then(|content| {
                let owner = self
                    .targets
                    .scalar_owners
                    .partition_point(|&(at, _)| at <= full.start)
                    .checked_sub(1)
                    .map_or(0, |index| self.targets.scalar_owners[index].1);
                let folded = style == ScalarStyle::Folded;
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
                for index in (first..=last).filter(|_| !folded || content > owner) {
                    self.plan_fold_line(index, &scalar, projection)?;
                }
                Ok(())
            })
    }

    fn plan_fold_line(
        &mut self,
        index: usize,
        scalar: &ScalarFold,
        projection: &projection::Projection,
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
        let bom = if line.bom { "\u{feff}" } else { "" };
        let mut parts = Vec::new();
        let result = self
            .visit_body(index, |part| parts.push(part))
            .and_then(|()| {
                let prefix = self.prefix(index);
                let length = prefix
                    + bom.len()
                    + parts.iter().map(|part| part.len()).sum::<usize>();
                let scalar = if folded {
                    indent..length
                } else {
                    let start = if index == first { content } else { 0 };
                    let end = if index == last {
                        self.byte_on_line(index, full.end, projection)
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
                    return Ok(Vec::new());
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
                let breaks = breaks_from_chars(
                    chars,
                    &scalar,
                    style,
                    indent,
                    usize::from(width),
                );
                Ok(breaks)
            });
        result.map(|breaks| {
            self.folds.extend(breaks.into_iter().map(|byte| FoldBreak {
                line: index,
                byte,
                indent,
            }));
        })
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
    input
        .get_line(input.line_count().saturating_sub(1))
        .is_some_and(|line| line.bytes().all(|byte| byte == b' '))
        && input
            .line_contents_from(0)
            .rev()
            .take_while(|line| line.trim_matches([' ', '\t']).is_empty())
            .take(2)
            .count()
            == 2
        && input.headers().iter().any(|header| {
            let line = header.span.start.line() - 1;
            input
                .line_content(header.line - 1)
                .chars()
                .nth(header.column - 1)
                == Some('>')
                && targets.deltas[layout.project_line(
                    line,
                    crate::rules::support::span_utils::marker_byte_offset(
                        header.span.start,
                    )
                    .get(),
                )] < 0
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
    let mut offset = start.get();
    let mut replacement_end = 0;
    let mut edits = Vec::new();
    for ((_, old, ending), (_, new, replacement_break)) in
        split_lines_preserve_endings(original)
            .zip(split_lines_preserve_endings(replacement))
    {
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
        replacement_end += new.len() + replacement_break.len();
    }
    (offset == end.get() && replacement_end == replacement.len()).then_some(edits)
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

pub(crate) struct Bridge {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) bytes: usize,
    pub(crate) characters: usize,
    pub(crate) prefix_column: usize,
    pub(crate) end_position: Option<(usize, usize)>,
}

pub(crate) struct VirtualLine<'a> {
    pub(crate) origin: usize,
    pub(crate) prefix: usize,
    pub(crate) body: &'a str,
    pub(crate) tail: &'a str,
    pub(crate) ending: &'a str,
    pub(crate) bom: bool,
    pub(crate) bridge: Option<Box<Bridge>>,
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
    let mut gap = 0;
    let mut column = line.prefix;
    let mut append = |text: Piece<'a>| -> Result<(), Fallback> {
        let mut from = 0;
        let mut valid = true;
        let mut complete = true;
        while valid
            && complete
            && gap < gaps.len()
            && gaps[gap].column + 2 < column + text.len()
        {
            let site = &gaps[gap];
            let start = (site.column + 2).saturating_sub(column);
            let end = (site.column + 2 + site.removed)
                .saturating_sub(column)
                .min(text.len());
            let slices = text.get(from..start).zip(text.get(end..text.len()));
            valid = slices
                .map(|(prefix, _)| {
                    write(prefix);
                    from = end;
                    complete = column + end == site.column + 2 + site.removed;
                    gap += usize::from(complete);
                })
                .is_some();
        }
        text.get(from..text.len())
            .filter(|_| valid)
            .map(|tail| {
                write(tail);
                column += text.len();
            })
            .ok_or(Fallback::Overlap)
    };
    [line.body, line.tail]
        .into_iter()
        .enumerate()
        .try_for_each(|(piece, part)| {
            let bridge = if piece == 1 {
                line.bridge.as_ref().map_or(Ok(()), |bridge| {
                    let edit = &edits[edits
                        .partition_point(|(from, _, _)| from.get() < bridge.start)];
                    ((edit.0.get(), edit.1.get()) == (bridge.start, bridge.end))
                        .then_some(())
                        .ok_or(Fallback::Overlap)
                        .and_then(|()| append(edit.2.piece()))
                })
            } else {
                Ok(())
            };
            bridge.and_then(|()| {
                if part.is_empty() {
                    return Ok(());
                }
                let start = part.as_ptr() as usize - source.as_ptr() as usize;
                let end = start + part.len();
                let mut at = start;
                let first = edits.partition_point(|(from, end, _)| {
                    end.get() < start || (end.get() == start && from < end)
                });
                edits[first..]
                    .iter()
                    .take_while(|(from, _, _)| from.get() <= end)
                    .try_for_each(|(from, to, text)| {
                        if line.bridge.as_ref().is_some_and(|bridge| {
                            (bridge.start, bridge.end) == (from.get(), to.get())
                        }) {
                            return Ok(());
                        }
                        if from.get() < start || to.get() > end {
                            return Err(Fallback::Overlap);
                        }
                        let result = append(Piece::Text(&source[at..from.get()]))
                            .and_then(|()| append(text.piece()));
                        at = to.get();
                        result
                    })
                    .and_then(|()| append(Piece::Text(&source[at..end])))
            })
        })
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
