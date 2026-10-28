use std::collections::HashMap;

use crate::fix::{PassTracking, Passes};
use crate::rules::support::collection_style;
use crate::rules::support::span_utils::BytePos;
use crate::rules::{
    braces, brackets, colons, commas, comments, comments_indentation, document_end,
    document_start, empty_lines, hyphens, indentation, new_lines, quoted_strings,
    trailing_spaces,
};
use crate::text_version::Analysis;
use crate::text_version::TextVersion;
use crate::text_version::edits::Edit;

pub(crate) mod facts;
mod folding;
pub(crate) mod frame;
mod guard;
pub(crate) mod line_endings;
use folding::FoldBreak;
use guard::Guard;
pub(crate) use guard::Permission;
mod pieces;
pub(crate) mod projection;
pub(crate) use pieces::Piece;
pub(crate) use pieces::PlanEdit;
pub(crate) use pieces::Replacement;
pub(crate) use pieces::RetainedPart;
use pieces::plan_edit;
pub(crate) mod stats;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Fallback {
    ScalarInteraction,
    Overlap,
    Refusal,
    Verification,
    DryEdits,
    Ambiguous,
}

struct LayoutTargets<'a>(DocumentPlan<'a>);
struct LineTargets<'a>(DocumentPlan<'a>);
struct EmitPlan<'a>(DocumentPlan<'a>);

struct LayoutCandidates<'a> {
    input: Analysis<'a>,
    layout: indentation::Layout<'a>,
    targets: indentation::Candidates,
    presentation: Presentation,
    config: indentation::Config,
}

struct DocumentPlan<'a> {
    input: Analysis<'a>,
    layout: indentation::Layout<'a>,
    targets: indentation::Targets,
    edits: Vec<PlanEdit>,
    boundaries: Vec<Edit>,
    styles: quoted_strings::Styles,
    magic: comments::Magic,
    rules: Vec<&'static str>,
    endings: line_endings::Target,
    dirty: bool,
    removed: Vec<std::ops::Range<usize>>,
    folds: Vec<FoldBreak>,
    refused: Vec<indentation::Refusal>,
    inconsistent: Vec<indentation::Violation>,
    guard: Option<Box<Guard>>,
}

struct FoldSite {
    full: std::ops::Range<usize>,
    quotes: usize,
    tail: folding::Tail,
    before_indent: bool,
}

pub(crate) fn format(
    source: &mut TextVersion<'_>,
    passes: &Passes<'_>,
    tracking: &mut PassTracking<'_>,
) -> Result<TextVersion<'static>, Fallback> {
    if passes.preserve_events {
        return format_document_refused(source, passes, tracking)
            .map(|(output, _)| output);
    }
    let mut current = None;
    (0..crate::fix::FIX_PIPELINE_MAX_PASSES)
        .find_map(|_| {
            let input = current.as_mut().unwrap_or(&mut *source);
            match format_document_refused(input, passes, tracking) {
                Ok((output, false)) => {
                    current = Some(output);
                    None
                }
                result => Some(result.map(|(output, _)| output)),
            }
        })
        .unwrap_or(Err(Fallback::Refusal))
}

/// Refuses the rewrite pass's input once, before its first plan, when it is ambiguous.
fn probe(tracking: &mut PassTracking<'_>, input: Analysis<'_>) -> Result<(), Fallback> {
    if std::mem::take(&mut tracking.unprobed) && crate::fix::ambiguous(input) {
        Err(Fallback::Ambiguous)
    } else {
        Ok(())
    }
}

type Candidate = (String, Vec<&'static str>);
/// What [`crate::format::unfixed`] reports, on the plan's output lines: each kept dash,
/// each refused document, and each consistent-width violation.
type Kept = (
    Vec<(usize, usize)>,
    Vec<indentation::Refusal>,
    Vec<(usize, usize)>,
);

/// The emitted candidate, or `None` for a clean plan, with the layout facts to remember.
fn prepare_candidate(
    input: Analysis<'_>,
    passes: &Passes<'_>,
    tracking: &mut PassTracking<'_>,
    quotes: &mut HashMap<String, Option<bool>>,
) -> Result<(Option<Candidate>, Kept), Fallback> {
    if let Some(cfg) = passes.collection_style {
        let (mut collections, observed) = collection_style::settled_with_view(
            input,
            cfg,
            passes.indentation.as_ref(),
        );
        if collections.has_candidates() {
            return prepare_converted(
                input,
                passes,
                tracking,
                quotes,
                cfg,
                &mut collections,
                observed,
            );
        }
    }
    if blank_stream(input.text(), passes) {
        stats::count(stats::Counter::Plan);
        let candidate = (String::new(), vec![empty_lines::ID]);
        Ok((Some(candidate), Kept::default()))
    } else {
        let EmitPlan(plan) = EmitPlan::new(input, passes, quotes)?;
        probe(tracking, plan.input)?;
        let consistent = plan.dirty
            || !passes.indentation.is_some_and(|cfg| {
                cfg.requires_consistency()
                    && !plan.layout.consistent_except(&cfg, &plan.refused)
            });
        consistent
            .then_some(())
            .ok_or(Fallback::Refusal)
            .and_then(|()| {
                let kept = plan.kept();
                if !plan.dirty {
                    return Ok((None, kept));
                }
                plan.emit().map(|text| (Some((text, plan.rules)), kept))
            })
    }
}

fn prepare_converted<'a>(
    input: Analysis<'a>,
    passes: &Passes<'_>,
    tracking: &mut PassTracking<'_>,
    cache: &mut HashMap<String, Option<bool>>,
    config: collection_style::Config,
    collections: &mut collection_style::Plan,
    observed: Option<ConvertedLines<'a>>,
) -> Result<(Option<Candidate>, Kept), Fallback> {
    probe(tracking, input)?;
    if !collections.is_empty() {
        stats::count(stats::Counter::Plan);
        return prepare_selected(
            input,
            passes,
            cache,
            config,
            collections,
            true,
            observed,
        );
    }
    LayoutCandidates::observe(input, passes, cache)
        .and_then(|candidates| candidates.settle(passes))
        .and_then(|LayoutTargets(plan)| {
            let trailing = plan.settled_source_edits(passes);
            if collections.reconsider(
                input,
                config,
                &plan.edits,
                &plan.styles,
                (&plan.layout, plan.targets.candidates()),
                &trailing,
            ) {
                drop(plan);
                prepare_selected(input, passes, cache, config, collections, false, None)
            } else {
                finish_candidate(
                    LayoutTargets(plan),
                    passes,
                    std::mem::take(collections),
                )
            }
        })
}

fn prepare_selected<'a>(
    input: Analysis<'a>,
    passes: &Passes<'_>,
    cache: &mut HashMap<String, Option<bool>>,
    config: collection_style::Config,
    collections: &mut collection_style::Plan,
    initially_selected: bool,
    observed: Option<ConvertedLines<'a>>,
) -> Result<(Option<Candidate>, Kept), Fallback> {
    converted_presentation(input, passes, cache, collections, observed).and_then(
        |(mut presentation, retained)| {
            let spellings = std::mem::take(&mut presentation.edits);
            let (frame, rows) = retained.unwrap_or_else(|| {
                frame::Frame::with_edits(
                    input,
                    input.byte_line_starts(),
                    collections,
                    &spellings,
                )
            });
            converted_targets(input, passes, frame, rows, presentation).and_then(
                |LayoutTargets(plan)| {
                    let trailing = plan.settled_source_edits(passes);
                    let native = initially_selected
                        && collections.reconsider(
                            input,
                            config,
                            &spellings,
                            &plan.styles,
                            (&plan.layout, plan.targets.candidates()),
                            &trailing,
                        );
                    let implicit = collections.close_implicit(
                        input,
                        config,
                        &spellings,
                        &plan.styles,
                        &plan.layout,
                        plan.targets.candidates(),
                    );
                    if native || implicit {
                        drop(plan);
                        finish_selected(input, passes, cache, config, collections)
                    } else {
                        finish_candidate(
                            LayoutTargets(plan),
                            passes,
                            std::mem::take(collections),
                        )
                    }
                },
            )
        },
    )
}

