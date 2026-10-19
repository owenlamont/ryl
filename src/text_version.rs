use std::borrow::Cow;
use std::cell::OnceCell;
use std::collections::HashMap;
use std::ops::Deref;
use std::rc::Rc;

use crate::rules::support::span_utils::CharPos;
use crate::rules::support::{line_syntax, punctuation, token_spacing};
use granit_parser::{Event, Parser, ScalarStyle, ScanError, Span};
mod events;
pub(crate) mod lexical;
use events::StoredEvent;

use crate::rules::block_scalar_chomping::{self, Header};

pub struct TextVersion<'text> {
    text: Cow<'text, str>,
    strict_valid: bool,
    event_capacity: usize,
    epoch: Rc<()>,
    parsed: OnceCell<ParsedFacts>,
    headers: OnceCell<Vec<Header>>,
    chars: OnceCell<Vec<(usize, char)>>,
    line_starts: OnceCell<Vec<CharPos>>,
    lines: OnceCell<Vec<(usize, usize, usize)>>,
    tokens: OnceCell<Box<[(lexical::TokenSpan, lexical::Kind)]>>,
    sites: OnceCell<Vec<token_spacing::Site>>,
    indentation: OnceCell<crate::rules::indentation::Scanned>,
    directives: OnceCell<crate::directives::Directives>,
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
            chars: OnceCell::new(),
            line_starts: OnceCell::new(),
            lines: OnceCell::new(),
            tokens: OnceCell::new(),
            sites: OnceCell::new(),
            indentation: OnceCell::new(),
            directives: OnceCell::new(),
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
            chars: self.chars,
            line_starts: self.line_starts,
            lines: self.lines,
            tokens: self.tokens,
            sites: self.sites,
            indentation: self.indentation,
            directives: self.directives,
        }
    }

    pub fn into_text(self) -> String {
        self.text.into_owned()
    }

    pub(crate) fn mark_strict_valid(&mut self) {
        self.strict_valid = true;
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
        self.source
            .headers
            .get_or_init(|| block_scalar_chomping::headers_with_analysis(self))
    }

    pub(crate) fn event_iter(self) -> impl Iterator<Item = (Event<'text>, Span)> {
        self.parsed()
            .events
            .iter()
            .map(move |(event, span)| (event.borrow(self.text()), *span))
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

    pub(crate) fn chars(self) -> &'text [(usize, char)] {
        self.source
            .chars
            .get_or_init(|| self.text().char_indices().collect())
    }

    pub(crate) fn line_starts(self) -> &'text [CharPos] {
        self.source
            .line_starts
            .get_or_init(|| punctuation::build_line_starts(self.chars()))
    }

    pub(crate) fn lines(self) -> impl Iterator<Item = (usize, &'text str, &'text str)> {
        let lines = self
            .source
            .lines
            .get_or_init(|| line_syntax::scan_lines(self.text()).collect());
        lines
            .iter()
            .enumerate()
            .map(move |(line, &(start, end, next))| {
                (line, &self.text()[start..end], &self.text()[end..next])
            })
    }

    pub(crate) fn line_contents(self) -> Vec<&'text str> {
        self.lines().map(|(_, line, _)| line).collect()
    }

    pub(crate) fn tokens(self) -> &'text [(lexical::TokenSpan, lexical::Kind)] {
        self.source
            .tokens
            .get_or_init(|| lexical::scan(self.text()))
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

    pub(crate) fn seed_facts(self, facts: ParsedFacts) {
        self.source.parsed.get_or_init(|| facts);
    }
}

#[derive(Default)]
pub(crate) struct ParsedFacts {
    pub(crate) scalars: Vec<(ScalarStyle, Span)>,
    pub(crate) root_scalars: HashMap<usize, usize>,
    pub(crate) document_starts: Vec<usize>,
    pub(crate) valid: bool,
    pub(crate) error: Option<ScanError>,
    events: Vec<(StoredEvent, Span)>,
}

impl ParsedFacts {
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
                self.scalars.push((*style, span));
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

    pub(crate) fn parse_with_options(
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
            facts.record(&event, span, &mut depth);
            facts.events.push((StoredEvent::new(text, event), span));
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
