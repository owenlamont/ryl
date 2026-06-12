use proptest::prelude::*;

#[derive(Debug, Clone, Copy)]
pub enum Newline {
    Lf,
    Crlf,
    /// A bare `\r` — a YAML 1.2 line break the generator may emit freely; the
    /// harness oracle counts it as a break.
    Cr,
}

impl Newline {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::Crlf => "\r\n",
            Self::Cr => "\r",
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
    TagDirective,
    DocumentStart,
    DocumentEnd,
    Blank {
        spaces: u8,
    },
    /// An indented raw line with no key/colon structure &mdash; used to build the
    /// header and content lines of a block scalar (e.g. a standalone `  |` header
    /// on its own line, or an indented content line).
    Raw {
        indent: u8,
        text: String,
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
            Self::TagDirective => out.push_str("%TAG !e! tag:example.com,2000:"),
            Self::DocumentStart => out.push_str("---"),
            Self::DocumentEnd => out.push_str("..."),
            Self::Blank { spaces } => push_spaces(out, *spaces),
            Self::Raw { indent, text } => {
                push_spaces(out, *indent);
                out.push_str(text);
            }
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
    prop_oneof![
        Just('é'),
        Just('—'),
        Just('世'),
        Just('🦀'),
        Just('å'),
        // Raw NEL / LS / PS: content in YAML 1.2 (not line breaks), so they
        // interleave into keys/values/comments without splitting lines and
        // exercise `unicode-line-breaks` across contexts.
        Just('\u{85}'),
        Just('\u{2028}'),
        Just('\u{2029}'),
    ]
}

fn arb_key() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("a".to_string()),
        Just("b".to_string()),
        Just("c".to_string()),
        Just("dup".to_string()),
        Just("a#b".to_string()),
        Just("a !foo".to_string()),
        Just("a &foo".to_string()),
        Just("café".to_string()),
        Just("Yes".to_string()),
        Just("On".to_string()),
        // `<<`, `0xB`, `11`, `~` exercise `key-duplicates: check-canonical`
        // (standalone `<<` handling and canonical scalar equality); coherent
        // merge structures come from `arb_merge_block`.
        Just("<<".to_string()),
        Just("0xB".to_string()),
        Just("11".to_string()),
        Just("~".to_string()),
        // An alias as a mapping key: paired with the entry generator's
        // `spaces_before_colon` range it produces `*anchor : v` (the required-space
        // `colons` exemption) plus the non-exempt 0/2-space forms, and exercises an
        // alias in key position through the shared mapping-key walker.
        Just("*anchor".to_string()),
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
        // Colon welded to an anchor/alias name exercises the ryl-only
        // `forbid-ambiguous-anchor-alias-names` check, including a colon-leading
        // name and a `&` mid-plain-scalar (which must NOT be read as an anchor).
        Just("&anchor: value".to_string()),
        Just("*anchor:".to_string()),
        Just("&:lead value".to_string()),
        Just("rock&roll:thing".to_string()),
        Just("x".repeat(40)),
        Just("word ".repeat(8)),
        arb_multibyte().prop_map(|ch| format!("v{ch}")),
    ]
}

// Tag tokens spanning shorthand, local, verbatim, and non-specific forms. They
// are synthesized onto values so the no-panic / in-bounds-span invariants run
// over tagged nodes — exercising `tags::check`, tag-token positions, and
// author-facing spellings across positions, multibyte chars, and LF/CRLF. This
// suite only asserts those invariants; tag-handling correctness lives in the
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
    prop_oneof![Just(Newline::Lf), Just(Newline::Crlf), Just(Newline::Cr)]
}

fn merge_entry(indent: u8, key: &str, value: String) -> Line {
    Line::Entry {
        indent,
        key: key.to_string(),
        spaces_before_colon: 0,
        spaces_after_colon: 1,
        value,
        comment: None,
        trailing_spaces: 0,
    }
}

/// A coherent merge structure the flat line generator never assembles by chance:
/// two anchored flow-mapping bases sharing the key `dup`, then a host that merges
/// them (optionally also defining `dup` explicitly). Exercises `key-duplicates`
/// merge expansion and value-aware collision detection — both merge-vs-merge and
/// explicit-vs-merge shadowing — under the canonical config in `collect_spans`.
fn arb_merge_block() -> impl Strategy<Value = Vec<Line>> {
    (
        arb_bare_value(),
        arb_bare_value(),
        0u8..=2,
        prop::option::of(arb_bare_value()),
    )
        .prop_map(|(v0, v1, shape, shadow)| {
            let merge_value = match shape {
                0 => "*m0",
                1 => "[*m0, *m1]",
                _ => "[*m0, *m0]",
            }
            .to_string();
            let mut lines = vec![
                merge_entry(0, "b0", format!("&m0 {{dup: {v0}}}")),
                merge_entry(0, "b1", format!("&m1 {{dup: {v1}}}")),
                merge_entry(0, "h", String::new()),
                merge_entry(2, "<<", merge_value),
            ];
            if let Some(shadow) = shadow {
                lines.push(merge_entry(2, "dup", shadow));
            }
            lines
        })
}