fn finish_selected(
    input: Analysis<'_>,
    passes: &Passes<'_>,
    cache: &mut HashMap<String, Option<bool>>,
    config: collection_style::Config,
    collections: &mut collection_style::Plan,
) -> Result<(Option<Candidate>, Kept), Fallback> {
    converted_presentation(input, passes, cache, collections, None).and_then(
        |(mut presentation, retained)| {
            let spellings = std::mem::take(&mut presentation.edits);
            let (frame, rows) = retained.unwrap_or_else(|| {
                frame::Frame::with_edits(
                    input,
                    input.byte_line_starts(),
                    collections,
                    &spellings,
                )
            });
            converted_targets(input, passes, frame, rows, presentation).and_then(
                |LayoutTargets(plan)| {
                    if !collections.close_implicit(
                        input,
                        config,
                        &spellings,
                        &plan.styles,
                        &plan.layout,
                        plan.targets.candidates(),
                    ) {
                        return finish_candidate(
                            LayoutTargets(plan),
                            passes,
                            std::mem::take(collections),
                        );
                    }
                    let mut presentation = Presentation {
                        edits: Vec::new(),
                        styles: plan.styles,
                        magic: plan.magic,
                        rules: plan.rules,
                        guard: plan.guard,
                        retained: None,
                    };
                    presentation.collection_spacing(passes, collections);
                    let (frame, rows) = frame::Frame::with_edits(
                        input,
                        input.byte_line_starts(),
                        collections,
                        &spellings,
                    );
                    converted_targets(input, passes, frame, rows, presentation)
                        .and_then(|targets| {
                            finish_candidate(
                                targets,
                                passes,
                                std::mem::take(collections),
                            )
                        })
                },
            )
        },
    )
}

type ConvertedLines<'a> = frame::FrameRows<'a>;

fn converted_presentation<'a>(
    input: Analysis<'a>,
    passes: &Passes<'_>,
    cache: &mut HashMap<String, Option<bool>>,
    collections: &mut collection_style::Plan,
    observed: Option<ConvertedLines<'a>>,
) -> Result<(Presentation, Option<ConvertedLines<'a>>), Fallback> {
    let prefixes: Vec<_> = passes
        .comments_indentation
        .map_or_else(Vec::new, |cfg| comments_indentation::edits(input, cfg))
        .into_iter()
        .map(plan_edit)
        .collect();
    let (presentation, retained) = if prefixes.is_empty() {
        let view = frame::View::new(input, collections, &[], observed);
        let presentation =
            Presentation::pre_scan_view(input, passes, cache, Some(&view));
        (presentation, view.into_lines())
    } else {
        drop(observed);
        let view = frame::View::new(input, collections, &prefixes, None);
        (
            Presentation::pre_scan_view(input, passes, cache, Some(&view)),
            None,
        )
    };
    presentation.and_then(|mut presentation| {
        presentation.spacing(input, passes.colons, passes.hyphens);
        presentation.edits.extend(prefixes);
        presentation.settle().map(|()| {
            let changed = presentation.collection_spacing(passes, collections);
            let retained =
                retained.filter(|_| presentation.edits.is_empty() && !changed);
            (presentation, retained)
        })
    })
}

fn converted_targets<'a>(
    input: Analysis<'a>,
    passes: &Passes<'_>,
    frame: frame::Frame,
    rows: Vec<VirtualLine<'a>>,
    mut presentation: Presentation,
) -> Result<LayoutTargets<'a>, Fallback> {
    let cfg = passes.indentation.unwrap_or_else(|| {
        indentation::Config::new(
            indentation::SpacesSetting::Consistent,
            indentation::IndentSequencesSetting::Whatever,
            false,
        )
    });
    let mut layout = indentation::Layout::converted(input, &cfg, frame, rows);
    layout.trimmed = passes.trailing_spaces;
    layout.restyle(presentation.restyled());
    presentation.comments(input, passes, &mut layout);
    presentation
        .settle()
        .and_then(|()| {
            layout.prepared_targets(
                &cfg,
                &presentation.edits,
                &presentation.styles,
                passes.line_length.is_some(),
            )
        })
        .and_then(|mut targets| {
            comments_indentation::align_projected(
                input,
                passes.comments_indentation,
                &layout,
                &mut targets,
                Guard::permission(
                    presentation.guard.as_deref(),
                    comments_indentation::ID,
                ),
            );
            presentation.align_trailing_comments(input, passes, &layout, &mut targets);
            LayoutCandidates {
                input,
                layout,
                targets,
                presentation,
                config: cfg,
            }
            .settle(passes)
        })
}

fn finish_candidate(
    targets: LayoutTargets<'_>,
    passes: &Passes<'_>,
    collections: collection_style::Plan,
) -> Result<(Option<Candidate>, Kept), Fallback> {
    let selected = !collections.is_empty();
    let rules: Vec<_> = collections.rules().collect();
    drop(collections);
    targets
        .finish_lines(passes)
        .and_then(|targets| targets.fold(passes))
        .and_then(|EmitPlan(mut plan)| {
            let kept = plan.kept();
            if !plan.dirty && !selected {
                return Ok((None, kept));
            }
            plan.rules.extend(rules);
            plan.emit().map(|text| (Some((text, plan.rules)), kept))
        })
}

/// The plan of `source` emitted, and whether it settled: a format plan always does, while
/// a lint fix, whose rule options can contradict each other, settles once its output
/// replans clean.
fn format_document_refused(
    source: &mut TextVersion<'_>,
    passes: &Passes<'_>,
    tracking: &mut PassTracking<'_>,
) -> Result<(TextVersion<'static>, bool), Fallback> {
    let mut quotes = HashMap::new();
    let (prepared, kept) =
        prepare_candidate(source.analysis(), passes, tracking, &mut quotes)?;
    let Some((text, rules)) = prepared else {
        remember(tracking, source, passes.indentation, kept, false);
        source.release_tokens();
        source.mark_strict_valid();
        return Ok((
            std::mem::replace(source, TextVersion::new(String::new())).into_owned(),
            true,
        ));
    };
    // The reference parser reads such a line as a marker where ryl's parser does not, so no
    // check of ours can vouch for the output.
    if crate::fix::non_ascii_marker(&text) {
        return Err(Fallback::Verification);
    }
    let unambiguous =
        !quoted_strings::protection::may_turn_ambiguous(source.analysis());
    source.release_layout();
    let mut output = TextVersion::new(text);
    stats::count(stats::Counter::Emit);
    output.mark_strict_valid();
    tracking.edited.extend(rules);
    if !passes.preserve_events
        && EmitPlan::new(output.analysis(), passes, &mut quotes)
            .is_ok_and(|replan| replan.0.dirty)
    {
        stats::count(stats::Counter::Rollback);
        return (output.analysis().text() != source.analysis().text())
            .then_some((output, false))
            .ok_or(Fallback::DryEdits);
    }
    remember(tracking, &output, passes.indentation, kept, unambiguous);
    output.release_tokens();
    *source = TextVersion::new(String::new());
    Ok((output, true))
}

/// Whether `text` is a stream of LF-only blank lines that the resolved passes empty, so its
/// candidate needs no line records; with no block scalar it cannot be ambiguous, so it skips
/// the probe.
fn blank_stream(text: &str, passes: &Passes<'_>) -> bool {
    passes.collection_style.is_none()
        && passes.per_line.is_empty()
        && passes
            .empty_lines
            .is_some_and(|cfg| empty_lines::clears_stream(text, &cfg))
}

fn remember(
    tracking: &mut PassTracking<'_>,
    text: &TextVersion<'_>,
    target: Option<indentation::Config>,
    (kept_dash_lines, refused, inconsistent): Kept,
    unambiguous: bool,
) {
    if let (Some(slot), Some(target)) = (tracking.reindented.as_deref_mut(), target) {
        *slot = Some(crate::fix::ReindentEvidence::new(
            text.analysis(),
            target,
            refused,
            kept_dash_lines,
            Some(inconsistent),
            unambiguous,
        ));
    }
}

struct Presentation {
    edits: Vec<PlanEdit>,
    styles: quoted_strings::Styles,
    magic: comments::Magic,
    rules: Vec<&'static str>,
    guard: Option<Box<Guard>>,
    retained: Option<frame::RetainedRanges>,
}

