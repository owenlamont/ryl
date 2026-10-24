use std::collections::HashMap;

use ryl_yaml_parser::{Event, ScalarStyle};

use crate::fix::{PassTracking, Passes};
use crate::rules::support::span_utils::BytePos;
use crate::rules::support::{collection_style, event_compare};
use crate::rules::{
    braces, brackets, colons, commas, comments, comments_indentation, document_end,
    document_start, empty_lines, hyphens, indentation, new_lines, quoted_strings,
    trailing_spaces,
};
use crate::text_version::Analysis;
use crate::text_version::TextVersion;
use crate::text_version::edits::Edit;

pub(crate) mod facts;
mod guard;
use guard::Guard;
mod pieces;
pub(crate) mod projection;
pub(crate) use pieces::PlanEdit;
use pieces::{Piece, plan_edit};
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
    refused: Vec<indentation::Refusal>,
    inconsistent: Vec<indentation::Violation>,
    guard: Option<Box<Guard>>,
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

struct FormatStep {
    output: Option<TextVersion<'static>>,
    frozen: Vec<Option<indentation::Cause>>,
    settled: bool,
}

impl FormatStep {
    fn output(output: TextVersion<'static>, settled: bool) -> Self {
        Self {
            output: Some(output),
            frozen: Vec::new(),
            settled,
        }
    }

    fn replan(frozen: Vec<Option<indentation::Cause>>) -> Self {
        Self {
            output: None,
            frozen,
            settled: false,
        }
    }
}

pub(crate) fn format(
    source: &mut TextVersion<'_>,
    passes: &Passes<'_>,
    tracking: &mut PassTracking<'_>,
) -> Result<TextVersion<'static>, Fallback> {
    let Some(cfg) = passes.collection_style else {
        return with_quote_stage(
            source,
            passes,
            tracking,
            |input, passes, tracking, attempt| {
                plan_document(input, passes, tracking, None, attempt)
            },
        );
    };
    format_collections(source, passes, tracking, cfg)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Attempt {
    Unstaged,
    Staged,
    /// A staged attempt whose unheld plan already failed with a scalar interaction.
    Held,
}

/// Plans `input`; a folded tail that re-indentation would disturb folds before its document
/// re-indents, as main's earlier passes did.
fn plan_document(
    input: &mut TextVersion<'_>,
    passes: &Passes<'_>,
    tracking: &mut PassTracking<'_>,
    first: Option<&Passes<'_>>,
    attempt: Attempt,
) -> Result<TextVersion<'static>, Fallback> {
    let edited = tracking.edited.len();
    let result = if attempt == Attempt::Held {
        Err(Fallback::ScalarInteraction)
    } else {
        format_document_staged(input, passes, tracking, first, None)
    };
    match result {
        Err(Fallback::ScalarInteraction)
            if attempt != Attempt::Unstaged
                && passes.line_length.is_some()
                && passes.indentation.is_some() =>
        {
            tracking.edited.truncate(edited);
            let mut held = vec![None; input.analysis().parsed().document_starts.len()];
            *held
                .last_mut()
                .expect("a folded tail belongs to a document") =
                Some(indentation::Cause::Changed);
            format_document_staged(input, passes, tracking, first, Some(held))
        }
        result => result,
    }
}

/// `plan` over `input`, retried with its quote edits applied first when they cannot share
/// its virtual lines, or on any unambiguous failure while folding.
fn with_quote_stage(
    input: &mut TextVersion<'_>,
    passes: &Passes<'_>,
    tracking: &mut PassTracking<'_>,
    mut plan: impl FnMut(
        &mut TextVersion<'_>,
        &Passes<'_>,
        &mut PassTracking<'_>,
        Attempt,
    ) -> Result<TextVersion<'static>, Fallback>,
) -> Result<TextVersion<'static>, Fallback> {
    let folding = passes.line_length.is_some();
    let edited = tracking.edited.len();
    let reason = match plan(input, passes, tracking, Attempt::Unstaged) {
        Err(reason)
            if matches!(reason, Fallback::ScalarInteraction)
                || (folding && !matches!(reason, Fallback::Ambiguous)) =>
        {
            tracking.edited.truncate(edited);
            reason
        }
        result => return result,
    };
    prepare_quotes(input.analysis(), passes).and_then(|quoted| match quoted {
        Some(mut quoted) => probe(tracking, input.analysis())
            .and_then(|()| plan(&mut quoted, passes, tracking, Attempt::Staged))
            .inspect(|_| tracking.edited.push(quoted_strings::ID)),
        // Without quote edits the unstaged plan was the staged one, so only its held retry is new.
        None => match reason {
            Fallback::ScalarInteraction if folding => {
                plan(input, passes, tracking, Attempt::Held)
            }
            reason => Err(reason),
        },
    })
}

/// Refuses the rewrite pass's input once, before its first plan, when it is ambiguous.
fn probe(tracking: &mut PassTracking<'_>, input: Analysis<'_>) -> Result<(), Fallback> {
    if std::mem::take(&mut tracking.unprobed) && crate::fix::ambiguous(input) {
        Err(Fallback::Ambiguous)
    } else {
        Ok(())
    }
}