/// A coherent block sequence-of-mappings the flat line generator never assembles by
/// chance: a parent key introducing a block sequence whose entries span the layouts
/// `hyphens: dash-on-own-line` must classify — dash+first-key on one line (the flagged
/// shape), dash-alone with the body indented below, an anchor/tag or comment on the
/// dash line (keys still below, accepted), a nested sequence whose inner mapping opens
/// on the inner dash line, and a flow/scalar value (never flagged). Drives the
/// scanner-driven detection in `collect_spans`'s dash-on-own-line dispatch over
/// multibyte keys and mixed newlines so the dual dispatch is not fuzzed vacuously.
fn arb_seq_of_mappings_block() -> impl Strategy<Value = Vec<Line>> {
    (arb_key(), 0u8..=6u8).prop_map(|(key, shape)| {
        let mut lines = vec![merge_entry(0, "items", String::new())];
        match shape {
            0 => {
                lines.push(Line::Raw {
                    indent: 2,
                    text: format!("- {key}: web"),
                });
                lines.push(Line::Raw {
                    indent: 4,
                    text: "port: 80".to_string(),
                });
            }
            1 => {
                lines.push(Line::Raw {
                    indent: 2,
                    text: "-".to_string(),
                });
                lines.push(Line::Raw {
                    indent: 4,
                    text: format!("{key}: web"),
                });
            }
            2 => {
                lines.push(Line::Raw {
                    indent: 2,
                    text: "- &a !x".to_string(),
                });
                lines.push(Line::Raw {
                    indent: 4,
                    text: format!("{key}: web"),
                });
            }
            3 => {
                lines.push(Line::Raw {
                    indent: 2,
                    text: "- # c".to_string(),
                });
                lines.push(Line::Raw {
                    indent: 4,
                    text: format!("{key}: web"),
                });
            }
            4 => lines.push(Line::Raw {
                indent: 2,
                text: format!("- - {key}: 1"),
            }),
            5 => lines.push(Line::Raw {
                indent: 2,
                text: format!("- {{{key}: 1}}"),
            }),
            _ => lines.push(Line::Raw {
                indent: 2,
                text: "- scalar".to_string(),
            }),
        }
        lines
    })
}

fn arb_custom_tag_block() -> impl Strategy<Value = Vec<Line>> {
    prop_oneof![Just("!e!keep"), Just("!e!other")].prop_map(|tag| {
        vec![
            Line::TagDirective,
            Line::DocumentStart,
            merge_entry(0, "tagged", format!("{tag} value")),
        ]
    })
}

// Block scalar headers spanning bare clip (`|`/`>`), explicit strip/keep, an
// indentation-only header (`|2`, which `block-scalar-chomping` still flags), the
// digit+chomp combination, and a trailing comment after the header.
fn arb_block_scalar_header() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("|".to_string()),
        Just(">".to_string()),
        Just("|-".to_string()),
        Just(">+".to_string()),
        Just("|+".to_string()),
        Just(">-".to_string()),
        Just("|2".to_string()),
        Just("|+2".to_string()),
        Just("| # chomp".to_string()),
        Just("!<tag:example.com,2000:app/foo#bar> |".to_string()),
    ]
}

/// A coherent block scalar the flat line generator never assembles: a key
/// introducing a literal/folded block plus an indented content line. The header
/// sits on the key's line, on its own line below the key, or above a blank gap —
/// the layouts where the header is *not* the content line, which is exactly what
/// `block-scalar-chomping`'s header recovery must handle.
fn arb_block_scalar_block() -> impl Strategy<Value = Vec<Line>> {
    (
        arb_key(),
        arb_block_scalar_header(),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(|(key, header, header_on_own_line, blank_gap, blank_only)| {
            let content = Line::Raw {
                indent: 4,
                text: if blank_only {
                    String::new()
                } else {
                    "block content".to_string()
                },
            };
            if header_on_own_line {
                return vec![
                    merge_entry(0, &key, String::new()),
                    Line::Raw {
                        indent: 2,
                        text: header,
                    },
                    content,
                ];
            }
            let mut lines = vec![merge_entry(0, &key, header)];
            if blank_gap {
                lines.push(Line::Blank { spaces: 0 });
            }
            lines.push(content);
            lines
        })
}

fn arb_fragment() -> impl Strategy<Value = Vec<(Line, Newline)>> {
    prop_oneof![
        10 => (arb_line(), arb_newline()).prop_map(|pair| vec![pair]),
        1 => (arb_merge_block(), arb_newline()).prop_map(|(lines, newline)| {
            lines.into_iter().map(|line| (line, newline)).collect()
        }),
        1 => (arb_custom_tag_block(), arb_newline()).prop_map(|(lines, newline)| {
            lines.into_iter().map(|line| (line, newline)).collect()
        }),
        2 => (arb_block_scalar_block(), arb_newline()).prop_map(|(lines, newline)| {
            lines.into_iter().map(|line| (line, newline)).collect()
        }),
        2 => (arb_seq_of_mappings_block(), arb_newline()).prop_map(|(lines, newline)| {
            lines.into_iter().map(|line| (line, newline)).collect()
        }),
    ]
}

pub fn arb_document() -> impl Strategy<Value = Document> {
    (prop::collection::vec(arb_fragment(), 1..=8), any::<bool>()).prop_map(
        |(fragments, has_final_newline)| Document {
            lines: fragments.into_iter().flatten().collect(),
            has_final_newline,
        },
    )
}