impl Presentation {
    fn collection_spacing(
        &mut self,
        passes: &Passes<'_>,
        collections: &mut collection_style::Plan,
    ) -> bool {
        let comments_changed = collections.comment_spacing(
            passes.comments.as_ref(),
            Guard::permission(self.guard.as_deref(), comments::ID),
        );
        if comments_changed {
            self.rules.push(comments::ID);
        }
        let flow_changed = collections.flow_spacing(
            &braces::closing_policy(
                passes.braces.as_ref(),
                Guard::permission(self.guard.as_deref(), braces::ID),
            ),
            &brackets::closing_policy(
                passes.brackets.as_ref(),
                Guard::permission(self.guard.as_deref(), brackets::ID),
            ),
        );
        comments_changed || flow_changed
    }

    fn new(
        input: Analysis<'_>,
        passes: &Passes<'_>,
        layout: &mut indentation::Layout<'_>,
        cache: &mut HashMap<String, Option<bool>>,
    ) -> Result<Self, Fallback> {
        Self::pre_scan(input, passes, cache).and_then(|mut presentation| {
            presentation.comments(input, passes, layout);
            presentation.spacing(input, passes.colons, passes.hyphens);
            presentation.settle().map(|()| presentation)
        })
    }

    fn restyled(&self) -> quoted_strings::Restyled {
        self.styles.restyled()
    }

    fn lint_spacing(&mut self, input: Analysis<'_>, passes: &Passes<'_>) {
        self.spacing(
            input,
            passes.colons.filter(|cfg| *cfg != colons::Config::format()),
            passes
                .hyphens
                .filter(|cfg| *cfg != hyphens::Config::format()),
        );
    }

    fn spacing(
        &mut self,
        input: Analysis<'_>,
        colons: Option<colons::Config>,
        hyphens: Option<hyphens::Config>,
    ) {
        if let Some(cfg) = colons {
            self.extend(colons::ID, colons::edits(input, &cfg));
        }
        if let Some(cfg) = hyphens {
            self.extend(hyphens::ID, hyphens::edits(input, &cfg));
        }
    }

    fn pre_scan(
        input: Analysis<'_>,
        passes: &Passes<'_>,
        cache: &mut HashMap<String, Option<bool>>,
    ) -> Result<Self, Fallback> {
        Self::pre_scan_view(input, passes, cache, None)
    }

