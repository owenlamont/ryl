//! Documents built to be folded: multi-word plain and quoted scalars in every block
//! position (mapping value, sequence entry, compact `- key:`, nested sequence, root),
//! beside the multi-word keys, flow collections and explicit keys a fold must leave alone.
//! Words after the first start with YAML indicators, `---`/`...`, escapes, `''` or
//! multibyte characters, and are joined by spaces, double spaces, tabs or no-break spaces.
//! `>` and `|` bodies, some empty or blank-only, mix content, more-indented, tab-led and
//! blank lines, with and without an indentation indicator, under every chomping mode.
//!
//! Two oracles that need no YAML parser's leniency: `inserted_breaks` proves the output is
//! the input with lone spaces turned into indented line breaks and no line grown, and
//! `under_indented_break` proves each inserted continuation is deeper than the block
//! collection owning its scalar, and at least two columns deeper under a quoted one.

use granit_parser::{ScalarStyle, Scanner, StrInput, TokenType};
use proptest::prelude::*;

const FIRST_WORDS: [&str; 4] = ["aaa", "b", "世界", "é"];

const WORDS: [&str; 26] = [
    "aaa",
    "bbbbbbbbbb",
    "-",
    "-x",
    "?",
    ":x",
    "a:b",
    "&x",
    "*x",
    "!x",
    "[x]",
    "{x}",
    "|",
    ">",
    "'x'",
    "\"x\"",
    "%x",
    "@x",
    "`x`",
    ",x",
    "---",
    "...",
    "é",
    "世界",
    "🦀",
    "x#",
];

const SINGLE_QUOTED_WORDS: [&str; 10] = [
    "aaa",
    "bbbbbbbbbb",
    "it''s",
    "''",
    "#x",
    "-",
    "---",
    ":",
    "\"x\"",
    "\\",
];

const DOUBLE_QUOTED_WORDS: [&str; 11] = [
    "aaa",
    "bbbbbbbbbb",
    "x\\ y",
    "\\t",
    "\\\\",
    "\\\"",
    "\\u00e9",
    "#x",
    "'x'",
    "---",
    ":",
];

const SEPARATORS: [&str; 8] = [" ", " ", " ", " ", "  ", "\t", " \t", "\u{a0}"];

fn arb_words(words: &'static [&'static str]) -> impl Strategy<Value = String> {
    (
        prop::sample::select(&FIRST_WORDS[..]),
        prop::collection::vec(
            (
                prop::sample::select(&SEPARATORS[..]),
                prop::sample::select(words),
            ),
            1..=8,
        ),
    )
        .prop_map(|(first, rest)| {
            rest.into_iter()
                .fold(first.to_string(), |text, (gap, word)| {
                    format!("{text}{gap}{word}")
                })
        })
}

fn arb_value() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => arb_plain(),
        1 => arb_words(&SINGLE_QUOTED_WORDS).prop_map(|text| format!("'{text}'")),
        1 => arb_words(&DOUBLE_QUOTED_WORDS).prop_map(|text| format!("\"{text}\"")),
    ]
}

fn arb_plain() -> impl Strategy<Value = String> {
    arb_words(&WORDS)
}

fn arb_flow_safe() -> impl Strategy<Value = String> {
    arb_words(&WORDS[..2])
}

fn arb_comment() -> impl Strategy<Value = String> {
    prop::option::of("[a-z ]{0,12}").prop_map(|comment| {
        comment.map_or_else(String::new, |text| format!(" #{text}"))
    })
}

