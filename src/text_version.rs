use std::borrow::Cow;
use std::cell::OnceCell;
use std::collections::HashMap;
use std::ops::Deref;
use std::rc::Rc;

use granit_parser::{Event, Parser, ScalarStyle, Span};

use crate::rules::block_scalar_chomping::{self, Header};

pub struct TextVersion<'text> {
    text: Cow<'text, str>,
    epoch: Rc<()>,
    parsed: OnceCell<ParsedFacts>,
    headers: OnceCell<Vec<Header>>,
}

#[derive(Clone, Copy)]
pub struct Analysis<'text> {
    text: &'text str,
    epoch: &'text Rc<()>,
    parsed: &'text OnceCell<ParsedFacts>,
    headers: &'text OnceCell<Vec<Header>>,
}

pub struct EditPlan {
    epoch: Rc<()>,
    output: String,
}

impl<'text> TextVersion<'text> {
    pub fn new(text: impl Into<Cow<'text, str>>) -> Self {
        Self {
            text: text.into(),
            epoch: Rc::new(()),
            parsed: OnceCell::new(),
            headers: OnceCell::new(),
        }
    }

    pub fn analysis(&self) -> Analysis<'_> {
        Analysis {
            text: &self.text,
            epoch: &self.epoch,
            parsed: &self.parsed,
            headers: &self.headers,
        }
    }

    /// Returns `None` for a plan from another epoch, otherwise whether bytes changed.
    pub fn apply(&mut self, plan: EditPlan) -> Option<bool> {
        if !Rc::ptr_eq(&self.epoch, &plan.epoch) {
            return None;
        }
        if self.text == plan.output {
            return Some(false);
        }
        *self = Self::new(plan.output);
        Some(true)
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
        self.text
    }

    #[must_use]
    pub fn plan(self, output: String) -> EditPlan {
        EditPlan {
            epoch: Rc::clone(self.epoch),
            output,
        }
    }

    pub(crate) fn parsed(self) -> &'text ParsedFacts {
        self.parsed.get_or_init(|| ParsedFacts::parse(self.text))
    }

    pub(crate) fn headers(self) -> &'text [Header] {
        self.headers
            .get_or_init(|| block_scalar_chomping::headers(self.text))
    }

    pub(crate) fn seed_events(self, events: &[(Event<'_>, Span)]) {
        self.parsed.get_or_init(|| {
            let mut facts = ParsedFacts::default();
            let mut depth = 0;
            for (event, span) in events {
                facts.record(event, *span, &mut depth);
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
            let Ok((event, span)) = item else {
                return facts;
            };
            facts.record(&event, span, &mut depth);
        }
        facts.valid = true;
        facts
    }
}