    fn pre_scan_view(
        input: Analysis<'_>,
        passes: &Passes<'_>,
        cache: &mut HashMap<String, Option<bool>>,
        projected: Option<&frame::View<'_, '_>>,
    ) -> Result<Self, Fallback> {
        let mut presentation = Self::quote_view(input, passes, cache, projected)?;
        let mut braces = passes
            .braces
            .as_ref()
            .and_then(|cfg| braces::format_sites(cfg, input));
        let mut brackets = passes
            .brackets
            .as_ref()
            .and_then(|cfg| brackets::format_sites(cfg, input));
        let mut comma_edits = Vec::new();
        let mut closing_edits = Vec::new();
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
                    let Some(cfg) = &passes.commas else { continue };
                    commas::collect_comma_fixes(
                        cfg,
                        input.text().as_bytes(),
                        at,
                        &mut comma_edits,
                    );
                    let closing = commas::Closing::new(
                        passes.commas,
                        passes.preserve_events,
                        braces::closing_policy(
                            passes.braces.as_ref(),
                            Guard::permission(
                                presentation.guard.as_deref(),
                                braces::ID,
                            ),
                        ),
                        brackets::closing_policy(
                            passes.brackets.as_ref(),
                            Guard::permission(
                                presentation.guard.as_deref(),
                                brackets::ID,
                            ),
                        ),
                        Guard::permission(presentation.guard.as_deref(), commas::ID),
                    );
                    if closing.select(
                        input.text(),
                        at,
                        &mut comma_edits,
                        &mut closing_edits,
                    ) {
                        comma_edits.retain(|edit| edit.0.get() != at + 1);
                        comma_edits.push((BytePos::new(at), BytePos::new(at + 1), 0));
                    }
                }
                _ => {}
            }
        }
        presentation.extend_spaces(
            commas::ID,
            crate::rules::support::token_spacing::separate_keys(
                input.text(),
                comma_edits,
            ),
        );
        let close = |edits: Vec<crate::rules::support::token_spacing::SpaceEdit>| {
            crate::rules::support::token_spacing::separate_keys(
                input.text(),
                edits
                    .into_iter()
                    .filter(|(from, to, _)| {
                        closing_edits.binary_search(&(*from, *to)).is_err()
                    })
                    .collect(),
            )
        };
        for (rule, edits) in [(braces::ID, braces), (brackets::ID, brackets)] {
            if let Some(edits) = edits {
                match edits.finish() {
                    crate::rules::support::flow_collection::FormattedEdits::Spaces(
                        edits,
                    ) => presentation.extend_spaces(rule, close(edits)),
                    crate::rules::support::flow_collection::FormattedEdits::Text(
                        edits,
                    ) => presentation.extend(rule, edits),
                }
            }
        }
        presentation.settle().map(|()| presentation)
    }

    fn extend(&mut self, rule: &'static str, mut edits: Vec<Edit>) {
        if let Some(guard) = &self.guard {
            edits.retain(|(start, end, _)| guard.allows(rule, *start, *end));
        }
        if let Some(retained) = &self.retained {
            edits.retain(|(start, end, _)| retained.keeps(*start, *end));
        }
        if !edits.is_empty() {
            self.rules.push(rule);
        }
        self.edits.extend(edits.into_iter().map(plan_edit));
    }

    fn extend_spaces(
        &mut self,
        rule: &'static str,
        mut edits: Vec<crate::rules::support::token_spacing::SpaceEdit>,
    ) {
        if let Some(guard) = &self.guard {
            edits.retain(|(start, end, _)| guard.allows(rule, *start, *end));
        }
        if let Some(retained) = &self.retained {
            edits.retain(|(start, end, _)| retained.keeps(*start, *end));
        }
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
            let (edits, magic) = comments::edits_with_magic(
                input,
                cfg,
                Guard::permission(self.guard.as_deref(), comments::ID),
            );
            self.magic = magic;
            self.extend(comments::ID, edits);
        }
        if !layout.is_converted() {
            self.align_comments(input, passes, layout);
        }
    }

    fn align_comments(
        &mut self,
        input: Analysis<'_>,
        passes: &Passes<'_>,
        layout: &mut indentation::Layout<'_>,
    ) {
        if let Some(cfg) = passes.comments_indentation {
            let mut prefixes = comments_indentation::edits(input, cfg);
            if let Some(guard) = &self.guard {
                prefixes.retain(|(start, end, _)| {
                    guard.allows(comments_indentation::ID, *start, *end)
                });
            }
            if !prefixes.is_empty() {
                self.rules.push(comments_indentation::ID);
            }
            for (start, end, text) in prefixes {
                let original = input
                    .byte_line_starts()
                    .partition_point(|&at| at <= start.get())
                    - 1;
                let index = layout.project_line(original, start.get());
                let line = &mut layout.lines[index];
                let body = line.body.as_ptr() as usize - input.text().as_ptr() as usize;
                if end.get() > body {
                    line.body = &line.body[end.get() - body..];
                }
                line.prefix = text.len();
            }
        }
    }

    fn align_trailing_comments(
        &self,
        input: Analysis<'_>,
        passes: &Passes<'_>,
        layout: &indentation::Layout<'_>,
        targets: &mut indentation::Candidates,
    ) {
        comments_indentation::align_trailing(
            input,
            passes.comments_indentation,
            layout,
            targets,
            Guard::permission(self.guard.as_deref(), comments_indentation::ID),
        );
    }

    fn retain_documents(
        &mut self,
        input: Analysis<'_>,
        passes: &Passes<'_>,
        layout: &mut indentation::Layout<'_>,
        targets: &mut indentation::Candidates,
        cfg: &indentation::Config,
    ) -> Result<Vec<indentation::Violation>, Fallback> {
        if passes.indentation.is_none() {
            targets.deltas.fill(0);
            targets.gaps.clear();
            targets.refused.clear();
        }
        if passes.final_newline.is_some()
            && quoted_strings::protection::drops_final_break(input)
        {
            return Err(Fallback::Verification);
        }
        if !targets.refused.iter().any(Option::is_some) {
            return Ok(Vec::new());
        }
        let mut refused = targets.refused.clone();
        let mut reshaped = std::mem::take(&mut layout.reshaped);
        let mut violations = Vec::new();
        loop {
            reshaped.retain(|&(from, to, spaces)| {
                let line = input
                    .byte_line_starts()
                    .partition_point(|&at| at <= from.get())
                    - 1;
                if !layout.frozen(&refused, line) {
                    return true;
                }
                if from == to {
                    self.edits.retain(|edit| (edit.0, edit.1) != (from, to));
                } else if self
                    .guard
                    .as_ref()
                    .is_none_or(|guard| guard.allows(hyphens::ID, from, to))
                {
                    self.rules.push(hyphens::ID);
                    self.edits
                        .push((from, to, pieces::Replacement::Spaces(spaces)));
                }
                false
            });
            layout.keep_refused(&refused, &self.edits);
            *targets = layout.targets(
                cfg,
                &self.edits,
                &self.styles,
                passes.line_length.is_some(),
            );
            targets.refused.clone_from(&refused);
            if !cfg.requires_consistency()
                && refused.iter().all(|cause| {
                    cause.is_none_or(|cause| cause == indentation::Cause::Disabled)
                })
            {
                break;
            }
            violations =
                layout.target_violations(cfg, targets, &self.edits, &self.styles);
            if !cfg.requires_consistency()
                || !layout.refuse_violated(&violations, &mut refused)
            {
                break;
            }
        }
        self.align_comments(input, passes, layout);
        self.align_trailing_comments(input, passes, layout, targets);
        self.settle().map(|()| violations)
    }

    fn settle(&mut self) -> Result<(), Fallback> {
        self.edits
            .sort_by_key(|(start, end, _)| (start.get(), end.get()));
        let disjoint = !self
            .edits
            .windows(2)
            .any(|pair| pair[0].1 > pair[1].0 || pair[0].0 == pair[1].0)
            || {
                self.edits.dedup();
                !self.edits.windows(2).any(|pair| {
                    pair[0].1 > pair[1].0
                        || (pair[0].0 == pair[1].0 && pair[1].0 == pair[1].1)
                })
            };

        self.rules.sort_by_key(|rule| {
            super::FORMAT_RULE_IDS
                .iter()
                .position(|candidate| candidate == rule)
        });
        self.rules.dedup();
        disjoint.then_some(()).ok_or(Fallback::Overlap)
    }

    fn normalized<'a>(
        input: Analysis<'a>,
        passes: &Passes<'_>,
        cache: &mut HashMap<String, Option<bool>>,
        cfg: &indentation::Config,
    ) -> Result<(indentation::Layout<'a>, indentation::Candidates, Self), Fallback>
    {
        facts::prepare_input(input, passes.line_length.is_some()).and_then(|()| {
            let mut layout = indentation::Layout::format(input, cfg);
            layout.trimmed = passes.trailing_spaces;
            Self::new(input, passes, &mut layout, cache).map(|mut presentation| {
                for start in layout.joined_starts() {
                    let at = BytePos::new(start);
                    presentation.edits.push((at, at, Replacement::Spaces(1)));
                    layout.reshaped.push((at, at, 1));
                }
                presentation
                    .edits
                    .sort_by_key(|(start, end, _)| (*start, *end));
                layout.restyle(presentation.restyled());
                presentation.drop_split_gaps(input, &mut layout);
                layout.collapse_scalars(&presentation.edits);
                let mut targets = layout.targets(
                    cfg,
                    &presentation.edits,
                    &presentation.styles,
                    passes.line_length.is_some(),
                );
                presentation.align_trailing_comments(
                    input,
                    passes,
                    &layout,
                    &mut targets,
                );
                (layout, targets, presentation)
            })
        })
    }

    fn stream<'a>(
        input: Analysis<'a>,
        passes: &Passes<'_>,
        cache: &mut HashMap<String, Option<bool>>,
        cfg: &indentation::Config,
    ) -> Result<(indentation::Layout<'a>, indentation::Candidates, Self), Fallback>
    {
        let mut presentation = Self::pre_scan(input, passes, cache)?;
        let mut layout = indentation::Layout::format(input, cfg);
        layout.trimmed = passes.trailing_spaces;
        layout.restyle(presentation.restyled());
        layout.collapse_scalars(&presentation.edits);
        layout
            .stream_targets(cfg, &presentation.edits, &presentation.styles, passes)
            .and_then(|(targets, spacing)| {
                for site in spacing {
                    let dash = site.indicator
                        == crate::rules::support::token_spacing::Indicator::Dash;
                    let rule = if dash { hyphens::ID } else { colons::ID };
                    let (from, to, spaces) = site.edit;
                    // A space inserted after a dash is its join's, which `indentation`
                    // owns, so a refused document can drop it again.
                    let join = dash && from == to;
                    if !join
                        && presentation
                            .guard
                            .as_ref()
                            .is_some_and(|guard| !guard.allows(rule, from, to))
                    {
                        continue;
                    }
                    if !join {
                        presentation.rules.push(rule);
                    }
                    presentation.edits.push((
                        from,
                        to,
                        pieces::Replacement::Spaces(spaces),
                    ));
                }
                presentation.lint_spacing(input, passes);
                presentation.drop_split_gaps(input, &mut layout);
                presentation.comments(input, passes, &mut layout);
                presentation.settle().map(|()| {
                    let mut targets = targets.finish(&layout, &presentation.edits);
                    presentation.align_trailing_comments(
                        input,
                        passes,
                        &layout,
                        &mut targets,
                    );
                    (layout, targets, presentation)
                })
            })
    }

    /// Drops the gap after each dash whose mapping a split moves onto its own line, which
    /// starts at its target column; a refused document gets the gap back.
    fn drop_split_gaps(
        &mut self,
        input: Analysis<'_>,
        layout: &mut indentation::Layout<'_>,
    ) {
        if !layout.shaped {
            return;
        }
        let split = layout.split_targets();
        self.edits.retain(|&(from, to, ref replacement)| {
            let Replacement::Spaces(spaces) = *replacement else {
                return true;
            };
            let kept =
                split.keeps_gap(&hyphens::retained_before(input.text(), from), to);
            if !kept {
                layout.reshaped.push((from, to, spaces));
            }
            kept
        });
    }

    fn quote_view(
        input: Analysis<'_>,
        passes: &Passes<'_>,
        quotes: &mut HashMap<String, Option<bool>>,
        projected: Option<&frame::View<'_, '_>>,
    ) -> Result<Self, Fallback> {
        let mut edits = Vec::new();
        let mut styles = quoted_strings::Styles::default();
        let guard = projected.map_or_else(
            || Guard::new(input, passes),
            |view| Guard::projected(input, passes, view),
        );
        let spaced_colon = colons::Guarantees::new(
            passes.colons,
            Guard::permission(guard.as_deref(), colons::ID),
        );
        let result = passes.quoted_strings.as_ref().map_or(Ok(()), |cfg| {
            let mut next = quoted_strings::format_replacements(
                input,
                cfg,
                quotes,
                passes.preserve_events,
                &spaced_colon,
                projected.map(|view| view as &dyn quoted_strings::ConvertedLayout),
            );
            if let Some(guard) = &guard {
                let mut partial = false;
                next.retain(|(start, end, _)| {
                    let allowed = guard.allows(quoted_strings::ID, *start, *end);
                    partial |=
                        !allowed && guard.allows_any(quoted_strings::ID, *start, *end);
                    allowed
                });
                if partial {
                    stats::count(stats::Counter::Rollback);
                    return Err(Fallback::Verification);
                }
            }
            styles = quoted_strings::selected_styles(&next);
            next.into_iter().try_for_each(|(start, end, text)| {
                let text = text.into_text();
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
                magic: comments::Magic::default(),
                rules,
                guard,
                retained: projected.map(frame::View::retained_ranges),
            }
        })
    }
}

impl<'a> LayoutCandidates<'a> {
    fn observe(
        input: Analysis<'a>,
        passes: &Passes<'_>,
        quotes: &mut HashMap<String, Option<bool>>,
    ) -> Result<Self, Fallback> {
        stats::count(stats::Counter::Plan);
        let cfg = passes.indentation.unwrap_or_else(|| {
            indentation::Config::new(
                indentation::SpacesSetting::Consistent,
                indentation::IndentSequencesSetting::Whatever,
                false,
            )
        });
        let planned = if input.text().contains('\u{feff}') {
            Presentation::normalized(input, passes, quotes, &cfg)
        } else {
            Presentation::stream(input, passes, quotes, &cfg)
        };
        planned.map(|(layout, targets, presentation)| Self {
            input,
            layout,
            targets,
            presentation,
            config: cfg,
        })
    }

    fn settle(self, passes: &Passes<'_>) -> Result<LayoutTargets<'a>, Fallback> {
        let Self {
            input,
            mut layout,
            mut targets,
            mut presentation,
            config,
        } = self;
        let violations = presentation.retain_documents(
            input,
            passes,
            &mut layout,
            &mut targets,
            &config,
        )?;
        let (targets, refused) = targets.settle(&layout);
        Ok(DocumentPlan::assemble(
            input,
            passes,
            layout,
            targets,
            presentation,
            (refused, violations),
        ))
    }
}

impl<'a> EmitPlan<'a> {
    fn new(
        input: Analysis<'a>,
        passes: &Passes<'_>,
        quotes: &mut HashMap<String, Option<bool>>,
    ) -> Result<Self, Fallback> {
        LayoutCandidates::observe(input, passes, quotes)
            .and_then(|candidates| candidates.settle(passes))
            .and_then(|targets| targets.finish_lines(passes))
            .and_then(|lines| lines.fold(passes))
    }
}

impl<'a> DocumentPlan<'a> {
    fn assemble(
        input: Analysis<'a>,
        passes: &Passes<'_>,
        mut layout: indentation::Layout<'a>,
        targets: indentation::Targets,
        presentation: Presentation,
        (refused, inconsistent): (
            Vec<indentation::Refusal>,
            Vec<indentation::Violation>,
        ),
    ) -> LayoutTargets<'a> {
        let Presentation {
            edits,
            styles,
            magic,
            mut rules,
            guard,
            ..
        } = presentation;
        if targets.deltas().iter().any(|&delta| delta != 0)
            || !targets.gaps().is_empty()
            || layout.shaped
        {
            rules.push(indentation::ID);
        }
        let endings = match passes.new_lines {
            Some(cfg) => line_endings::Candidate::Configured(cfg),
            None => line_endings::Candidate::Retained(
                crate::rules::support::line_syntax::buffer_newline(input.text()),
            ),
        }
        .resolve();
        if endings.normalizes() {
            let mut changed = false;
            for line in &mut layout.lines {
                let allowed = guard.as_ref().is_none_or(|guard| {
                    guard.allows(
                        new_lines::ID,
                        line.source_anchor(input.text()),
                        line.source_anchor(input.text()),
                    )
                });
                let target = endings.line(line.ending, allowed);
                if target != line.ending {
                    line.ending = target;
                    changed = true;
                }
            }
            if changed {
                rules.push(new_lines::ID);
            }
        }
        let plan = Self {
            input,
            layout,
            targets,
            edits,
            boundaries: Vec::new(),
            styles,
            magic,
            rules,
            endings,
            dirty: false,
            removed: Vec::new(),
            folds: Vec::new(),
            refused,
            inconsistent,
            guard,
        };
        LayoutTargets(plan)
    }

    fn settled_source_edits(&self, passes: &Passes<'_>) -> Vec<PlanEdit> {
        let mut edits = self.layout.closed_source_gaps(&self.targets);
        if passes.trailing_spaces {
            edits.extend(
                trailing_spaces::edits_protected(self.input, self.layout.protection())
                    .into_iter()
                    .filter(|(start, end, _)| {
                        start < end
                            && self.layout.keeps_source_edit(*start, *end)
                            && self.guard.as_ref().is_none_or(|guard| {
                                guard.allows(trailing_spaces::ID, *start, *end)
                            })
                    })
                    .map(plan_edit),
            );
        }
        edits.sort_unstable_by_key(|(from, to, _)| (*from, *to));
        let mut merged: Vec<PlanEdit> = Vec::new();
        for (from, to, replacement) in edits {
            if let Some((_, end, _)) = merged.last_mut()
                && from <= *end
            {
                *end = (*end).max(to);
            } else {
                merged.push((from, to, replacement));
            }
        }
        merged
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
            let mut next = trailing_spaces::edits_protected(input, layout.protection());
            next.retain(|(start, end, _)| layout.keeps_source_edit(*start, *end));
            if let Some(guard) = &self.guard {
                next.retain(|(start, end, _)| {
                    guard.allows(trailing_spaces::ID, *start, *end)
                });
            }
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
                    && edit.0.get() == input.text().len()
                    && layout
                        .lines
                        .last()
                        .is_some_and(|line| !line.ending.is_empty())
                {
                    continue;
                }
                if edit.0 == edit.1 {
                    boundaries.push(edit);
                    continue;
                }
                if !blank_whitespace_line(input, layout, &self.targets, edit.0) {
                    edits.push(plan_edit(edit));
                }
            }
        }
    }

    fn markers(&mut self, passes: &Passes<'_>) {
        let final_newline = self.final_newline_enabled(passes)
            || empty_lines::retains_final_blank(
                (0..self.layout.lines.len())
                    .map(|index| self.layout.geometry(index, &self.targets)),
            );
        let keep_magic = !self.magic.changed();
        let input = self.input;

        let boundaries = &mut self.boundaries;
        let rules = &mut self.rules;
        for (rule, mut next) in [
            (
                document_start::ID,
                passes.document_start.map_or_else(Vec::new, |cfg| {
                    document_start::edits_keeping_magic(input, cfg, keep_magic)
                }),
            ),
            (
                document_end::ID,
                passes
                    .document_end
                    .map_or_else(Vec::new, |cfg| document_end::edits(input, cfg)),
            ),
        ] {
            if let Some(guard) = &self.guard {
                next.retain(|(start, end, _)| guard.allows(rule, *start, *end));
            }
            if !next.is_empty() {
                rules.push(rule);
            }
            for mut edit in next {
                if edit.0 != edit.1 {
                    blank_whitespace_line(
                        input,
                        &mut self.layout,
                        &self.targets,
                        edit.0,
                    );
                    continue;
                }
                if rule == document_start::ID {
                    attach_marker_bom(input, &mut self.layout, &self.targets, edit.0);
                }
                if rule == document_end::ID {
                    edit = document_end::finalize_boundary(input, edit, final_newline);
                }
                boundaries.push(edit);
            }
        }
    }

    fn final_newline_enabled(&self, passes: &Passes<'_>) -> bool {
        crate::rules::new_line_at_end_of_file::requirement(
            self.input.text(),
            passes.final_newline.as_ref(),
            Guard::permission(
                self.guard.as_deref(),
                crate::rules::new_line_at_end_of_file::ID,
            ),
        )
        .enabled()
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
        let protected = empty_lines::preserved_scalar_lines_with_empty(
            self.input,
            &empty_lines::BlankLines::selected(&empty),
        )
        .expect("layout input has a complete parse");
        let mut runs = empty_lines::Runs::new(*cfg, &protected);
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
                    runs.marker(),
                    self.guard.as_deref(),
                );
                while self
                    .boundaries
                    .get(boundary)
                    .is_some_and(|(start, _, _)| start.get() <= at)
                {
                    boundary += 1;
                }
            }
            let initial = empty_lines::projected(
                &self.layout.geometry(index, &self.targets),
                marker,
            );
            remove_blanks(
                self.input,
                &self.layout.lines,
                &mut self.removed,
                runs.line(index, initial),
                self.guard.as_deref(),
            );
        }
        remove_blanks(
            self.input,
            &self.layout.lines,
            &mut self.removed,
            runs.finish(boundary < self.boundaries.len()),
            self.guard.as_deref(),
        );
        if !self.removed.is_empty() {
            self.rules.push(empty_lines::ID);
        }
    }
}

