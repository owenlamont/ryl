use std::borrow::Cow;
use std::cell::OnceCell;
use std::collections::HashMap;
use std::ops::Deref;
use std::rc::Rc;

use crate::rules::support::{line_syntax, punctuation, token_spacing};
use granit_parser::{Event, Parser, ScalarStyle, ScanError, Span};
pub(crate) mod edits;
mod events;
pub(crate) mod lexical;
mod spans;
use events::StoredEvent;

use crate::rules::block_scalar_chomping::{self, Header};

pub struct TextVersion<'text> {
    text: Cow<'text, str>,
    strict_valid: bool,
    event_capacity: usize,
    epoch: Rc<()>,
    parsed: OnceCell<ParsedFacts>,
    headers: OnceCell<Vec<Header>>,
    line_table: OnceCell<LineTable>,
    tokens: OnceCell<lexical::Tokens>,
    sites: OnceCell<Vec<token_spacing::Site>>,
    flow_sites: OnceCell<Vec<(usize, u8)>>,
    indentation: OnceCell<crate::rules::indentation::Scanned>,
    directives: OnceCell<crate::directives::Directives>,
    layout_facts: OnceCell<crate::format::layout::facts::PlanFacts>,
    scalar_lines: OnceCell<Vec<(usize, usize)>>,
    content_blanks: OnceCell<std::collections::HashSet<usize>>,
}

struct LineTable {
    starts: Vec<usize>,
    endings: Vec<u8>,
}

impl LineTable {
    fn new(text: &str) -> Self {
        let mut starts = vec![0];
        let mut endings = Vec::new();
        for (_, end, next) in line_syntax::scan_lines(text) {
            starts.push(next);
            endings.push(
                u8::try_from(next - end)
                    .expect("a YAML line ending is at most two bytes"),
            );
        }
        if endings.last() == Some(&0) {
            starts.pop();
        }
        Self { starts, endings }
    }
}

#[derive(Clone, Copy)]
pub struct Analysis<'text> {
    source: &'text TextVersion<'text>,
}

pub struct EditPlan {
    epoch: Rc<()>,
    output: TextVersion<'static>,
}

impl<'text> TextVersion<'text> {
    pub fn new(text: impl Into<Cow<'text, str>>) -> Self {
        Self {
            text: text.into(),
            strict_valid: false,
            event_capacity: 0,
            epoch: Rc::new(()),
            parsed: OnceCell::new(),
            headers: OnceCell::new(),
            line_table: OnceCell::new(),
            tokens: OnceCell::new(),
            sites: OnceCell::new(),
            flow_sites: OnceCell::new(),
            indentation: OnceCell::new(),
            directives: OnceCell::new(),
            layout_facts: OnceCell::new(),
            scalar_lines: OnceCell::new(),
            content_blanks: OnceCell::new(),
        }
    }

    pub fn analysis(&self) -> Analysis<'_> {
        Analysis { source: self }
    }

    /// Returns `None` for a plan from another epoch, otherwise whether bytes changed.
    pub fn apply(&mut self, plan: EditPlan) -> Option<bool> {
        if !Rc::ptr_eq(&self.epoch, &plan.epoch) {
            return None;
        }
        if self.text == plan.output.text {
            return Some(false);
        }
        *self = plan.output;
        Some(true)
    }

    pub(crate) fn into_owned(self) -> TextVersion<'static> {
        TextVersion {
            text: Cow::Owned(self.text.into_owned()),
            strict_valid: self.strict_valid,
            event_capacity: self.event_capacity,
            epoch: self.epoch,
            parsed: self.parsed,
            headers: self.headers,
            line_table: self.line_table,
            tokens: self.tokens,
            sites: self.sites,
            flow_sites: self.flow_sites,
            indentation: self.indentation,
            directives: self.directives,
            layout_facts: self.layout_facts,
            scalar_lines: self.scalar_lines,
            content_blanks: self.content_blanks,
        }
    }

    pub fn into_text(self) -> String {
        self.text.into_owned()
    }

    pub(crate) fn mark_strict_valid(&mut self) {
        self.strict_valid = true;
    }

    pub(crate) fn into_facts(self) -> Option<ParsedFacts> {
        self.parsed.into_inner()
    }

    pub(crate) fn take_facts(&mut self) -> Option<ParsedFacts> {
        self.parsed.take()
    }

    pub(crate) fn release_tokens(&mut self) {
        self.tokens.take();
    }

    pub(crate) fn release_layout(&mut self) {
        self.headers.take();
        self.line_table.take();
        self.tokens.take();
        self.sites.take();
        self.flow_sites.take();
        self.indentation.take();
        self.directives.take();
        self.layout_facts.take();
        self.scalar_lines.take();
        self.content_blanks.take();
    }

    pub(crate) fn inherit_event_capacity(&mut self, previous: Analysis<'_>) {
        self.event_capacity = previous.parsed().events.len();
    }
}