fn format_collections(
    source: &mut TextVersion<'_>,
    passes: &Passes<'_>,
    tracking: &mut PassTracking<'_>,
    cfg: collection_style::Config,
) -> Result<TextVersion<'static>, Fallback> {
    let mut current = None;
    let mut prepared = collection_style::prepare(
        source.analysis(),
        cfg,
        passes.indentation.as_ref(),
        passes.comments_indentation,
    );
    probe(tracking, source.analysis())?;
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
        let output = with_quote_stage(
            &mut converted,
            passes,
            tracking,
            |input, passes, tracking, attempt| {
                if !rules.is_empty()
                    && passes.comments_indentation.is_some()
                    && input
                        .analysis()
                        .event_iter()
                        .any(|(event, _)| matches!(event, Event::Comment(..)))
                {
                    let mut staged = passes.clone();
                    staged.comments_indentation = None;
                    plan_document(input, passes, tracking, Some(&staged), attempt)
                } else {
                    plan_document(input, passes, tracking, None, attempt)
                }
            },
        )?;
        tracking.edited.extend(rules);
        if rejected.as_deref() == Some(output.analysis().text()) {
            return Ok(output);
        }
        prepared = collection_style::prepare(
            output.analysis(),
            cfg,
            passes.indentation.as_ref(),
            passes.comments_indentation,
        );
        if matches!(prepared, Ok(None)) {
            result = Ok(output);
            break;
        }
        current = Some(output);
    }
    result
}

fn format_document_staged(
    source: &mut TextVersion<'_>,
    passes: &Passes<'_>,
    tracking: &mut PassTracking<'_>,
    first: Option<&Passes<'_>>,
    held: Option<Vec<Option<indentation::Cause>>>,
) -> Result<TextVersion<'static>, Fallback> {
    let staged = first.is_some() || held.is_some();
    let mut frozen = held;
    let mut current = None;
    let mut result = Err(Fallback::Refusal);
    for epoch in 0..crate::fix::FIX_PIPELINE_MAX_PASSES {
        let input = current.as_mut().unwrap_or(source);
        let active = if epoch == 0 {
            first.unwrap_or(passes)
        } else {
            passes
        };
        let step = format_document_refused(input, active, tracking, frozen.as_deref())?;
        if step.settled && (epoch > 0 || !staged) {
            let output = step.output.expect("a settled layout has output");
            result = Ok(output);
            break;
        }
        if let Some(output) = step.output {
            frozen = frozen.filter(|_| epoch > 0);
            current = Some(output);
        } else {
            frozen = Some(step.frozen);
        }
    }
    result
}

type Candidate = (
    String,
    Vec<(usize, ScalarStyle)>,
    Vec<&'static str>,
    Option<Box<Guard>>,
);
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
    frozen: Option<&[Option<indentation::Cause>]>,
) -> Result<(Option<Candidate>, Kept), Fallback> {
    if blank_stream(input.text(), passes, frozen) {
        stats::count(stats::Counter::Plan);
        let candidate = (String::new(), Vec::new(), vec![empty_lines::ID], None);
        Ok((Some(candidate), Kept::default()))
    } else {
        DocumentPlan::new(input, passes, quotes, Sink::Emit, frozen).and_then(|plan| {
            probe(tracking, plan.input)?;
            let consistent = plan.dirty
                || !passes.indentation.is_some_and(|cfg| {
                    cfg.requires_consistency()
                        && !plan.layout.consistent_except(&cfg, &plan.refused)
                });
            let kept = plan.kept();
            consistent
                .then_some(())
                .ok_or(Fallback::Refusal)
                .and_then(|()| {
                    plan.dirty
                        .then(|| {
                            plan.emit()
                                .map(|text| (text, plan.styles, plan.rules, plan.guard))
                        })
                        .transpose()
                        .map(|prepared| (prepared, kept))
                })
        })
    }
}

fn format_document_refused(
    source: &mut TextVersion<'_>,
    passes: &Passes<'_>,
    tracking: &mut PassTracking<'_>,
    frozen: Option<&[Option<indentation::Cause>]>,
) -> Result<FormatStep, Fallback> {
    let mut quotes = HashMap::new();
    let (prepared, kept) =
        prepare_candidate(source.analysis(), passes, tracking, &mut quotes, frozen)?;
    let Some((text, styles, rules, guard)) = prepared else {
        remember(tracking, source, passes.indentation, kept);
        source.release_tokens();
        source.mark_strict_valid();
        return Ok(FormatStep::output(
            std::mem::replace(source, TextVersion::new(String::new())).into_owned(),
            true,
        ));
    };
    source.release_layout();
    let input = source.analysis();
    let mut output = TextVersion::new(text);
    stats::count(stats::Counter::Emit);
    if let Some(frozen) = verify_layout(
        input,
        output.analysis(),
        &styles,
        passes,
        guard.as_deref(),
        frozen,
    )? {
        return Ok(FormatStep::replan(frozen));
    }
    output.mark_strict_valid();
    source.take_facts();
    DocumentPlan::new(
        output.analysis(),
        passes,
        &mut quotes,
        Sink::Validate,
        frozen,
    )
    .map(|dry| dry.dirty)
    .and_then(|dirty| {
        if dirty {
            stats::count(stats::Counter::Rollback);
            stats::count(stats::Counter::DryDirty);
            (output.analysis().text() != source.analysis().text())
                .then(|| {
                    tracking.edited.extend(rules);
                    FormatStep::output(output, false)
                })
                .ok_or(Fallback::DryEdits)
        } else {
            remember(tracking, &output, passes.indentation, kept);
            output.release_tokens();
            tracking.edited.extend(rules);
            let plan = source.analysis().plan_version(output);
            source
                .apply(plan)
                .expect("layout plan belongs to the input epoch");
            Ok(FormatStep::output(
                std::mem::replace(source, TextVersion::new(String::new())).into_owned(),
                true,
            ))
        }
    })
}