impl DocumentPlan<'_> {
    fn removed(&self, line_start: usize) -> bool {
        self.removed
            .get(
                self.removed
                    .partition_point(|range| range.end <= line_start),
            )
            .is_some_and(|range| range.start <= line_start)
    }
}

impl DocumentPlan<'_> {
    fn kept(&self) -> Kept {
        let refused = &self.targets.refused();
        if self.layout.kept_dash_lines.is_empty()
            && !refused.iter().any(Option::is_some)
        {
            return Kept::default();
        }
        let starts = self.input.byte_line_starts();
        let mut lines = Vec::with_capacity(self.layout.lines.len());
        let mut markers = Vec::new();
        let (mut line, mut boundary) = (0, 0);
        for (index, virtual_line) in self.layout.lines.iter().enumerate() {
            let at = starts[virtual_line.origin];
            while let Some((from, _, text)) = self
                .boundaries
                .get(boundary)
                .filter(|edit| edit.0.get() <= at + usize::from(virtual_line.bom) * 3)
            {
                if text.starts_with("---") {
                    markers.push((
                        starts.partition_point(|&start| start <= from.get()) - 1,
                        line,
                    ));
                }
                line += text.matches('\n').count() + text.matches('\r').count()
                    - text.matches("\r\n").count();
                boundary += 1;
            }
            lines.push(line);
            if !self.removed(at) {
                line += self.folds
                    [self.folds.partition_point(|fold| fold.line < index)..]
                    .iter()
                    .take_while(|fold| fold.line == index)
                    .count()
                    + usize::from(!virtual_line.ending.is_empty());
            }
        }
        let document_starts = &self.input.parsed().document_starts;
        let start = |document: usize| {
            if document == 0 {
                return 0;
            }
            markers
                .iter()
                .find(|(marked, _)| {
                    document_starts.partition_point(|&start| start < *marked)
                        == document
                })
                .map_or_else(
                    || {
                        let source = document_starts[document];
                        lines[self.layout.project_line(source, starts[source])]
                    },
                    |&(_, line)| line,
                )
        };
        let refusals = refused
            .iter()
            .enumerate()
            .filter_map(|(document, cause)| {
                cause.map(|cause| indentation::Refusal {
                    lines: start(document) + 1..=if document + 1 < refused.len() {
                        start(document + 1)
                    } else {
                        line + 1
                    },
                    cause,
                })
            })
            .collect();
        let place = |(line, column): (usize, usize)| (lines[line - 1] + 1, column);
        (
            self.layout
                .kept_dash_targets(&self.targets, &self.edits, &self.styles)
                .into_iter()
                .map(place)
                .collect(),
            refusals,
            self.inconsistent
                .iter()
                .map(|violation| place((violation.line, violation.column)))
                .collect(),
        )
    }
}