impl Deref for TextVersion<'_> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.text
    }
}

impl<'text> Analysis<'text> {
    pub(crate) fn epoch(self) -> Rc<()> {
        Rc::clone(&self.source.epoch)
    }

    pub(crate) fn strict_valid(self) -> bool {
        self.source.strict_valid
    }

    #[must_use]
    pub fn text(self) -> &'text str {
        &self.source.text
    }

    #[must_use]
    pub fn plan(self, output: String) -> EditPlan {
        self.plan_version(TextVersion::new(output))
    }

    /// Returns no plan for empty, overlapping or non-UTF-8-aligned byte edits.
    #[must_use]
    pub fn plan_byte_edits(
        self,
        mut edits: Vec<(std::ops::Range<usize>, String)>,
    ) -> Option<EditPlan> {
        edits.sort_by_key(|(range, _)| (range.start, range.end));
        let buffer = self.text();
        let mut previous = None;
        for (range, _) in &edits {
            if range.start > range.end
                || !buffer.is_char_boundary(range.start)
                || !buffer.is_char_boundary(range.end)
                || previous.is_some_and(|(left, right)| {
                    range.start < right || range.start == left
                })
            {
                return None;
            }
            previous = Some((range.start, range.end));
        }
        if edits.is_empty() {
            return None;
        }
        let mut output = String::with_capacity(buffer.len());
        let mut cursor = 0;
        for (range, text) in edits {
            output.push_str(&buffer[cursor..range.start]);
            output.push_str(&text);
            cursor = range.end;
        }
        output.push_str(&buffer[cursor..]);
        Some(self.plan(output))
    }

    pub(crate) fn plan_version(self, mut output: TextVersion<'static>) -> EditPlan {
        if let Some(parsed) = self.source.parsed.get() {
            output.event_capacity = parsed.events.len();
        }
        EditPlan {
            epoch: Rc::clone(&self.source.epoch),
            output,
        }
    }

    pub(crate) fn parsed(self) -> &'text ParsedFacts {
        self.source
            .parsed
            .get_or_init(|| ParsedFacts::parse(self.text(), self.source.event_capacity))
    }

    pub(crate) fn headers(self) -> &'text [Header] {
        if let Some(facts) = self.layout_facts() {
            return &facts.headers;
        }
        self.source
            .headers
            .get_or_init(|| block_scalar_chomping::headers_with_analysis(self))
    }

    pub(crate) fn event_iter(self) -> impl Iterator<Item = (Event<'text>, Span)> {
        self.parsed()
            .events
            .iter()
            .map(move |(event, span)| (event.borrow(self.text()), span.get()))
    }

    pub(crate) fn verify_strict(self, output: Analysis<'_>) -> Result<bool, ScanError> {
        self.verify_strict_events(output, self.event_iter().map(|(event, _)| event))
    }

    pub(crate) fn verify_strict_events(
        self,
        output: Analysis<'_>,
        expected: impl Iterator<Item = Event<'text>>,
    ) -> Result<bool, ScanError> {
        self.verify_events(output, expected, false)
    }

    pub(crate) fn verify_format_events(
        self,
        output: Analysis<'_>,
        expected: impl Iterator<Item = Event<'text>>,
    ) -> Result<bool, ScanError> {
        self.verify_events(output, expected, true)
    }

    fn verify_events(
        self,
        output: Analysis<'_>,
        expected: impl Iterator<Item = Event<'text>>,
        semantic: bool,
    ) -> Result<bool, ScanError> {
        if semantic && prefetch_enabled(output.text()) {
            let text = output.text();
            let mut tokens = None;
            let result = rayon::in_place_scope(|scope| {
                scope.spawn(|_| tokens = Some(lexical::scan(text)));
                self.verify_events_serial(output, expected, semantic)
            });
            crate::format::layout::stats::prefetched();
            if result.as_ref().is_ok_and(|matches| *matches) {
                output
                    .seed_tokens(tokens.expect("the scanner joined before scope exit"));
            }
            return result;
        }
        self.verify_events_serial(output, expected, semantic)
    }

    fn verify_events_serial(
        self,
        output: Analysis<'_>,
        expected: impl Iterator<Item = Event<'text>>,
        semantic: bool,
    ) -> Result<bool, ScanError> {
        crate::format::layout::stats::count(
            crate::format::layout::stats::Counter::Analysis,
        );
        crate::format::layout::stats::count(
            crate::format::layout::stats::Counter::Verify,
        );
        crate::format::layout::stats::count(
            crate::format::layout::stats::Counter::Strict,
        );
        let mut facts = ParsedFacts {
            events: Vec::with_capacity(self.parsed().events.len()),
            ..ParsedFacts::default()
        };
        let mut depth = 0;
        let mut original = self.event_iter().map(|(event, _)| event).filter(|event| {
            !matches!(
                event,
                Event::Comment(..) | Event::StreamStart | Event::StreamEnd
            )
        });
        let mut comparison = crate::fix::EventComparison::default();
        let mut semantic_matches =
            !semantic || !crate::fix::non_ascii_marker(output.text());
        let matches = events::verify_strict(expected, output.text(), |event, span| {
            if semantic
                && !matches!(
                    event,
                    Event::Comment(..) | Event::StreamStart | Event::StreamEnd
                )
            {
                semantic_matches &= original
                    .next()
                    .is_some_and(|before| comparison.matches(&before, &event));
            }
            facts.push(output.text(), event, span, &mut depth);
        })?;
        facts.valid = true;
        facts.compact();
        output.seed_facts(facts);
        Ok(matches && semantic_matches && (!semantic || original.next().is_none()))
    }

    pub(crate) fn significant_events_match(self, other: Analysis<'_>) -> bool {
        struct WithSource<'a> {
            event: &'a StoredEvent,
            source: &'a str,
        }

        impl PartialEq for WithSource<'_> {
            fn eq(&self, other: &Self) -> bool {
                self.event
                    .equivalent(other.event, self.source, other.source)
            }
        }

        impl Eq for WithSource<'_> {}

        let original = self
            .parsed()
            .events
            .iter()
            .map(|(event, _)| event)
            .filter(|event| event.significant())
            .map(|event| WithSource {
                event,
                source: self.text(),
            });
        let rewritten = other
            .parsed()
            .events
            .iter()
            .map(|(event, _)| event)
            .filter(|event| event.significant())
            .map(|event| WithSource {
                event,
                source: other.text(),
            });
        original.eq(rewritten)
    }

    pub(crate) fn byte_line_starts(self) -> &'text [usize] {
        &self.line_table().starts
    }

    fn line_table(self) -> &'text LineTable {
        self.source.line_table.get_or_init(|| {
            crate::format::layout::stats::count(
                crate::format::layout::stats::Counter::Lines,
            );
            LineTable::new(self.text())
        })
    }

    pub(crate) fn lines(self) -> impl Iterator<Item = (usize, &'text str, &'text str)> {
        self.lines_from(0)
    }

    pub(crate) fn lines_from(
        self,
        first: usize,
    ) -> impl Iterator<Item = (usize, &'text str, &'text str)> {
        (first..self.line_count()).map(move |line| {
            let (start, end, next) = self.line_range(line);
            (line, &self.text()[start..end], &self.text()[end..next])
        })
    }

    pub(crate) fn line_count(self) -> usize {
        self.line_table().endings.len()
    }

    fn line_range(self, line: usize) -> (usize, usize, usize) {
        let table = self.line_table();
        let starts = &table.starts;
        let start = starts[line];
        let next = starts.get(line + 1).copied().unwrap_or(self.text().len());
        let end = next - usize::from(table.endings.get(line).copied().unwrap_or(0));
        (start, end, next)
    }

    pub(crate) fn line_content(self, line: usize) -> &'text str {
        let (start, end, _) = self.line_range(line);
        &self.text()[start..end]
    }

    pub(crate) fn get_line(self, line: usize) -> Option<&'text str> {
        (line < self.line_count()).then(|| self.line_content(line))
    }

    pub(crate) fn line_contents_from(
        self,
        first: usize,
    ) -> impl DoubleEndedIterator<Item = &'text str> + ExactSizeIterator + Clone {
        self.line_contents_range(first..self.line_count())
    }

    pub(crate) fn line_contents_range(
        self,
        range: std::ops::Range<usize>,
    ) -> impl DoubleEndedIterator<Item = &'text str> + ExactSizeIterator + Clone {
        // Skipping mapped lines still decodes them, so bound the range before mapping.
        range.map(move |line| self.line_content(line))
    }

    pub(crate) fn line_contents(self) -> Vec<&'text str> {
        self.lines().map(|(_, line, _)| line).collect()
    }

    pub(crate) fn line_is_empty(self, line: usize) -> bool {
        let (start, end, _) = self.line_range(line);
        start == end
    }

    pub(crate) fn prefetched_tokens(self) -> Option<&'text lexical::Tokens> {
        self.source.tokens.get()
    }

    pub(crate) fn seed_tokens(self, tokens: lexical::Tokens) {
        self.source.tokens.get_or_init(|| tokens);
    }

    pub(crate) fn tokens(self) -> &'text lexical::Tokens {
        self.source.tokens.get_or_init(|| {
            crate::format::layout::stats::count(
                crate::format::layout::stats::Counter::Tape,
            );
            crate::format::layout::stats::count(
                crate::format::layout::stats::Counter::Scan,
            );
            lexical::scan(self.text())
        })
    }

    pub(crate) fn directives(self) -> &'text crate::directives::Directives {
        self.source
            .directives
            .get_or_init(|| crate::directives::Directives::parse_uncached(self))
    }

    pub(crate) fn indentation(self) -> &'text crate::rules::indentation::Scanned {
        self.source
            .indentation
            .get_or_init(|| crate::rules::indentation::scan_with_analysis(self))
    }

    pub(crate) fn sites(self) -> &'text [token_spacing::Site] {
        self.source
            .sites
            .get_or_init(|| token_spacing::sites_with_analysis(self))
    }

    pub(crate) fn format_sites(self) -> &'text [token_spacing::Site] {
        self.layout_facts()
            .map_or_else(|| self.sites(), |facts| &facts.sites)
    }

    pub(crate) fn flow_sites(self) -> &'text [(usize, u8)] {
        self.source
            .flow_sites
            .get_or_init(|| punctuation::flow_sites(self, true))
    }

    pub(crate) fn seed_facts(self, facts: ParsedFacts) {
        self.source.parsed.get_or_init(|| facts);
    }

    pub(crate) fn layout_facts(
        self,
    ) -> Option<&'text crate::format::layout::facts::PlanFacts> {
        self.source.layout_facts.get()
    }

    pub(crate) fn seed_layout_facts(
        self,
        facts: crate::format::layout::facts::PlanFacts,
    ) {
        self.source.layout_facts.get_or_init(|| facts);
    }

    pub(crate) fn scalar_lines(self) -> &'text [(usize, usize)] {
        self.source
            .scalar_lines
            .get_or_init(|| line_syntax::scalar_line_ranges(self))
    }

    pub(crate) fn content_blank_lines(self) -> &'text std::collections::HashSet<usize> {
        self.source.content_blanks.get_or_init(|| {
            crate::rules::trailing_spaces::content_blank_lines_with_analysis(self)
        })
    }
}

