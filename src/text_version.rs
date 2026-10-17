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
    epoch: Rc<()>,
    parsed: OnceCell<ParsedFacts>,
    headers: OnceCell<Vec<Header>>,
    chars: OnceCell<Vec<(usize, char)>>,
    line_starts: OnceCell<Vec<CharPos>>,
    lines: OnceCell<Vec<(usize, usize, usize)>>,
    tokens: OnceCell<Vec<(Span, lexical::Kind)>>,
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
}

impl Deref for TextVersion<'_> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.text
    }
}

impl<'text> Analysis<'text> {
    #[must_use]
    pub fn text(self) -> &'text str {
        &self.source.text
    }

    #[must_use]
    pub fn plan(self, output: String) -> EditPlan {
        self.plan_version(TextVersion::new(output))
    }

    pub(crate) fn plan_version(self, output: TextVersion<'static>) -> EditPlan {
        EditPlan {
            epoch: Rc::clone(&self.source.epoch),
            output,
        }
    }

    pub(crate) fn parsed(self) -> &'text ParsedFacts {
        self.source
            .parsed
            .get_or_init(|| ParsedFacts::parse(self.text()))
    }

    pub(crate) fn headers(self) -> &'text [Header] {
        self.source
            .headers
            .get_or_init(|| block_scalar_chomping::headers_with_analysis(self))
    }

    pub(crate) fn events(self) -> Vec<(Event<'text>, Span)> {
        events::borrow_events(self.text(), &self.parsed().events)
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

    pub(crate) fn tokens(self) -> &'text [(Span, lexical::Kind)] {
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

    pub(crate) fn seed_events(self, events: &[(Event<'_>, Span)]) {
        self.source.parsed.get_or_init(|| {
            let mut facts = ParsedFacts::default();
            let mut depth = 0;
            for (event, span) in events {
                facts.record(event, *span, &mut depth);
                facts
                    .events
                    .push((StoredEvent::new(self.text(), event.clone()), *span));
            }
            facts.valid = true;
            facts
        });
    }
}

#[derive(Default)]
pub(crate) struct ParsedFacts {
    pub(crate) scalars: Vec<(ScalarStyle, Span)>,
    pub(crate) root_scalars: HashMap<usize, usize>,
    pub(crate) valid: bool,
    pub(crate) error: Option<ScanError>,
    events: Vec<(StoredEvent, Span)>,
}

impl ParsedFacts {
    fn record(&mut self, event: &Event<'_>, span: Span, depth: &mut usize) {
        match event {
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

    fn parse(text: &str) -> Self {
        let mut facts = Self::default();
        let mut depth = 0;
        for item in Parser::new_from_str(text) {
            let (event, span) = match item {
                Ok(pair) => pair,
                Err(error) => {
                    facts.error = Some(error);
                    return facts;
                }
            };
            facts.record(&event, span, &mut depth);
            facts.events.push((StoredEvent::new(text, event), span));
        }
        facts.valid = true;
        facts
    }
}

impl From<String> for TextVersion<'static> {
    fn from(text: String) -> Self {
        Self::new(text)
    }
}
