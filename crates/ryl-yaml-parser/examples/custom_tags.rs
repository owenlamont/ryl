use granit_parser::{Event, Parser};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let yaml = "opw_kinematics_joint_offsets: !degrees [0, 0, -90, 0, 0, 180]\n";

    for next in Parser::new_from_str(yaml) {
        let (event, span) = next?;

        let node_kind = match &event {
            Event::Scalar(..) => "scalar",
            Event::SequenceStart(..) => "sequence",
            Event::MappingStart(..) => "mapping",
            _ => continue,
        };

        if let Some(tag) = event.tag() {
            let (handle, suffix) = tag.parts();
            println!(
                "tag={tag} custom={} parts=({handle:?}, {suffix:?}) node={node_kind} event={event:?} chars={}..{} bytes={:?}",
                tag.is_custom(),
                span.start.index(),
                span.end.index(),
                span.byte_range()
            );
        }
    }

    Ok(())
}
