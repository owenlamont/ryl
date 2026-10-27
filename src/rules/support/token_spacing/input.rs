use crate::rules::quoted_strings::{self, RetainedRegions};
use crate::text_version::Analysis;

pub(super) struct Indicators<'a> {
    text: &'a str,
    start: usize,
    scalars: RetainedRegions<'a>,
}

impl<'a> Indicators<'a> {
    pub(super) fn new(input: Analysis<'a>, range: std::ops::Range<usize>) -> Self {
        Self {
            text: &input.text()[range.clone()],
            start: range.start,
            scalars: quoted_strings::retained_regions(input),
        }
    }

    pub(super) fn spacing_prefixes(
        &self,
    ) -> impl Iterator<Item = (std::ops::Range<usize>, usize)> + '_ {
        self.text
            .match_indices(['-', '?', ':'])
            .filter(|(at, _)| {
                (*at == 0 || self.text[..*at].ends_with(' '))
                    && !self.scalars.contains(self.start + at)
            })
            .map(|(at, _)| {
                let spaces = self.text[at + 1..]
                    .bytes()
                    .take_while(|&byte| byte == b' ')
                    .count();
                (self.start + at + 1..self.start + at + 1 + spaces, spaces)
            })
    }
}

#[derive(Clone, Copy)]
pub(super) struct ExplicitKey {
    pub(super) key: bool,
    pub(super) single_space: bool,
}

impl ExplicitKey {
    pub(super) fn observe(text: &str, from: usize, to: usize) -> Self {
        let bytes = text.as_bytes();
        Self {
            key: bytes[..from].ends_with(b"?")
                && (from < 2 || b"[{, \t\r\n".contains(&bytes[from - 2])),
            single_space: to - from == 1 && bytes.get(from) == Some(&b' '),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum ExtentKind {
    Open,
    Close,
    Comment,
    BlockScalar,
    Other,
}

pub(super) enum Adjacent {
    Value,
    Property,
    Other,
}

pub(super) struct Extent {
    pub(super) kind: ExtentKind,
    pub(super) adjacent: Adjacent,
    pub(super) first: usize,
    pub(super) last: usize,
}

impl Extent {
    pub(super) fn observe(
        span: ryl_yaml_parser::Span,
        token: &crate::text_version::lexical::Kind<'_>,
    ) -> Self {
        use crate::text_version::lexical::Kind;
        let kind = match token {
            Kind::BlockMappingStart | Kind::BlockSequenceStart => ExtentKind::Open,
            Kind::BlockEnd => ExtentKind::Close,
            Kind::Comment { .. } => ExtentKind::Comment,
            Kind::Scalar { style, .. }
                if quoted_strings::retained_layout_style(*style).is_block() =>
            {
                ExtentKind::BlockScalar
            }
            _ => ExtentKind::Other,
        };
        let adjacent = match token {
            Kind::Scalar { .. } | Kind::FlowMappingEnd | Kind::FlowSequenceEnd => {
                Adjacent::Value
            }
            Kind::Alias | Kind::Anchor | Kind::Tag => Adjacent::Property,
            _ => Adjacent::Other,
        };
        Self {
            kind,
            adjacent,
            first: span.start.line(),
            last: span.end.line(),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Following {
    BreakOrComment,
    Separator,
    Content,
}

impl Following {
    pub(super) const fn observe(byte: u8) -> Self {
        match byte {
            b'\n' | b'\r' | b'#' => Self::BreakOrComment,
            b',' | b']' | b'}' => Self::Separator,
            _ => Self::Content,
        }
    }
}
