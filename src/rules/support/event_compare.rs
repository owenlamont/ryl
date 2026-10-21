//! Each document's parser events without marks or comments: what a layout rewrite must
//! leave unchanged.

use ryl_yaml_parser::Event;

pub(crate) struct Document<'a> {
    pub(crate) events: Vec<Event<'a>>,
}

pub(crate) fn documents_with_analysis(
    input: crate::text_version::Analysis<'_>,
) -> (Vec<Document<'_>>, bool) {
    let mut documents: Vec<Document<'_>> = Vec::new();
    for (event, _) in input.event_iter() {
        match event {
            Event::StreamStart | Event::StreamEnd | Event::Comment(..) => {}
            Event::DocumentStart(..) => documents.push(Document {
                events: vec![event],
            }),
            event => documents
                .last_mut()
                .expect("every event follows a document start")
                .events
                .push(event),
        }
    }
    (documents, input.parsed().valid)
}
