//! Stacks file-shape issues around the safe-fix generator's entries so several fixers
//! act on the same lines and must agree: indented comments, whitespace-only blank lines,
//! trailing spaces after a flow value or inline comment, `---`/`...` markers and extra
//! end-of-file blanks, on top of the flow spacing and quoting `arb_document` emits.

use proptest::prelude::*;

use super::ast::{BlockEntry, Document, NewlineStyle};
use super::strategy::arb_document;

#[derive(Debug, Clone)]
pub enum Filler {
    Blank {
        spaces: u8,
    },
    Comment {
        indent: u8,
        spaces_after_hash: u8,
        text: String,
    },
}

#[derive(Debug, Clone)]
pub struct Decoration {
    pub leading: Vec<Filler>,
    pub trailing_spaces: u8,
}

#[derive(Debug, Clone)]
pub struct StackedDocument {
    pub document: Document,
    pub decorations: Vec<Decoration>,
    pub start_marker: bool,
    pub end_marker: bool,
    pub trailing_blank_lines: u8,
}

fn line_terminator(newline: NewlineStyle) -> &'static str {
    match newline {
        NewlineStyle::Lf => "\n",
        NewlineStyle::Crlf => "\r\n",
        NewlineStyle::Cr => "\r",
    }
}

impl Filler {
    fn render(&self, buffer: &mut String) {
        match self {
            Self::Blank { spaces } => {
                buffer.push_str(&" ".repeat(usize::from(*spaces)))
            }
            Self::Comment {
                indent,
                spaces_after_hash,
                text,
            } => {
                buffer.push_str(&" ".repeat(usize::from(*indent)));
                buffer.push('#');
                buffer.push_str(&" ".repeat(usize::from(*spaces_after_hash)));
                buffer.push_str(text);
            }
        }
    }
}

impl StackedDocument {
    pub fn render(&self) -> String {
        let newline = self.document.newline;
        let terminator = line_terminator(newline);
        let single = |entries: Vec<BlockEntry>, version_directive| {
            Document {
                version_directive,
                entries,
                newline,
                has_final_newline: false,
            }
            .render()
        };
        let mut buffer = String::new();
        if self.document.version_directive.is_some() {
            buffer.push_str(&single(Vec::new(), self.document.version_directive));
        } else if self.start_marker {
            buffer.push_str("---");
            buffer.push_str(terminator);
        }
        for (entry, decoration) in self.document.entries.iter().zip(&self.decorations) {
            for filler in &decoration.leading {
                filler.render(&mut buffer);
                buffer.push_str(terminator);
            }
            let text = single(vec![entry.clone()], None);
            buffer.push_str(&text);
            // Trailing spaces after a block or multi-line scalar's last line are scalar
            // content, so only single-line entries get them.
            if !text.contains(['\n', '\r']) {
                buffer.push_str(&" ".repeat(usize::from(decoration.trailing_spaces)));
            }
            buffer.push_str(terminator);
        }
        if self.end_marker {
            buffer.push_str("...");
            buffer.push_str(terminator);
        }
        for _ in 0..self.trailing_blank_lines {
            buffer.push_str(terminator);
        }
        if !self.document.has_final_newline {
            buffer.truncate(buffer.len() - terminator.len());
        }
        buffer
    }
}

fn arb_filler() -> impl Strategy<Value = Filler> {
    prop_oneof![
        (0u8..=2).prop_map(|spaces| Filler::Blank { spaces }),
        (0u8..=3, 0u8..=2, "[a-z][a-z0-9 ]{0,6}").prop_map(
            |(indent, spaces_after_hash, text)| Filler::Comment {
                indent,
                spaces_after_hash,
                text,
            }
        ),
    ]
}

fn arb_decoration() -> impl Strategy<Value = Decoration> {
    (prop::collection::vec(arb_filler(), 0..=3), 0u8..=3).prop_map(
        |(leading, trailing_spaces)| Decoration {
            leading,
            trailing_spaces,
        },
    )
}

pub fn arb_stacked_document() -> impl Strategy<Value = StackedDocument> {
    arb_document().prop_flat_map(|document| {
        let entries = document.entries.len();
        (
            Just(document),
            prop::collection::vec(arb_decoration(), entries),
            any::<bool>(),
            any::<bool>(),
            0u8..=3,
        )
            .prop_map(
                |(
                    document,
                    decorations,
                    start_marker,
                    end_marker,
                    trailing_blank_lines,
                )| {
                    StackedDocument {
                        document,
                        decorations,
                        start_marker,
                        end_marker,
                        trailing_blank_lines,
                    }
                },
            )
    })
}
