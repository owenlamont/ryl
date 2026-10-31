use ryl_yaml_parser::{Marker, Span};

const INDENT_AT_START: u32 = 1 << 31;

#[derive(Clone, Copy)]
pub(super) struct StoredSpan([u32; 8]);

#[derive(Default)]
pub(super) struct Spans {
    wide: Vec<Span>,
}

impl Spans {
    #[inline]
    pub(super) fn store(&mut self, span: &Span) -> StoredSpan {
        let (start, end) = (span.start, span.end);
        let widest = start
            .index()
            .max(end.index())
            .max(start.col())
            .max(end.col())
            .max(start.byte_offset().unwrap_or(0))
            .max(end.byte_offset().unwrap_or(0));
        let narrow = |value: usize| u32::try_from(value).unwrap_or(u32::MAX);
        if narrow(widest) < u32::MAX
            && narrow(start.line().max(end.line())) < INDENT_AT_START
            && span.tag_start().is_none()
            && span.indent.is_none_or(|indent| indent == start.col())
        {
            let byte = |at: Option<usize>| at.map_or(u32::MAX, narrow);
            return StoredSpan([
                narrow(start.index()),
                byte(start.byte_offset()),
                narrow(start.line())
                    | (u32::from(span.indent.is_some()) * INDENT_AT_START),
                narrow(start.col()),
                narrow(end.index()),
                byte(end.byte_offset()),
                narrow(end.line()),
                narrow(end.col()),
            ]);
        }
        let index = u32::try_from(self.wide.len()).expect("wide span count fits u32");
        self.wide.push(*span);
        let mut values = [u32::MAX; 8];
        values[1] = index;
        StoredSpan(values)
    }

    #[inline]
    pub(super) fn bytes(&self, span: StoredSpan) -> (usize, usize) {
        let values = span.0;
        if values[0] == u32::MAX {
            let span = self.wide[values[1] as usize];
            let byte = |marker: Marker| marker.byte_offset().unwrap_or(usize::MAX);
            return (byte(span.start), byte(span.end));
        }
        (values[1] as usize, values[5] as usize)
    }

    #[inline]
    pub(super) fn get(&self, span: StoredSpan) -> Span {
        let values = span.0;
        if values[0] == u32::MAX {
            return self.wide[values[1] as usize];
        }
        let optional = |value: u32| (value != u32::MAX).then_some(value as usize);
        let marker = |at| {
            Marker::new(
                values[at] as usize,
                (values[at + 2] & !INDENT_AT_START) as usize,
                values[at + 3] as usize,
            )
            .with_byte_offset(optional(values[at + 1]))
        };
        Span::new(marker(0), marker(4)).with_indent(
            (values[2] & INDENT_AT_START != 0).then_some(values[3] as usize),
        )
    }
}