/// Whether `text` is a stream of LF-only blank lines that the resolved passes empty, so its
/// candidate needs no line records; verification and the dry plan still check it, and with
/// no block scalar it cannot be ambiguous, so it skips the probe.
/// `edits` keeping the separation an explicit `?` key needs before what follows it.
fn separate_keys(
    text: &str,
    edits: Vec<crate::rules::support::token_spacing::SpaceEdit>,
) -> Vec<crate::rules::support::token_spacing::SpaceEdit> {
    let bytes = text.as_bytes();
    edits
        .into_iter()
        .filter_map(|(from, to, count)| {
            let at = from.get();
            let key = bytes[..at].ends_with(b"?")
                && (at < 2 || b"[{, \t\r\n".contains(&bytes[at - 2]));
            if key && count == 0 {
                (to.get() - at != 1 || bytes[at] != b' ').then_some((from, to, 1))
            } else {
                Some((from, to, count))
            }
        })
        .collect()
}

/// Whether only blanks and comments separate `from` from a closing `]` or `}`.
fn closes_after(text: &[u8], mut from: usize) -> bool {
    loop {
        match text.get(from) {
            Some(b' ' | b'\t' | b'\n' | b'\r') => from += 1,
            Some(b'#') => {
                from += text[from..]
                    .iter()
                    .position(|&byte| matches!(byte, b'\n' | b'\r'))
                    .unwrap_or(text.len() - from);
            }
            next => return matches!(next, Some(b']' | b'}')),
        }
    }
}

fn blank_stream(
    text: &str,
    passes: &Passes<'_>,
    frozen: Option<&[Option<indentation::Cause>]>,
) -> bool {
    text.starts_with('\n')
        && frozen.is_none()
        && passes.collection_style.is_none()
        && passes.per_line.is_empty()
        && passes
            .empty_lines
            .is_some_and(|cfg| cfg.max_start() == 0 && cfg.max_end() == 0)
        && text.bytes().all(|byte| byte == b'\n')
}

/// `Ok(None)` for a verified layout, or the documents to freeze before replanning one
/// whose re-indentation changed them.
fn verify_layout(
    input: Analysis<'_>,
    output: Analysis<'_>,
    styles: &[(usize, ScalarStyle)],
    passes: &Passes<'_>,
    guard: Option<&Guard>,
    frozen: Option<&[Option<indentation::Cause>]>,
) -> Result<Option<Vec<Option<indentation::Cause>>>, Fallback> {
    let document_start = passes.document_start.is_some();
    match verify_presentation(input, output, styles, document_start, guard) {
        Ok(()) => Ok(None),
        Err(reason) => passes
            .indentation
            .and_then(|_| {
                changed_documents(
                    output,
                    expected_events(input, styles, document_start, guard),
                    frozen,
                )
            })
            .map(Some)
            .ok_or(reason),
    }
}

fn verify_presentation(
    input: Analysis<'_>,
    output: Analysis<'_>,
    styles: &[(usize, ScalarStyle)],
    document_start: bool,
    guard: Option<&Guard>,
) -> Result<(), Fallback> {
    let expected = expected_events(input, styles, document_start, guard);
    if input
        .verify_format_events(output, expected)
        .unwrap_or(false)
        || (output.parsed().valid
            && crate::lint::strict_parse_error(output.text()).is_none()
            && crate::fix::format_events_match(input.text(), output.text()))
    {
        Ok(())
    } else {
        stats::count(stats::Counter::Rollback);
        Err(Fallback::Verification)
    }
}

/// The documents a rejected layout changed, frozen alongside `frozen`, or `None` when
/// freezing more documents cannot help.
fn changed_documents<'a>(
    output: Analysis<'_>,
    expected: impl Iterator<Item = Event<'a>>,
    frozen: Option<&[Option<indentation::Cause>]>,
) -> Option<Vec<Option<indentation::Cause>>> {
    let before = event_compare::documents(expected);
    let after = event_compare::documents(output.event_values());
    let mut rejected = frozen.map_or_else(|| vec![None; before.len()], <[_]>::to_vec);
    let mut changed = false;
    for (index, document) in before.iter().enumerate() {
        if rejected[index].is_none() && after.get(index) != Some(document) {
            rejected[index] = Some(indentation::Cause::Changed);
            changed = true;
        }
    }
    changed.then_some(rejected)
}

fn expected_events<'a>(
    input: Analysis<'a>,
    styles: &'a [(usize, ScalarStyle)],
    document_start: bool,
    guard: Option<&'a Guard>,
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
            let added = document_start
                && guard.is_none_or(|guard| {
                    let document =
                        input.parsed().document_starts.partition_point(|&line| {
                            line < span.start.line().saturating_sub(1)
                        });
                    guard.added_start(document)
                });
            Event::DocumentStart(explicit || added, version)
        }
        other => other,
    })
}

