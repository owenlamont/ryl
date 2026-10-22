//! Each document's parser events without marks or comments: what a layout rewrite must
//! leave unchanged.

use ryl_yaml_parser::Event;

pub(crate) fn documents<'a>(
    events: impl IntoIterator<Item = Event<'a>>,
) -> Vec<Vec<Event<'a>>> {
    let mut documents: Vec<Vec<Event<'a>>> = Vec::new();
    for event in events {
        match event {
            Event::StreamStart | Event::StreamEnd | Event::Comment(..) => {}
            Event::DocumentStart(..) => documents.push(vec![event]),
            event => documents
                .last_mut()
                .expect("every event follows a document start")
                .push(event),
        }
    }
    documents
}
