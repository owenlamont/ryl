use ryl_yaml_parser::{Marker, Span};

const INDENT_AT_START: u32 = 1 << 31;

#[inline]
fn narrow_coordinate(value: usize) -> Option<u32> {
    u32::try_from(value).ok().filter(|&value| value != u32::MAX)
}

#[derive(Clone, Copy)]
pub(super) struct StoredSpan([u32; 8]);

#[derive(Default)]
pub(super) struct Spans {
    wide: Vec<Span>,
}

impl Spans {
    #[inline]
    pub(super) fn store(&mut self, span: &Span) -> StoredSpan {
        let compact = || {
            if span.tag_start().is_some()
                || span.indent.is_some_and(|indent| indent != span.start.col())
            {
                return None;
            }
            let values = [
                Some(span.start.index()),
                span.start.byte_offset(),
                Some(span.start.line()),
                Some(span.start.col()),
                Some(span.end.index()),
                span.end.byte_offset(),
                Some(span.end.line()),
                Some(span.end.col()),
            ];
            let mut compact = [u32::MAX; 8];
            for (slot, value) in compact.iter_mut().zip(values) {
                *slot = value.map_or(Some(u32::MAX), narrow_coordinate)?;
            }
            if compact[2] >= INDENT_AT_START || compact[6] >= INDENT_AT_START {
                return None;
            }
            compact[2] |= u32::from(span.indent.is_some()) * INDENT_AT_START;
            Some(StoredSpan(compact))
        };
        compact().unwrap_or_else(|| {
            let index =
                u32::try_from(self.wide.len()).expect("wide span count fits u32");
            self.wide.push(*span);
            let mut values = [u32::MAX; 8];
            values[1] = index;
            StoredSpan(values)
        })
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