fn arb_block_scalar(base: usize) -> impl Strategy<Value = String> {
    let indent =
        move |extra: usize, text: String| format!("{}{text}", " ".repeat(base + extra));
    let blank = || prop_oneof![Just(String::new()), Just(" ".to_string())];
    let line = prop_oneof![
        4 => arb_plain().prop_map(move |text| indent(0, text)),
        1 => arb_plain().prop_map(move |text| indent(2, text)),
        1 => arb_plain().prop_map(move |text| indent(0, format!("\t{text}"))),
        1 => blank(),
    ];
    let body = prop_oneof![
        4 => (arb_plain(), prop::collection::vec(line, 0..=4)).prop_map(
            move |(first, rest)| std::iter::once(indent(0, first)).chain(rest).collect()
        ),
        1 => prop::collection::vec(blank(), 0..=2),
    ];
    (
        prop_oneof![Just(">"), Just("|")],
        prop_oneof![Just(""), Just("2")],
        prop_oneof![Just(""), Just("-"), Just("+")],
        body,
    )
        .prop_map(
            |(style, indicator, chomp, body): (_, _, &str, Vec<String>)| {
                // `max-blank-lines` drops a blank-only keep body at document end (#558).
                let blank_only = body.iter().all(|line| line.trim().is_empty());
                let chomp = if blank_only && chomp == "+" {
                    ""
                } else {
                    chomp
                };
                format!("{style}{indicator}{chomp}\n{}", body.join("\n"))
            },
        )
}

fn arb_entry() -> impl Strategy<Value = String> {
    prop_oneof![
        4 => (arb_value(), arb_comment()).prop_map(|(value, comment)| format!(": {value}{comment}")),
        1 => (arb_plain(), arb_plain(), prop_oneof![Just(2), Just(4)])
            .prop_map(|(first, next, indent)| {
                format!(": {first}\n{}{next}", " ".repeat(indent))
            }),
        1 => (arb_flow_safe(), arb_flow_safe(), prop_oneof![Just("'"), Just("\"")])
            .prop_map(|(first, next, quote)| format!(": {quote}{first}\n   {next}{quote}")),
        1 => (arb_value(), arb_value()).prop_map(|(a, b)| format!(":\n  nested: {a}\n  other: {b}")),
        1 => (arb_value(), arb_value()).prop_map(|(a, b)| format!(":\n- {a}\n- inner: {b}")),
        1 => (arb_flow_safe(), arb_flow_safe())
            .prop_map(|(a, b)| format!(": [{a}, '{b}']")),
        1 => arb_flow_safe().prop_map(|a| format!(": {{x: {a}}}")),
        2 => arb_block_scalar(2).prop_map(|body| format!(": {body}")),
    ]
}

fn arb_mapping() -> impl Strategy<Value = String> {
    prop::collection::vec(
        (
            prop_oneof![
                3 => Just(None),
                1 => (arb_flow_safe(), prop_oneof![Just(""), Just("'"), Just("\"")])
                    .prop_map(Some),
            ],
            arb_entry(),
        ),
        1..=4,
    )
    .prop_map(|entries| {
        entries
            .into_iter()
            .enumerate()
            .map(|(index, (key, entry))| match key {
                Some((key, quote)) => format!("{quote}{key} {index}{quote}{entry}\n"),
                None => format!("k{index}{entry}\n"),
            })
            .collect()
    })
}

fn arb_sequence() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            (arb_value(), arb_comment())
                .prop_map(|(value, comment)| format!("- {value}{comment}\n")),
            arb_value().prop_map(|value| format!("- key: {value}\n  other: x\n")),
            arb_value().prop_map(|value| format!("- - {value}\n")),
            (
                arb_flow_safe(),
                arb_flow_safe(),
                prop_oneof![Just("'"), Just("\"")],
                1..=3usize
            )
                .prop_map(|(first, next, quote, indent)| {
                    format!("- {quote}{first}\n{}{next}{quote}\n", " ".repeat(indent))
                }),
            arb_block_scalar(2).prop_map(|body| format!("- {body}\n")),
            (arb_block_scalar(4), arb_value())
                .prop_map(|(body, value)| format!("- key: {body}\n  other: {value}\n")),
            (arb_flow_safe(), arb_value())
                .prop_map(|(key, value)| format!("- ? {key}\n  : {value}\n")),
        ],
        1..=4,
    )
    .prop_map(|entries| entries.concat())
}

fn arb_root() -> impl Strategy<Value = String> {
    (
        prop_oneof![Just(""), Just("--- "), Just("%YAML 1.1\n--- ")],
        arb_value(),
    )
        .prop_map(|(prefix, value)| format!("{prefix}{value}\n"))
}

