use proptest::prelude::*;

#[derive(Debug, Clone, Copy)]
pub enum Newline {
    Lf,
    Crlf,
}

impl Newline {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::Crlf => "\r\n",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Line {
    Entry {
        indent: u8,
        key: String,
        spaces_before_colon: u8,
        spaces_after_colon: u8,
        value: String,
        comment: Option<(u8, String)>,
        trailing_spaces: u8,
    },
    SeqItem {
        indent: u8,
        spaces_after_dash: u8,
        value: String,
        trailing_spaces: u8,
    },
    Comment {
        indent: u8,
        spaces_after_hash: u8,
        text: String,
    },
    DocumentStart,
    DocumentEnd,
    Blank {
        spaces: u8,
    },
}

#[derive(Debug, Clone)]
pub struct Document {
    pub lines: Vec<(Line, Newline)>,
    pub has_final_newline: bool,
}

fn push_spaces(out: &mut String, count: u8) {
    for _ in 0..count {
        out.push(' ');
    }
}

impl Line {
    fn render(&self, out: &mut String) {
        match self {
            Self::Entry {
                indent,
                key,
                spaces_before_colon,
                spaces_after_colon,
                value,
                comment,
                trailing_spaces,
            } => {
                push_spaces(out, *indent);
                out.push_str(key);
                push_spaces(out, *spaces_before_colon);
                out.push(':');
                push_spaces(out, *spaces_after_colon);
                out.push_str(value);
                if let Some((spaces_after_hash, text)) = comment {
                    out.push_str("  #");
                    push_spaces(out, *spaces_after_hash);
                    out.push_str(text);
                }
                push_spaces(out, *trailing_spaces);
            }
            Self::SeqItem {
                indent,
                spaces_after_dash,
                value,
                trailing_spaces,
            } => {
                push_spaces(out, *indent);
                out.push('-');
                push_spaces(out, *spaces_after_dash);
                out.push_str(value);
                push_spaces(out, *trailing_spaces);
            }
            Self::Comment {
                indent,
                spaces_after_hash,
                text,
            } => {
                push_spaces(out, *indent);
                out.push('#');
                push_spaces(out, *spaces_after_hash);
                out.push_str(text);
            }
            Self::DocumentStart => out.push_str("---"),
            Self::DocumentEnd => out.push_str("..."),
            Self::Blank { spaces } => push_spaces(out, *spaces),
        }
    }
}

impl Document {
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        let last = self.lines.len().saturating_sub(1);
        for (index, (line, newline)) in self.lines.iter().enumerate() {
            line.render(&mut out);
            if index < last || self.has_final_newline {
                out.push_str(newline.as_str());
            }
        }
        out
    }
}

fn arb_multibyte() -> impl Strategy<Value = char> {
    prop_oneof![Just('é'), Just('—'), Just('世'), Just('🦀'), Just('å')]
}

fn arb_key() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("a".to_string()),
        Just("b".to_string()),
        Just("c".to_string()),
        Just("dup".to_string()),
        Just("café".to_string()),
        Just("Yes".to_string()),
        Just("On".to_string()),
        // `<<`, `0xB`, `11`, `~` exercise `key-duplicates: check-canonical`
        // (merge expansion and canonical scalar equality) under the canonical
        // config in `harness::collect_spans`.
        Just("<<".to_string()),
        Just("0xB".to_string()),
        Just("11".to_string()),
        Just("~".to_string()),
        arb_multibyte().prop_map(|ch| format!("k{ch}")),
    ]
}

fn arb_bare_value() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),
        Just("value".to_string()),
        Just("Yes".to_string()),
        Just("No".to_string()),
        Just("Off".to_string()),
        Just("True".to_string()),
        Just("010".to_string()),
        Just("0o17".to_string()),
        Just("0.5".to_string()),
        Just(".5".to_string()),
        Just("1e3".to_string()),
        Just(".inf".to_string()),
        Just(".nan".to_string()),
        Just("'plain'".to_string()),
        Just("\"plain\"".to_string()),
        Just("[a , b]".to_string()),
        Just("{ a: 1 }".to_string()),
        Just("[ ]".to_string()),
        Just("{}".to_string()),
        Just("&anchor value".to_string()),
        Just("*anchor".to_string()),
        Just("x".repeat(40)),
        Just("word ".repeat(8)),
        arb_multibyte().prop_map(|ch| format!("v{ch}")),
    ]
}