fn prepare_quotes(
    input: Analysis<'_>,
    passes: &Passes<'_>,
) -> Result<Option<TextVersion<'static>>, Fallback> {
    let presentation = Presentation::quotes(input, passes, &mut HashMap::new())
        .and_then(|mut presentation| presentation.settle().map(|()| presentation))?;
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
    verify_presentation(input, output.analysis(), &presentation.styles, false, None)?;
    output.mark_strict_valid();
    Ok(Some(output))
}

fn remember(
    tracking: &mut PassTracking<'_>,
    text: &TextVersion<'_>,
    target: Option<indentation::Config>,
    (kept_dash_lines, refused, inconsistent): Kept,
) {
    if let (Some(slot), Some(target)) = (tracking.reindented.as_deref_mut(), target) {
        *slot = Some(crate::fix::ReindentEvidence::new(
            text.analysis(),
            target,
            refused,
            kept_dash_lines,
            Some(inconsistent),
        ));
    }
}

struct Presentation {
    edits: Vec<PlanEdit>,
    styles: Vec<(usize, ScalarStyle)>,
    rules: Vec<&'static str>,
    guard: Option<Box<Guard>>,
}

impl Presentation {
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
                    if presentation.drops_comma(
                        input,
                        passes,
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
        presentation
            .extend_spaces(commas::ID, separate_keys(input.text(), comma_edits));
        let close = |edits: Vec<crate::rules::support::token_spacing::SpaceEdit>| {
            separate_keys(
                input.text(),
                edits
                    .into_iter()
                    .filter(|(from, to, _)| {
                        closing_edits.binary_search(&(*from, *to)).is_err()
                    })
                    .collect(),
            )
        };
        if let Some(edits) = braces {
            presentation.extend_spaces(braces::ID, close(edits.finish()));
        }
        if let Some(edits) = brackets {
            presentation.extend_spaces(brackets::ID, close(edits.finish()));
        }
        if templates && let Some(cfg) = &passes.brackets {
            presentation.extend(brackets::ID, brackets::edits(input, cfg));
        }
        presentation.settle().map(|()| presentation)
    }

    /// Whether the comma at `at` goes: always before a closing bracket when formatting,
    /// and in a lint fix where the closing rule contradicts the commas spacing; a gap the
    /// closing rule owns otherwise joins `closing_edits`.
    fn drops_comma(
        &self,
        input: Analysis<'_>,
        passes: &Passes<'_>,
        at: usize,
        comma_edits: &mut [crate::rules::support::token_spacing::SpaceEdit],
        closing_edits: &mut Vec<(BytePos, BytePos)>,
    ) -> bool {
        if passes.preserve_events && closes_after(input.text().as_bytes(), at + 1) {
            return true;
        }
        let Some(edit) = comma_edits.last_mut().filter(|edit| edit.0.get() == at + 1)
        else {
            return false;
        };
        let contradicts = self.closing_comma(input, passes, edit);
        if contradicts == Some(false) {
            closing_edits.push((edit.0, edit.1));
        }
        contradicts == Some(true)
    }

    /// Whether the commas spacing contradicts the closing bracket's, or `None` when the
    /// closing rule does not own the gap `edit` respaces to its own target.
    fn closing_comma(
        &self,
        input: Analysis<'_>,
        passes: &Passes<'_>,
        edit: &mut crate::rules::support::token_spacing::SpaceEdit,
    ) -> Option<bool> {
        let (cfg, rule) = match input.text().as_bytes().get(edit.1.get()) {
            Some(b'}') => (
                passes
                    .braces
                    .as_ref()
                    .filter(|cfg| cfg.forbid() == braces::Forbid::None)
                    .map(|cfg| (cfg.min_spaces_inside(), cfg.max_spaces_inside())),
                braces::ID,
            ),
            Some(b']') => (
                passes
                    .brackets
                    .as_ref()
                    .filter(|cfg| cfg.forbid() == brackets::Forbid::None)
                    .map(|cfg| (cfg.min_spaces_inside(), cfg.max_spaces_inside())),
                brackets::ID,
            ),
            _ => return None,
        };
        let (minimum, maximum) = cfg?;
        if self.guard.as_ref().is_some_and(|guard| {
            !guard.allows(rule, edit.0, edit.1)
                || !guard.allows(commas::ID, edit.0, edit.1)
        }) {
            return None;
        }
        edit.2 = crate::rules::support::punctuation::target_spacing(
            edit.2, minimum, maximum,
        );
        Some(passes.commas.is_some_and(|cfg| {
            crate::rules::support::punctuation::target_spacing(
                edit.2,
                cfg.min_spaces_after(),
                cfg.max_spaces_after(),
            ) != edit.2
        }))
    }

