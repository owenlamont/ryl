//! Each document's granit events without marks or comments: what a layout rewrite must
//! leave unchanged.

use granit_parser::{Event, Parser};

pub(crate) struct Document<'a> {
    /// The char index the document starts at.
    pub(crate) start: usize,
    pub(crate) events: Vec<Event<'a>>,
}

/// The documents of `text`, and whether the whole stream parsed.
pub(crate) fn documents(text: &str) -> (Vec<Document<'_>>, bool) {
    let mut documents: Vec<Document<'_>> = Vec::new();
    for item in Parser::new_from_str(text) {
        let Ok((event, span)) = item else {
            return (documents, false);
        };
        match event {
            Event::StreamStart | Event::StreamEnd | Event::Comment(..) => {}
            Event::DocumentStart(..) => documents.push(Document {
                start: span.start.index(),
                events: vec![event],
            }),
            event => documents
                .last_mut()
                .expect("every event follows a document start")
                .events
                .push(event),
        }
    }
    (documents, true)
}