#[derive(Default)]
pub(crate) struct ParsedFacts {
    scalars: Vec<(ScalarStyle, usize)>,
    pub(crate) root_scalars: HashMap<usize, usize>,
    pub(crate) document_starts: Vec<usize>,
    pub(crate) valid: bool,
    pub(crate) error: Option<ScanError>,
    events: Vec<(StoredEvent, spans::StoredSpan)>,
}

impl ParsedFacts {
    pub(crate) fn scalars(&self) -> impl Iterator<Item = (ScalarStyle, Span)> {
        self.scalars
            .iter()
            .map(|&(style, index)| (style, self.events[index].1.get()))
    }

    pub(crate) fn event_count(&self) -> usize {
        self.events.len()
    }

    fn compact(&mut self) {
        self.events.shrink_to_fit();
        self.scalars.shrink_to_fit();
    }

    fn record(&mut self, event: &Event<'_>, span: Span, depth: &mut usize) {
        match event {
            Event::DocumentStart(..) => {
                self.document_starts.push(span.start.line() - 1);
            }
            Event::MappingStart(..) | Event::SequenceStart(..) => *depth += 1,
            Event::MappingEnd | Event::SequenceEnd => *depth -= 1,
            Event::Scalar(_, style, ..) => {
                self.scalars.push((*style, self.events.len()));
                if *depth == 0
                    && matches!(style, ScalarStyle::Literal | ScalarStyle::Folded)
                {
                    self.root_scalars
                        .insert(span.start.index(), span.end.index());
                }
            }
            _ => {}
        }
    }