impl DocumentPlan<'_> {
    fn finish_endings(&mut self) {
        if !self.endings.normalizes() {
            return;
        }
        for (at, _, text) in &mut self.boundaries {
            let allowed = self
                .guard
                .as_ref()
                .is_none_or(|guard| guard.allows(new_lines::ID, *at, *at));
            *text = self.endings.boundary(std::mem::take(text), allowed);
        }
    }
}

impl DocumentPlan<'_> {
    fn emit(&self) -> Result<String, Fallback> {
        let mut output = String::with_capacity(self.input.text().len());
        let fold_newline = self.endings.folding();
        let mut boundary = 0;
        let emitted =
            self.layout
                .lines
                .iter()
                .enumerate()
                .try_for_each(|(index, line)| {
                    let line_start = self.input.byte_line_starts()[line.origin];
                    if line.bom {
                        output.push('\u{feff}');
                    }
                    while self.boundaries.get(boundary).is_some_and(|edit| {
                        edit.0.get() <= line_start + usize::from(line.bom) * 3
                    }) {
                        let (_, _, text) = &self.boundaries[boundary];
                        output.push_str(text);
                        boundary += 1;
                    }
                    if self.removed(line_start) {
                        return Ok(());
                    }
                    let prefix = line
                        .prefix
                        .saturating_add_signed(self.targets.deltas()[index]);
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
                        while fold < last
                            && self.folds[fold].byte < position + text.len()
                        {
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
                    })
                    .map(|()| {
                        output.push_str(line.ending);
                    })
                });
        emitted.map(|()| {
            for (_, _, text) in &self.boundaries[boundary..] {
                output.push_str(text);
            }
            output
        })
    }
}

impl DocumentPlan<'_> {
    fn visit_body<'s>(
        &'s self,
        index: usize,
        emit: impl FnMut(Piece<'s>),
    ) -> Result<(), Fallback> {
        let first = self
            .targets
            .gaps()
            .partition_point(|gap| gap.line() < index);
        let last = self
            .targets
            .gaps()
            .partition_point(|gap| gap.line() <= index);
        visit_body(
            self.input.text(),
            &self.layout.lines[index],
            &self.targets.gaps()[first..last],
            &self.edits,
            emit,
        )
    }

    fn prefix(&self, index: usize) -> usize {
        self.layout.geometry(index, &self.targets).prefix()
    }

    fn byte_on_line(
        &self,
        index: usize,
        byte: usize,
        projection: &projection::Projection,
        end: bool,
    ) -> Option<usize> {
        if let Some(position) = self.layout.converted_position(byte, end) {
            return (position.line == index).then(|| {
                self.close_gaps(
                    index,
                    (position.byte_column
                        + usize::from(self.layout.lines[index].bom) * 3)
                        .saturating_add_signed(self.targets.deltas()[index]),
                )
            });
        }
        let line = &self.layout.lines[index];
        let source = self.input.text();
        let body = line.body.as_ptr() as usize - source.as_ptr() as usize;
        if byte <= body
            && body - byte <= line.prefix
            && (byte < body || line.body.is_empty())
        {
            return self.prefix(index).checked_sub(body - byte);
        }
        let mut position = self.prefix(index) + usize::from(line.bom) * 3;
        let mut valid = true;
        [
            Some(line.body),
            line.bridge
                .as_ref()
                .map(|bridge| &source[bridge.start..bridge.end]),
            Some(line.tail),
        ]
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .find_map(|part| {
            let start = part.as_ptr() as usize - source.as_ptr() as usize;
            let end = start + part.len();
            valid &= byte >= start;
            valid
                .then(|| {
                    position = projection.byte_position(start, byte.min(end), position);
                    (byte <= end).then(|| self.close_gaps(index, position))
                })
                .flatten()
        })
    }

    fn close_gaps(&self, index: usize, mut position: usize) -> usize {
        let raw = position.saturating_add_signed(-self.targets.deltas()[index]);
        let first = self
            .targets
            .gaps()
            .partition_point(|gap| gap.line() < index);
        for gap in self.targets.gaps()[first..]
            .iter()
            .take_while(|gap| gap.line() == index)
        {
            position -= gap.removed().min(raw.saturating_sub(gap.column() + 2));
        }
        position
    }
}

