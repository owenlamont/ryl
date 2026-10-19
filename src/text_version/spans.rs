use granit_parser::{Marker, Span};

pub(super) enum StoredSpan {
    Compact([u32; 9]),
    Wide(Box<Span>),
}

impl StoredSpan {
    pub(super) fn new(span: Span) -> Self {
        let values = [
            Some(span.start.index()),
            span.start.byte_offset(),
            Some(span.start.line()),
            Some(span.start.col()),
            Some(span.end.index()),
            span.end.byte_offset(),
            Some(span.end.line()),
            Some(span.end.col()),
            span.indent,
        ];
        let mut compact = [u32::MAX; 9];
        for (slot, value) in compact.iter_mut().zip(values) {
            if let Some(value) = value {
                let Ok(value) = u32::try_from(value) else {
                    return Self::Wide(Box::new(span));
                };
                if value == u32::MAX {
                    return Self::Wide(Box::new(span));
                }
                *slot = value;
            }
        }
        if span.tag_start().is_some() {
            Self::Wide(Box::new(span))
        } else {
            Self::Compact(compact)
        }
    }

    pub(super) fn get(&self) -> Span {
        match self {
            Self::Wide(span) => **span,
            Self::Compact(values) => {
                let optional =
                    |value: u32| (value != u32::MAX).then_some(value as usize);
                let marker = |at| {
                    Marker::new(
                        values[at] as usize,
                        values[at + 2] as usize,
                        values[at + 3] as usize,
                    )
                    .with_byte_offset(optional(values[at + 1]))
                };
                Span::new(marker(0), marker(4)).with_indent(optional(values[8]))
            }
        }
    }
}