    fn parse(text: &str, event_capacity: usize) -> Self {
        Self::parse_with_options(text, event_capacity, false)
    }

    fn push(&mut self, text: &str, event: Event<'_>, span: Span, depth: &mut usize) {
        self.record(&event, span, depth);
        self.events
            .push((StoredEvent::new(text, event), spans::StoredSpan::new(span)));
    }

    pub(crate) fn parse_with_options(
        text: &str,
        event_capacity: usize,
        strict: bool,
    ) -> Self {
        crate::format::layout::stats::count(
            crate::format::layout::stats::Counter::Analysis,
        );
        if strict {
            crate::format::layout::stats::count(
                crate::format::layout::stats::Counter::Strict,
            );
        }
        Self::parse_unrecorded(text, event_capacity, strict)
    }

    pub(crate) fn parse_for_format(text: &str) -> (Self, Option<lexical::Tokens>) {
        if !prefetch_enabled(text) {
            return (Self::parse_unrecorded(text, 0, true), None);
        }
        let (facts, tokens) = rayon::join(
            || Self::parse_unrecorded(text, 0, true),
            || lexical::scan(text),
        );
        (facts, Some(tokens))
    }

    pub(crate) fn parse_unrecorded(
        text: &str,
        event_capacity: usize,
        strict: bool,
    ) -> Self {
        let mut facts = Self {
            events: Vec::with_capacity(event_capacity),
            ..Self::default()
        };
        let mut depth = 0;
        let parser = Parser::new_from_str_with_options(
            text,
            granit_parser::options! { strict_indentation: strict },
        );
        for item in parser {
            let (event, span) = match item {
                Ok(pair) => pair,
                Err(error) => {
                    facts.error = Some(error);
                    break;
                }
            };
            facts.push(text, event, span, &mut depth);
        }
        facts.valid = facts.error.is_none();
        facts.compact();
        facts
    }
}

impl From<String> for TextVersion<'static> {
    fn from(text: String) -> Self {
        Self::new(text)
    }
}

fn prefetch_enabled(text: &str) -> bool {
    text.len() >= 1_048_576
        && std::thread::available_parallelism().is_ok_and(|cpus| cpus.get() > 1)
        && rayon::current_num_threads() > 1
}