    fn extend(&mut self, rule: &'static str, mut edits: Vec<Edit>) {
        if let Some(guard) = &self.guard {
            edits.retain(|(start, end, _)| guard.allows(rule, *start, *end));
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
        self.align_comments(input, passes, layout);
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

    /// `settle` moves an own-line comment below the last content line with that line's
    /// shift; `comments-indentation` then judges the moved column against the target of
    /// the content line it follows, as it would on the output.
    fn align_trailing_comments(
        &self,
        input: Analysis<'_>,
        passes: &Passes<'_>,
        layout: &indentation::Layout<'_>,
        targets: &mut indentation::Targets,
    ) {
        use comments_indentation::LineKind;
        let (Some(cfg), Some(last)) = (passes.comments_indentation, targets.last_token)
        else {
            return;
        };
        let lines = &layout.lines;
        if cfg != comments_indentation::Config::default()
            || lines[last + 1..]
                .iter()
                .all(|content| content.comment_column().is_none())
        {
            return;
        }
        let kind = comments_indentation::line_kinds(input);
        let column = |line: usize| {
            let content = &lines[line];
            (content.prefix + content.body.len() - content.body.trim_start().len())
                .cast_signed()
        };
        let previous = (0..=last)
            .rev()
            .find(|&line| kind(lines[line].origin) == Some(LineKind::Other))
            .map_or(0, |line| column(line) + targets.deltas[line]);
        let mut reference = previous;
        for (line, content) in lines.iter().enumerate().skip(last + 1) {
            if content.comment_column().is_none() {
                continue;
            }
            let moved = column(line) + targets.deltas[line];
            match kind(content.origin) {
                Some(LineKind::Comment) => {
                    let aligned = if moved == 0 { 0 } else { reference };
                    if self.guard.as_ref().is_none_or(|guard| {
                        guard.line(comments_indentation::ID, content.origin)
                    }) {
                        targets.deltas[line] = aligned - column(line);
                    }
                    reference = aligned;
                }
                Some(LineKind::ScalarTrailingComment) => reference = moved,
                _ => reference = previous,
            }
        }
    }

    fn retain_documents(
        &mut self,
        input: Analysis<'_>,
        passes: &Passes<'_>,
        layout: &mut indentation::Layout<'_>,
        targets: &mut indentation::Targets,
        cfg: &indentation::Config,
        frozen: Option<&[Option<indentation::Cause>]>,
    ) -> Result<(Vec<indentation::Refusal>, Vec<indentation::Violation>), Fallback>
    {
        if passes.indentation.is_none() {
            targets.deltas.fill(0);
            targets.gaps.clear();
            targets.refused.clear();
        }
        merge_frozen(&mut targets.refused, frozen);
        if !targets.refused.iter().any(Option::is_some) {
            return Ok((Vec::new(), Vec::new()));
        }
        let mut refused = targets.refused.clone();
        let mut reshaped = std::mem::take(&mut layout.reshaped);
        let mut violations = Vec::new();
        let kept = loop {
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
            let kept = layout.keep_refused(&refused, &self.edits);
            *targets = layout.targets(cfg, &self.edits, passes.line_length.is_some());
            targets.refused.clone_from(&refused);
            if !kept
                || (!cfg.requires_consistency()
                    && refused.iter().all(|cause| {
                        cause.is_none_or(|cause| cause == indentation::Cause::Disabled)
                    }))
            {
                break kept;
            }
            violations = layout.target_violations(cfg, targets, &self.edits);
            if !cfg.requires_consistency()
                || !layout.refuse_violated(&violations, &mut refused)
            {
                break true;
            }
        };
        kept.then_some(())
            .ok_or(Fallback::ScalarInteraction)
            .and_then(|()| {
                self.align_comments(input, passes, layout);
                self.align_trailing_comments(input, passes, layout, targets);
                self.settle()
                    .map(|()| (layout.freeze_targets(targets), violations))
            })
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
    ) -> Result<(indentation::Layout<'a>, indentation::Targets, Self), Fallback> {
        facts::prepare_input(input, passes.line_length.is_some()).and_then(|()| {
            let mut layout = indentation::Layout::new(input, cfg);
            layout.trimmed = passes.trailing_spaces;
            Self::new(input, passes, &mut layout, cache).and_then(|presentation| {
                if !layout.collapse_scalars(&presentation.edits) {
                    return Err(Fallback::ScalarInteraction);
                }
                let mut targets = layout.targets(
                    cfg,
                    &presentation.edits,
                    passes.line_length.is_some(),
                );
                presentation.align_trailing_comments(
                    input,
                    passes,
                    &layout,
                    &mut targets,
                );
                Ok((layout, targets, presentation))
            })
        })
    }

    fn stream<'a>(
        input: Analysis<'a>,
        passes: &Passes<'_>,
        cache: &mut HashMap<String, Option<bool>>,
        cfg: &indentation::Config,
    ) -> Result<(indentation::Layout<'a>, indentation::Targets, Self), Fallback> {
        let mut presentation = Self::pre_scan(input, passes, cache)?;
        let mut layout = indentation::Layout::format(input, cfg);
        layout.trimmed = passes.trailing_spaces;
        if !layout.collapse_scalars(&presentation.edits) {
            return Err(Fallback::ScalarInteraction);
        }
        layout
            .stream_targets(cfg, &presentation.edits, passes)
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
                presentation.comments(input, passes, &mut layout);
                presentation.settle().map(|()| {
                    let mut targets = targets.finish(&layout);
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

    fn quotes(
        input: Analysis<'_>,
        passes: &Passes<'_>,
        quotes: &mut HashMap<String, Option<bool>>,
    ) -> Result<Self, Fallback> {
        let mut edits = Vec::new();
        let mut styles = Vec::new();
        let guard = Guard::new(input, passes);
        let spaced_colon = |at| {
            passes.colons == Some(colons::Config::format())
                && guard.as_ref().is_none_or(|guard| {
                    guard.allows(colons::ID, BytePos::new(at), BytePos::new(at + 1))
                })
        };
        let result = passes.quoted_strings.as_ref().map_or(Ok(()), |cfg| {
            let mut next = quoted_strings::format_replacements(
                input,
                cfg,
                quotes,
                passes.preserve_events,
                &spaced_colon,
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
                guard,
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
        frozen: Option<&[Option<indentation::Cause>]>,
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
            if sink == Sink::Validate {
                stats::count(stats::Counter::DryScan);
            }
            Presentation::stream(input, passes, quotes, &cfg)
        };
        planned.and_then(|(mut layout, mut targets, mut presentation)| {
            presentation
                .retain_documents(
                    input,
                    passes,
                    &mut layout,
                    &mut targets,
                    &cfg,
                    frozen,
                )
                .and_then(|refusals| {
                    Self::assemble(
                        input,
                        passes,
                        layout,
                        targets,
                        presentation,
                        refusals,
                    )
                })
        })
    }

    fn assemble(
        input: Analysis<'a>,
        passes: &Passes<'_>,
        layout: indentation::Layout<'a>,
        targets: indentation::Targets,
        presentation: Presentation,
        (refused, inconsistent): (
            Vec<indentation::Refusal>,
            Vec<indentation::Violation>,
        ),
    ) -> Result<Self, Fallback> {
        let Presentation {
            edits,
            styles,
            mut rules,
            guard,
        } = presentation;
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
            layout.lines.iter().any(|line| {
                !line.ending.is_empty()
                    && line.ending != newline
                    && guard
                        .as_ref()
                        .is_none_or(|guard| guard.line(new_lines::ID, line.origin))
            })
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
            refused,
            inconsistent,
            guard,
        };
        plan.finish(passes).map(|()| plan)
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
            let styles = &self.styles;
            let mut next = trailing_spaces::edits_restyled(input, |start| {
                styles
                    .binary_search_by_key(&start, |(at, _)| *at)
                    .is_ok_and(|index| styles[index].1 != ScalarStyle::DoubleQuoted)
            });
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
                if !blank_whitespace_line(input, layout, edit.0) {
                    edits.push(plan_edit(edit));
                }
            }
        }
    }

    fn markers(&mut self, passes: &Passes<'_>) {
        let final_newline = self.final_newline_enabled(passes)
            || (self.layout.lines.len() > 1
                && self.layout.lines.last().is_some_and(VirtualLine::is_empty));
        let keep_magic = !self.edits_magic_first_line(passes);
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
                    blank_whitespace_line(input, &mut self.layout, edit.0);
                    continue;
                }
                if rule == document_start::ID {
                    attach_marker_bom(input, &mut self.layout, edit.0);
                }
                if rule == document_start::ID
                    && let Some(guard) = &mut self.guard
                {
                    guard.add_start(input, edit.0);
                }
                if rule == document_end::ID
                    && final_newline
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

    fn edits_magic_first_line(&self, passes: &Passes<'_>) -> bool {
        let Some(cfg) = passes
            .comments
            .as_ref()
            .filter(|cfg| !cfg.ignore_shebangs())
        else {
            return false;
        };
        let first = self.input.text().lines().next().unwrap_or_default();
        crate::rules::support::line_syntax::is_magic_first_line(
            first.strip_prefix('\u{feff}').unwrap_or(first),
        ) && comments::edits(self.input, cfg)
            .iter()
            .any(|(start, end, _)| {
                start.get() <= first.len()
                    && self
                        .guard
                        .as_ref()
                        .is_none_or(|guard| guard.allows(comments::ID, *start, *end))
            })
    }

    fn final_newline_enabled(&self, passes: &Passes<'_>) -> bool {
        passes.final_newline.is_some()
            && self.guard.as_ref().is_none_or(|guard| {
                let eof = crate::rules::support::span_utils::BytePos::new(
                    self.input.text().len(),
                );
                guard.allows(crate::rules::new_line_at_end_of_file::ID, eof, eof)
            })
    }

    fn finish(&mut self, passes: &Passes<'_>) -> Result<(), Fallback> {
        let final_newline = self.final_newline_enabled(passes)
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
            last.ending = match passes
                .final_newline
                .as_ref()
                .expect("the final-newline policy is enabled")
                .newline(self.input.text())
            {
                "\r\n" => "\r\n",
                "\r" => "\r",
                _ => "\n",
            };
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
        let disjoint = !edits.windows(2).any(|pair| {
            pair[0].1 > pair[1].0 || (pair[0].0 == pair[1].0 && pair[1].0 == pair[1].1)
        });
        boundaries.sort_by_key(|(start, _, _)| start.get());
        styles.sort_by_key(|(start, _)| *start);
        self.dirty = !rules.is_empty();
        disjoint
            .then_some(())
            .ok_or(Fallback::Overlap)
            .and_then(|()| {
                passes
                    .line_length
                    .map_or(Ok(()), |cfg| self.plan_folds(cfg))
            })
            .map(|()| {
                if !self.folds.is_empty() {
                    self.rules.push(crate::rules::line_length::ID);
                    self.dirty = true;
                }
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
                    self.guard.as_deref(),
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
            // A marker on a BOM-only line leaves the rest of that line blank.
            let blank = line.is_empty()
                || (marker && line.bom && line.prefix == 0 && line.body.is_empty());
            if blank && !protected[line.origin] {
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
                self.guard.as_deref(),
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
            self.guard.as_deref(),
        );
        if !self.removed.is_empty() {
            self.rules.push(empty_lines::ID);
        }
    }

    fn removed(&self, line_start: usize) -> bool {
        self.removed
            .get(
                self.removed
                    .partition_point(|range| range.end <= line_start),
            )
            .is_some_and(|range| range.start <= line_start)
    }

    fn kept(&self) -> Kept {
        let refused = &self.targets.refused;
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
                .kept_dash_targets(&self.targets, &self.edits)
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

    fn emit(&self) -> Result<String, Fallback> {
        if self.guard.is_some() {
            self.emit_lines::<true>()
        } else {
            self.emit_lines::<false>()
        }
    }

    fn emit_lines<const GUARDED: bool>(&self) -> Result<String, Fallback> {
        let mut output = String::with_capacity(self.input.text().len());
        let source = self.input.text();
        let fold_newline = self.newline.unwrap_or_else(|| {
            crate::rules::support::line_syntax::buffer_newline(source)
        });
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
                        let (at, _, text) = &self.boundaries[boundary];
                        output.push_str(&self.ending(text, *at));
                        boundary += 1;
                    }
                    if self.removed(line_start) {
                        return Ok(());
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
                        output.push_str(
                            self.newline
                                .filter(|_| !line.ending.is_empty())
                                .filter(|_| {
                                    !GUARDED
                                        || self
                                            .guard
                                            .as_ref()
                                            .expect("guarded emission has a guard")
                                            .line(new_lines::ID, line.origin)
                                })
                                .unwrap_or(line.ending),
                        );
                    })
                });
        emitted.map(|()| {
            for (at, _, text) in &self.boundaries[boundary..] {
                output.push_str(&self.ending(text, *at));
            }
            output
        })
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

    fn ending<'b>(
        &self,
        text: &'b str,
        at: crate::rules::support::span_utils::BytePos,
    ) -> std::borrow::Cow<'b, str> {
        if self
            .guard
            .as_ref()
            .is_some_and(|guard| !guard.allows(new_lines::ID, at, at))
        {
            return text.into();
        }
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
        let body = line.body.as_ptr() as usize - source.as_ptr() as usize;
        if byte <= body
            && body - byte <= line.prefix
            && (byte < body || line.body.is_empty())
        {
            return self.prefix(index).checked_sub(body - byte);
        }
        let mut position = self.prefix(index) + usize::from(line.bom) * 3;
        let mut valid = true;
        let bridged = line.bridge.as_ref().map_or(0, |bridge| {
            self.edits[self
                .edits
                .partition_point(|(from, _, _)| from.get() < bridge.start)]
            .2
            .len()
        });
        [(line.body, 0), (line.tail, bridged)]
            .into_iter()
            .filter(|(part, bridged)| !part.is_empty() || *bridged > 0)
            .find_map(|(part, bridged)| {
                let start = part.as_ptr() as usize - source.as_ptr() as usize;
                let end = start + part.len();
                valid &= byte >= start;
                valid
                    .then(|| {
                        position = projection.byte_position(
                            self.input,
                            start,
                            byte.min(end),
                            position + bridged,
                        );
                        (byte <= end).then(|| self.close_gaps(index, position))
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
        crate::rules::line_length::fold::foldable_scalars(self.input)
            .into_iter()
            .try_for_each(|scalar| {
                let line = self
                    .input
                    .byte_line_starts()
                    .partition_point(|&start| start <= scalar.content.start)
                    - 1;
                let frozen_owner = (!self.targets.refused.is_empty()
                    && self.layout.frozen(&self.targets.refused, line))
                .then(|| scalar.owner.map_or(0, |(_, column)| column));
                self.plan_scalar_fold(
                    scalar.content,
                    scalar.style,
                    frozen_owner,
                    cfg,
                    &projection,
                )
            })
            .map(|()| self.folds.sort_by_key(|site| (site.line, site.byte)))
    }

    fn plan_scalar_fold(
        &mut self,
        range: std::ops::Range<usize>,
        style: ScalarStyle,
        frozen_owner: Option<usize>,
        cfg: crate::rules::line_length::Fold,
        projection: &projection::Projection,
    ) -> Result<(), Fallback> {
        let starts = self.input.byte_line_starts();
        let line_of = |byte| {
            starts
                .partition_point(|&start| start <= byte)
                .saturating_sub(1)
        };
        let quote_width = |style| {
            usize::from(matches!(
                style,
                ScalarStyle::SingleQuoted | ScalarStyle::DoubleQuoted
            ))
        };
        let full = range.start - quote_width(style)..range.end + quote_width(style);
        if full.is_empty() {
            return Ok(());
        }
        let style = self
            .styles
            .binary_search_by_key(&full.start, |(at, _)| *at)
            .map_or(style, |index| self.styles[index].1);
        let quotes = quote_width(style);
        let first = self.layout.project_line(line_of(full.start), full.start);
        let last = self
            .layout
            .project_line(line_of(full.end - 1), full.end - 1);
        self.byte_on_line(first, full.start, projection)
            .map(|at| at + quotes)
            .ok_or(Fallback::ScalarInteraction)
            .and_then(|content| {
                let owner = frozen_owner.unwrap_or_else(|| {
                    self.targets
                        .scalar_owners
                        .partition_point(|&(at, _)| at <= full.start)
                        .checked_sub(1)
                        .map_or(0, |index| self.targets.scalar_owners[index].1)
                });
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
                (first..=last)
                    .filter(|_| !folded || content > owner)
                    .try_for_each(|index| {
                        self.plan_fold_line(index, &scalar, projection)
                    })
            })
    }

    fn has_directive_tail(&self, full: &std::ops::Range<usize>) -> bool {
        let origin = self
            .input
            .byte_line_starts()
            .partition_point(|&at| at < full.end)
            .saturating_sub(1);
        self.input
            .line_content(origin)
            .get(full.end - self.input.byte_line_starts()[origin]..)
            .and_then(|text| text.split_once('#'))
            .is_some_and(|(_, comment)| {
                crate::directives::directive_scope(comment).is_some()
            })
    }

    fn plan_fold_line(
        &mut self,
        index: usize,
        scalar: &ScalarFold,
        projection: &projection::Projection,
    ) -> Result<(), Fallback> {
        use crate::rules::line_length::fold::breaks_from_chars;
        if self.guard.as_ref().is_some_and(|guard| {
            !guard.line(
                crate::rules::line_length::ID,
                self.layout.lines[index].origin,
            )
        }) {
            return Ok(());
        }
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
        if self.guard.is_some() && index == last && self.has_directive_tail(full) {
            return Ok(());
        }
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
                let start = if folded {
                    indent
                } else if index == first {
                    content
                } else {
                    0
                };
                let end = if folded || index != last {
                    Some(length)
                } else {
                    self.byte_on_line(index, full.end, projection)
                        .map(|end| end - quotes)
                };
                end.ok_or(Fallback::ScalarInteraction).map(|end| {
                    // Bytes bound chars, and `breaks_from_chars` only breaks past `width` chars.
                    if length <= usize::from(width) {
                        return Vec::new();
                    }
                    let scalar = start..end;
                    if index == last
                        && !source[..full.end].ends_with(['\n', '\r'])
                        && source[full.end..]
                            .split(['\r', '\n'])
                            .next()
                            .and_then(|rest| rest.split_once('#'))
                            .is_some_and(|(_, comment)| {
                                crate::directives::directive_scope(comment).is_some()
                            })
                    {
                        return Vec::new();
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

                    breaks_from_chars(chars, &scalar, style, indent, usize::from(width))
                })
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

fn merge_frozen(
    refused: &mut [Option<indentation::Cause>],
    frozen: Option<&[Option<indentation::Cause>]>,
) {
    for (refused, previous) in refused.iter_mut().zip(frozen.unwrap_or_default()) {
        *refused = refused.or(*previous);
    }
}

/// Empties the whitespace-only line holding `at`, which a trailing-space edit clears.
fn blank_whitespace_line(
    input: Analysis<'_>,
    layout: &mut indentation::Layout<'_>,
    at: crate::rules::support::span_utils::BytePos,
) -> bool {
    let original = input
        .byte_line_starts()
        .partition_point(|&start| start <= at.get())
        - 1;
    let index = layout.project_line(original, at.get());
    let line = &mut layout.lines[index];
    let blank = line.tail.is_empty()
        && line.body.bytes().all(|byte| matches!(byte, b' ' | b'\t'));
    if blank {
        line.prefix = 0;
        line.body = &line.body[..0];
    }
    blank
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
        && input.headers().last().is_some_and(|header| {
            let line = header.line - 1;
            input.line_content(line).chars().nth(header.column - 1) == Some('>')
                && targets.deltas
                    [layout.project_line(line, input.byte_line_starts()[line])]
                    < 0
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
    guard: Option<&Guard>,
) {
    let keep = usize::try_from(maximum).unwrap_or(0);
    if let Some(guard) = guard {
        for &index in run.iter().skip(keep) {
            let origin = lines[index].origin;
            if !guard.line(empty_lines::ID, origin) {
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
    } else if let Some(&first) = run.get(keep) {
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

fn discarded_dash_spacing(
    source: &str,
    line: &VirtualLine<'_>,
    from: usize,
    to: usize,
) -> bool {
    line.bridge.is_none()
        && line.body.ends_with('-')
        && from
            == line.body.as_ptr() as usize - source.as_ptr() as usize + line.body.len()
        && (line.tail.is_empty()
            || to <= line.tail.as_ptr() as usize - source.as_ptr() as usize)
        && source[from..to]
            .bytes()
            .all(|byte| byte.is_ascii_whitespace())
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
                            return discarded_dash_spacing(
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