pub fn arb_fold_document() -> impl Strategy<Value = String> {
    (
        prop_oneof![3 => arb_mapping(), 2 => arb_sequence(), 1 => arb_root()],
        prop_oneof![Just("\n"), Just("\r\n"), Just("\r")],
    )
        .prop_map(|(text, newline)| text.replace('\n', newline))
}

fn line_lengths(text: &str) -> Vec<usize> {
    text.replace("\r\n", "\n")
        .split(['\n', '\r'])
        .map(|line| line.chars().count())
        .collect()
}

/// The `(byte offset, indent)` of each continuation line a fold inserted into `output`,
/// or why `output` is not `input` with lone spaces turned into indented line breaks that
/// shorten their lines.
pub fn inserted_breaks(
    input: &str,
    output: &str,
) -> Result<Vec<(usize, usize)>, String> {
    let lengths = line_lengths(input);
    let (mut line, mut piece) = (0, 0);
    let grew = |line: usize, piece: usize| {
        (piece > lengths[line])
            .then(|| format!("line {} grew to {piece} chars", line + 1))
    };
    let mut out = output.char_indices().peekable();
    let mut breaks = Vec::new();
    for (index, ch) in input.char_indices() {
        let Some(&(offset, got)) = out.peek() else {
            return Err(format!("output ends before input byte {index}"));
        };
        if got == ch {
            out.next();
            if !matches!(ch, '\n' | '\r') {
                piece += 1;
            } else if !input[index..].starts_with("\r\n") {
                grew(line, piece).map_or(Ok(()), Err)?;
                (line, piece) = (line + 1, 0);
            }
            continue;
        }
        let lone = ch == ' '
            && input[..index]
                .chars()
                .next_back()
                .is_some_and(|c| !c.is_whitespace())
            && input[index + 1..]
                .chars()
                .next()
                .is_some_and(|c| !c.is_whitespace());
        if !lone || !matches!(got, '\n' | '\r') {
            return Err(format!(
                "input byte {index} {ch:?} became {got:?} at output byte {offset}"
            ));
        }
        grew(line, piece).map_or(Ok(()), Err)?;
        while out.next_if(|(_, c)| matches!(c, '\n' | '\r')).is_some() {}
        piece = 0;
        while out.next_if(|(_, c)| *c == ' ').is_some() {
            piece += 1;
        }
        breaks.push((
            out.peek().map_or(output.len(), |(offset, _)| *offset),
            piece,
        ));
    }
    grew(line, piece).map_or(Ok(()), Err)?;
    match out.next() {
        Some((offset, _)) => Err(format!("output has extra text from byte {offset}")),
        None => Ok(breaks),
    }
}

/// The first inserted continuation (by byte offset in `output`) no deeper than the block
/// collection owning its scalar (column 0 at the root), or under a quoted scalar less
/// than two columns past it.
pub fn under_indented_break(output: &str, breaks: &[(usize, usize)]) -> Option<usize> {
    let mut blocks = Vec::new();
    let mut owners = Vec::new();
    for token in Scanner::new(StrInput::new(output)).map_while(Result::ok) {
        let (span, kind) = token.into_parts();
        match kind {
            TokenType::BlockMappingStart | TokenType::BlockSequenceStart => {
                blocks.push(span.start.col());
            }
            TokenType::BlockEnd => {
                blocks.pop();
            }
            TokenType::Scalar(style, _) => owners.push((
                span.start
                    .byte_offset()
                    .expect("str input has byte offsets")
                    ..span.end.byte_offset().expect("str input has byte offsets"),
                blocks.last().copied(),
                if style == ScalarStyle::Plain { 1 } else { 2 },
            )),
            _ => {}
        }
    }
    breaks.iter().find_map(|&(at, indent)| {
        let deep_enough = owners
            .iter()
            .find(|(span, ..)| span.contains(&at))
            .is_some_and(|(_, owner, step)| indent >= owner.unwrap_or(0) + step);
        (!deep_enough).then_some(at)
    })
}