impl DocumentPlan<'_> {
    /// Folds each long scalar; one whose line keeps its source indentation continues under
    /// its source owner rather than its target one.
    fn plan_folds(
        &mut self,
        cfg: crate::rules::line_length::Fold,
        reindents: bool,
    ) -> Result<(), Fallback> {
        let before_indent =
            folded_tail_interacts(self.input, &self.layout, &self.targets)
                .then(|| self.input.parsed().document_starts.len() - 1);
        let projection = projection::Projection::new(self.input, &self.edits);
        self.layout
            .converted_foldables()
            .unwrap_or_else(|| {
                crate::rules::line_length::fold::foldable_scalars(self.input)
            })
            .into_iter()
            .try_for_each(|scalar| {
                let line = self
                    .input
                    .byte_line_starts()
                    .partition_point(|&start| start <= scalar.content.start)
                    - 1;
                let frozen_owner = (!reindents
                    || (!self.targets.refused().is_empty()
                        && self.layout.frozen(self.targets.refused(), line)))
                .then(|| scalar.owner.map_or(0, |(_, column)| column));
                self.plan_scalar_fold(
                    scalar.content,
                    scalar.style,
                    frozen_owner,
                    cfg,
                    &projection,
                    before_indent.is_some_and(|document| {
                        self.input
                            .parsed()
                            .document_starts
                            .partition_point(|&start| start <= line)
                            - 1
                            == document
                    }),
                )
            })
            .map(|()| self.folds.sort_by_key(|site| (site.line, site.byte)))
    }

    fn plan_scalar_fold(
        &mut self,
        range: std::ops::Range<usize>,
        style: quoted_strings::LayoutStyle,
        frozen_owner: Option<usize>,
        cfg: crate::rules::line_length::Fold,
        projection: &projection::Projection,
        before_indent: bool,
    ) -> Result<(), Fallback> {
        let starts = self.input.byte_line_starts();
        let line_of = |byte| {
            starts
                .partition_point(|&start| start <= byte)
                .saturating_sub(1)
        };
        let full = range.start - style.quote_width()..range.end + style.quote_width();
        if full.is_empty() {
            return Ok(());
        }
        let style = self.styles.layout_at(full.start).unwrap_or(style);
        let quotes = style.quote_width();
        let first = self.layout.project_line(line_of(full.start), full.start);
        let last = self
            .layout
            .project_line(line_of(full.end - 1), full.end - 1);
        self.byte_on_line(first, full.start, projection, false)
            .map(|at| at + quotes)
            .ok_or(Fallback::ScalarInteraction)
            .and_then(|content| {
                let owner = frozen_owner.unwrap_or_else(|| {
                    self.targets
                        .scalar_owners()
                        .partition_point(|&(at, _)| at <= full.start)
                        .checked_sub(1)
                        .map_or(0, |index| self.targets.scalar_owners()[index].1)
                });
                let existing = (first + 1..=last)
                    .find(|&line| {
                        !self
                            .layout
                            .geometry(line, &self.targets)
                            .scalar_content()
                            .blank()
                    })
                    .map(|line| self.prefix(line))
                    .filter(|&prefix| prefix > 0);
                let candidate = folding::ScalarCandidate {
                    style,
                    first,
                    last,
                    content,
                    owner,
                    existing,
                };
                let Some(target) = candidate.settle(cfg) else {
                    return Ok(());
                };
                let directive =
                    comments::directive_tail(self.input, BytePos::new(full.end));
                let site = FoldSite {
                    quotes,
                    before_indent,
                    tail: folding::Tail {
                        ends_in_break: quoted_strings::retained_tail(
                            self.input.text(),
                            BytePos::new(full.end),
                        )
                        .ends_in_break(),
                        directive: directive.inline(),
                        guarded_directive: self.guard.is_some() && directive.guarded(),
                    },
                    full,
                };
                (first..=last).try_for_each(|index| {
                    self.plan_fold_line(index, &site, &target, projection)
                })
            })
    }

    fn plan_fold_line(
        &mut self,
        index: usize,
        site: &FoldSite,
        target: &folding::ScalarTarget,
        projection: &projection::Projection,
    ) -> Result<(), Fallback> {
        let line = self.layout.geometry(index, &self.targets);
        let geometry = folding::Geometry {
            index,
            prefix: line.prefix(),
            retained_prefix: if site.before_indent {
                line.retained_prefix()
            } else {
                line.prefix()
            },
            bom: line.bom(),
            content: line.scalar_content().kind(),
            allowed: self.guard.as_ref().is_none_or(|guard| {
                guard.allows(
                    crate::rules::line_length::ID,
                    line.source_anchor(),
                    line.source_anchor(),
                )
            }),
            guarded_directive_tail: site.tail.guarded_directive,
        };
        let Some(geometry) = target.allow_line(geometry) else {
            return Ok(());
        };
        let mut parts = Vec::new();
        let result = self
            .visit_body(index, |part| parts.push(part))
            .and_then(|()| {
                let scalar_end = target
                    .needs_end(index)
                    .then(|| {
                        self.byte_on_line(index, site.full.end, projection, true)
                            .map(|end| end - site.quotes)
                    })
                    .flatten();
                let content = folding::selected_line(&geometry, parts);
                target.plan_line(folding::Line {
                    geometry,
                    content,
                    scalar_end,
                    ends_in_break: site.tail.ends_in_break,
                    directive_tail: site.tail.directive,
                })
            });
        result.map(|breaks| self.folds.extend(breaks))
    }
}

/// Empties the whitespace-only line holding `at`, which a trailing-space edit clears.
fn blank_whitespace_line(
    input: Analysis<'_>,
    layout: &mut indentation::Layout<'_>,
    targets: &indentation::Targets,
    at: crate::rules::support::span_utils::BytePos,
) -> bool {
    let original = input
        .byte_line_starts()
        .partition_point(|&start| start <= at.get())
        - 1;
    let index = layout.project_line(original, at.get());
    let blank = trailing_spaces::clears_line(&layout.geometry(index, targets));
    let line = &mut layout.lines[index];
    if blank {
        line.prefix = 0;
        line.body = &line.body[..0];
    }
    blank
}