// Tag tokens spanning shorthand, local, verbatim, and non-specific forms. They
// are synthesized onto values so the no-panic / in-bounds-span invariants run
// over tagged nodes — exercising `tags::check`, the spelling normalisation, and
// `clamp_overshoot` across positions, multibyte chars, and LF/CRLF. This suite
// only asserts those invariants; tag-handling correctness lives in the
// deterministic CLI tests.
fn arb_tag() -> impl Strategy<Value = &'static str> {
    prop_oneof![
        Just("!!str"),
        Just("!!omap"),
        Just("!!set"),
        Just("!!python/object/apply:os.system"),
        Just("!!javax.script.ScriptEngineManager"),
        Just("!env"),
        Just("!keep"),
        Just("!ruby/object:Foo"),
        Just("!<tag:yaml.org,2002:int>"),
        Just("!<!python/object>"),
        Just("!"),
    ]
}

fn arb_value() -> impl Strategy<Value = String> {
    (prop::option::weighted(0.35, arb_tag()), arb_bare_value()).prop_map(
        |(tag, base)| match tag {
            None => base,
            // An empty base leaves the tag on an implicit scalar — the
            // `clamp_overshoot` hot path when it ends the document.
            Some(tag) if base.is_empty() => tag.to_string(),
            Some(tag) => format!("{tag} {base}"),
        },
    )
}

fn arb_text() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("note".to_string()),
        Just(String::new()),
        arb_multibyte().prop_map(|ch| format!("c{ch}")),
    ]
}

fn arb_entry() -> impl Strategy<Value = Line> {
    (
        0u8..=4,
        arb_key(),
        0u8..=2,
        0u8..=3,
        arb_value(),
        prop::option::of((0u8..=2, arb_text())),
        0u8..=3,
    )
        .prop_map(
            |(
                indent,
                key,
                spaces_before_colon,
                spaces_after_colon,
                value,
                comment,
                trailing_spaces,
            )| Line::Entry {
                indent,
                key,
                spaces_before_colon,
                spaces_after_colon,
                value,
                comment,
                trailing_spaces,
            },
        )
}

fn arb_seq_item() -> impl Strategy<Value = Line> {
    (0u8..=4, 0u8..=3, arb_value(), 0u8..=3).prop_map(
        |(indent, spaces_after_dash, value, trailing_spaces)| Line::SeqItem {
            indent,
            spaces_after_dash,
            value,
            trailing_spaces,
        },
    )
}

fn arb_comment() -> impl Strategy<Value = Line> {
    (0u8..=4, 0u8..=2, arb_text()).prop_map(|(indent, spaces_after_hash, text)| {
        Line::Comment {
            indent,
            spaces_after_hash,
            text,
        }
    })
}

fn arb_line() -> impl Strategy<Value = Line> {
    prop_oneof![
        10 => arb_entry(),
        3 => arb_seq_item(),
        2 => arb_comment(),
        1 => Just(Line::DocumentStart),
        1 => Just(Line::DocumentEnd),
        2 => (0u8..=3).prop_map(|spaces| Line::Blank { spaces }),
    ]
}

fn arb_newline() -> impl Strategy<Value = Newline> {
    prop_oneof![Just(Newline::Lf), Just(Newline::Crlf)]
}

pub fn arb_document() -> impl Strategy<Value = Document> {
    (
        prop::collection::vec((arb_line(), arb_newline()), 1..=8),
        any::<bool>(),
    )
        .prop_map(|(lines, has_final_newline)| Document {
            lines,
            has_final_newline,
        })
}
