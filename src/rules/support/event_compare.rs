//! Each document's granit events without marks or comments: what a layout rewrite must
//! leave unchanged.

use granit_parser::{Event, Span};

pub(crate) struct Document<'a> {
    /// The char index the document starts at.
    pub(crate) start: usize,
    pub(crate) events: Vec<Event<'a>>,
}

pub(crate) fn documents_with_analysis(
    input: crate::text_version::Analysis<'_>,
) -> (Vec<Document<'_>>, bool) {
    (documents_from_events(&input.events()), input.parsed().valid)
}

pub(crate) fn documents_from_events<'a>(
    events: &[(Event<'a>, Span)],
) -> Vec<Document<'a>> {
    let mut documents: Vec<Document<'a>> = Vec::new();
    for (event, span) in events {
        match event {
            Event::StreamStart | Event::StreamEnd | Event::Comment(..) => {}
            Event::DocumentStart(..) => documents.push(Document {
                start: span.start.index(),
                events: vec![event.clone()],
            }),
            event => documents
                .last_mut()
                .expect("every event follows a document start")
                .events
                .push(event.clone()),
        }
    }
    documents
}