fn attach_marker_bom(
    input: Analysis<'_>,
    layout: &mut indentation::Layout<'_>,
    targets: &indentation::Targets,
    at: crate::rules::support::span_utils::BytePos,
) {
    let original = input
        .byte_line_starts()
        .partition_point(|&byte| byte <= at.get())
        - 1;
    let index = layout.project_line(original, at.get());
    let attach = document_start::attaches_bom(
        &layout.geometry(index, targets),
        at.get() - input.byte_line_starts()[original],
    );
    let line = &mut layout.lines[index];
    if attach {
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
    quoted_strings::protection::folded_tail(input).is_some_and(|header| {
        let line = input
            .byte_line_starts()
            .partition_point(|&start| start <= header.get())
            - 1;
        targets.dedents_line(layout.project_line(line, header.get()))
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
    run: &[usize],
    guard: Option<&Guard>,
) {
    if let Some(guard) = guard {
        for &index in run {
            let origin = lines[index].origin;
            if !guard.allows(
                empty_lines::ID,
                lines[index].source_anchor(input.text()),
                lines[index].source_anchor(input.text()),
            ) {
                continue;
            }
            let start = input.byte_line_starts()[origin];
            let end = input
                .byte_line_starts()
                .get(origin + 1)
                .copied()
                .unwrap_or(input.text().len());
            if let Some(previous) =
                removed.last_mut().filter(|range| range.end == start)
            {
                previous.end = end;
            } else {
                removed.push(start..end);
            }
        }
    } else if let Some(&first) = run.first() {
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
}

#[derive(Clone)]
pub(crate) struct Bridge<'a> {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) retained_characters: usize,
    pub(crate) prefix_column: usize,
    pub(crate) end_position: Option<(usize, usize)>,
    pub(crate) joined: bool,
    pub(crate) tight_join: bool,
    pub(crate) parts: Option<Vec<RetainedPart<'a>>>,
}

#[derive(Clone)]
pub(crate) struct VirtualLine<'a> {
    pub(crate) origin: usize,
    pub(crate) prefix: usize,
    pub(crate) body: &'a str,
    pub(crate) tail: &'a str,
    pub(crate) ending: &'a str,
    pub(crate) bom: bool,
    pub(crate) bridge: Option<Box<Bridge<'a>>>,
}

impl<'a> VirtualLine<'a> {
    pub(crate) fn source_anchor(&self, source: &str) -> BytePos {
        BytePos::new(
            self.bridge
                .as_ref()
                .filter(|bridge| bridge.parts.is_some())
                .map_or_else(
                    || self.body.as_ptr() as usize - source.as_ptr() as usize,
                    |bridge| bridge.start,
                ),
        )
    }

    #[expect(
        clippy::inline_always,
        reason = "Keep the streamed layout's per-line constructor inlined."
    )]
    #[inline(always)]
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

    pub(crate) fn source_span(
        &self,
        source: &str,
        columns: std::ops::Range<usize>,
    ) -> Option<(BytePos, BytePos)> {
        let mut column = self.prefix;
        std::iter::once(RetainedPart::Source(self.body))
            .chain(self.parts().into_iter().flatten().copied())
            .map(|part| {
                let start = column;
                column += part.piece().chars().count();
                (part, start, column)
            })
            .find(|&(_, start, end)| start <= columns.start && columns.end <= end)
            .and_then(|(part, column, _)| {
                let RetainedPart::Source(text) = part else {
                    return None;
                };
                let byte = |at| {
                    text.char_indices()
                        .nth(at)
                        .map_or(text.len(), |(byte, _)| byte)
                };
                let start = text.as_ptr() as usize - source.as_ptr() as usize;
                Some((
                    BytePos::new(start + byte(columns.start - column)),
                    BytePos::new(start + byte(columns.end - column)),
                ))
            })
    }

    fn retained_parts(&self) -> Vec<RetainedPart<'a>> {
        self.parts()
            .map_or_else(|| vec![RetainedPart::Source(self.body)], <[_]>::to_vec)
    }

    fn set_parts(&mut self, source: &'a str, parts: Vec<RetainedPart<'a>>) {
        let anchor = self.source_anchor(source).get();
        self.body = &source[anchor..anchor];
        self.tail = "";
        self.bridge = Some(Box::new(Bridge {
            start: anchor,
            end: anchor,
            retained_characters: 0,
            prefix_column: 0,
            end_position: None,
            joined: false,
            tight_join: false,
            parts: Some(parts),
        }));
    }

    pub(crate) fn split_at(&mut self, source: &'a str, column: usize) -> Self {
        let mut before = Vec::new();
        let mut after = Vec::new();
        let mut remaining = column.saturating_sub(self.prefix);
        for part in self.retained_parts() {
            let width = part.piece().chars().count();
            if width <= remaining {
                before.push(part);
                remaining -= width;
            } else {
                let (left, right) = part.split_at(remaining);
                if remaining > 0 {
                    before.push(left);
                }
                after.push(right);
                remaining = 0;
            }
        }
        let mut below = self.clone();
        below.bom = false;
        below.prefix = column;
        below.set_parts(source, after);
        self.set_parts(source, before);
        below
    }

    pub(crate) fn truncate_at(&mut self, source: &'a str, column: usize) {
        self.split_at(source, column);
    }

    pub(crate) fn append_parts(&mut self, source: &'a str, below: &Self) {
        let mut parts = self.retained_parts();
        parts.push(RetainedPart::Spaces(1));
        parts.extend(below.retained_parts());
        self.ending = below.ending;
        self.set_parts(source, parts);
    }

    pub(crate) fn is_empty(&self) -> bool {
        !self.bom
            && self.prefix == 0
            && self.body.is_empty()
            && self.tail.is_empty()
            && self.bridge.is_none()
    }

    fn content_characters(&self) -> impl Iterator<Item = char> + '_ {
        self.body
            .chars()
            .chain(
                self.parts()
                    .into_iter()
                    .flatten()
                    .flat_map(|part| part.piece().chars()),
            )
            .chain(self.tail.chars())
    }

    pub(crate) fn end_column(&self) -> usize {
        self.prefix
            + self
                .content_characters()
                .enumerate()
                .filter_map(|(column, ch)| (!ch.is_whitespace()).then_some(column + 1))
                .last()
                .unwrap_or(0)
    }

    pub(crate) fn spaces_after(&self, column: usize) -> usize {
        std::iter::repeat_n(' ', self.prefix)
            .chain(self.content_characters())
            .skip(column + 1)
            .take_while(|&ch| ch == ' ')
            .count()
    }

    pub(crate) fn parts(&self) -> Option<&[RetainedPart<'a>]> {
        self.bridge
            .as_ref()
            .and_then(|bridge| bridge.parts.as_deref())
    }

    pub(crate) fn content(&self) -> &str {
        match (self.body.is_empty(), self.parts().and_then(<[_]>::first)) {
            (true, Some(RetainedPart::Source(text) | RetainedPart::Text(text))) => text,
            _ => self.body,
        }
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
            && gaps[gap].column() + 2 < column + text.len()
        {
            let site = &gaps[gap];
            let start = (site.column() + 2).saturating_sub(column);
            let end = (site.column() + 2 + site.removed())
                .saturating_sub(column)
                .min(text.len());
            let slices = text.get(from..start).zip(text.get(end..text.len()));
            valid = slices
                .map(|(prefix, _)| {
                    write(prefix);
                    from = end;
                    complete = column + end == site.column() + 2 + site.removed();
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
    let selected = line.parts();
    let visit = |(piece, part): (usize, RetainedPart<'a>)| {
        let RetainedPart::Source(part) = part else {
            return append(part.piece());
        };
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
            .take_while(|(from, _, _)| {
                from.get() < end || (piece != 1 && from.get() == end)
            })
            .try_for_each(|(from, to, text)| {
                if selected.is_none()
                    && piece != 1
                    && line.bridge.as_ref().is_some_and(|bridge| {
                        bridge.start <= from.get()
                            && from.get() < bridge.end
                            && to.get() <= bridge.end
                    })
                {
                    return Ok(());
                }
                if from.get() < start || to.get() > end {
                    return indentation::discards_gap(
                        source,
                        line,
                        from.get(),
                        to.get(),
                    )
                    .then_some(())
                    .ok_or(Fallback::Overlap);
                }
                let result = append(Piece::Text(&source[at..from.get()]))
                    .and_then(|()| append(text.piece()));
                at = to.get();
                result
            })
            .and_then(|()| append(Piece::Text(&source[at..end])))
    };
    match (selected, line.bridge.as_ref()) {
        (Some(parts), _) => parts
            .iter()
            .copied()
            .map(|part| (3, part))
            .try_for_each(visit),
        (None, Some(bridge)) => {
            [line.body, &source[bridge.start..bridge.end], line.tail]
                .into_iter()
                .enumerate()
                .map(|(piece, part)| (piece, RetainedPart::Source(part)))
                .try_for_each(visit)
        }
        (None, None) => [line.body, line.tail]
            .into_iter()
            .map(|part| (0, RetainedPart::Source(part)))
            .try_for_each(visit),
    }
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

impl<'a> LayoutTargets<'a> {
    fn finish_lines(
        mut self,
        passes: &Passes<'_>,
    ) -> Result<LineTargets<'a>, Fallback> {
        let final_newline = crate::rules::new_line_at_end_of_file::requirement(
            self.0.input.text(),
            passes.final_newline.as_ref(),
            Guard::permission(
                self.0.guard.as_deref(),
                crate::rules::new_line_at_end_of_file::ID,
            ),
        )
        .select(
            (0..self.0.layout.lines.len())
                .map(|index| self.0.layout.geometry(index, &self.0.targets)),
        );
        if let Some(final_newline) = final_newline {
            let last = self
                .0
                .layout
                .lines
                .last_mut()
                .expect("nonempty input has a line");
            last.ending = final_newline.materialize();
            let allowed = self.0.guard.as_ref().is_none_or(|guard| {
                guard.allows(
                    new_lines::ID,
                    last.source_anchor(self.0.input.text()),
                    last.source_anchor(self.0.input.text()),
                )
            });
            last.ending = self.0.endings.line(last.ending, allowed);
            self.0.rules.push(crate::rules::new_line_at_end_of_file::ID);
        }
        self.0.trim(passes);
        self.0.markers(passes);
        self.0.boundaries.sort_by_key(|(start, _, _)| start.get());
        if let Some(cfg) = passes.empty_lines {
            self.0.empty_lines(&cfg);
        }
        let edits = &mut self.0.edits;
        let boundaries = &mut self.0.boundaries;
        let rules = &mut self.0.rules;
        edits.sort_by_key(|(start, end, _)| (start.get(), end.get()));
        edits.dedup();
        let disjoint = !edits.windows(2).any(|pair| {
            pair[0].1 > pair[1].0 || (pair[0].0 == pair[1].0 && pair[1].0 == pair[1].1)
        });
        boundaries.sort_by_key(|(start, _, _)| start.get());
        self.0.dirty = !rules.is_empty();
        self.0.finish_endings();
        disjoint
            .then_some(LineTargets(self.0))
            .ok_or(Fallback::Overlap)
    }
}

impl<'a> LineTargets<'a> {
    fn fold(mut self, passes: &Passes<'_>) -> Result<EmitPlan<'a>, Fallback> {
        passes
            .line_length
            .map_or(Ok(()), |cfg| {
                self.0.plan_folds(cfg, passes.indentation.is_some())
            })
            .map(|()| {
                if !self.0.folds.is_empty() {
                    self.0.rules.push(crate::rules::line_length::ID);
                    self.0.dirty = true;
                }
                EmitPlan(self.0)
            })
    }
}
