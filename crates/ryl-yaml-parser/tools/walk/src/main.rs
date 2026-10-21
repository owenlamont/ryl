#![allow(unused_assignments)]

use std::ops::Range;

use granit_parser::{Event, Parser, ScanError, Span};
use miette::{bail, miette, Diagnostic, NamedSource, Result, SourceSpan};
use rustyline::{error::ReadlineError, DefaultEditor};
use thiserror::Error;

/// A REPL to navigate a YAML document from the spans emitted by `granit-parser`.
///
/// See [`read_action`] for commands.
fn main() {
    let args: Vec<_> = std::env::args().collect();
    match args.as_slice() {
        [_, filename] => {
            let contents = std::fs::read_to_string(filename).unwrap();
            let yaml = load_first_document(&contents).unwrap();
            walk(&contents, &yaml);
        }
        _ => {
            eprintln!("Usage: walk <file.yaml>");
        }
    }
}

fn walk(contents: &str, yaml: &WalkNode) {
    let mut stack = vec![];
    let mut io = DefaultEditor::new().unwrap();
    stack.push(yaml);

    print(contents, yaml);

    loop {
        let err = match read_action(&mut io) {
            Action::StepIn => step_in(&mut stack),
            Action::StepInKey => step_in_key(&mut stack),
            Action::StepInValue => step_in_value(&mut stack),
            Action::Next => next(&mut stack),
            Action::Prev => prev(&mut stack),
            Action::Fin => fin(&mut stack),
            Action::Stop => break,
        };

        match err {
            Ok(()) => {
                io.clear_screen().unwrap();
                print(contents, stack.last().unwrap());
            }
            Err(e) => eprintln!("{e}"),
        }
    }
}

fn print(contents: &str, yaml: &WalkNode) {
    let range = source_range(contents, yaml.span);
    eprintln!(
        "{:?}",
        miette::Error::new(FakeErr {
            src: NamedSource::new("<input>", contents.to_owned()),
            span: range.into(),
        })
    );
}

fn load_first_document(contents: &str) -> Result<WalkNode> {
    let mut parser = Parser::new_from_str(contents);

    while let Some(event) = parser.next() {
        let (event, span) = event.map_err(|err| miette!("{err}"))?;
        match event {
            Event::StreamStart | Event::Comment(..) => {}
            Event::DocumentStart(..) => return load_document_node(&mut parser, span),
            Event::StreamEnd => bail!("No YAML document found"),
            event => return build_node(&event, span, &mut parser),
        }
    }

    bail!("No YAML document found")
}

fn load_document_node<'input>(
    parser: &mut impl Iterator<Item = std::result::Result<(Event<'input>, Span), ScanError>>,
    document_span: Span,
) -> Result<WalkNode> {
    while let Some(event) = parser.next() {
        let (event, span) = event.map_err(|err| miette!("{err}"))?;
        match event {
            Event::DocumentEnd | Event::StreamEnd => {
                return Ok(WalkNode {
                    span: Span::empty(document_span.end),
                    data: WalkData::Scalar,
                });
            }
            Event::StreamStart | Event::DocumentStart(..) | Event::Comment(..) => {}
            event => return build_node(&event, span, parser),
        }
    }

    bail!("Document ended before a node was emitted")
}

fn build_node<'input>(
    event: &Event<'input>,
    span: Span,
    parser: &mut impl Iterator<Item = std::result::Result<(Event<'input>, Span), ScanError>>,
) -> Result<WalkNode> {
    match event {
        Event::Scalar(..) | Event::Alias(..) => Ok(WalkNode {
            span,
            data: WalkData::Scalar,
        }),
        Event::SequenceStart(..) => build_sequence(span, parser),
        Event::MappingStart(..) => build_mapping(span, parser),
        Event::Comment(..) => bail!("Unexpected comment while building node"),
        Event::StreamStart
        | Event::StreamEnd
        | Event::DocumentStart(..)
        | Event::DocumentEnd
        | Event::SequenceEnd
        | Event::MappingEnd => bail!("Unexpected event while building node: {event:?}"),
        _ => bail!("Unsupported event while building node: {event:?}"),
    }
}

fn build_sequence<'input>(
    start_span: Span,
    parser: &mut impl Iterator<Item = std::result::Result<(Event<'input>, Span), ScanError>>,
) -> Result<WalkNode> {
    let mut items = Vec::new();

    loop {
        let (event, span) = next_data_event(parser)?;
        if matches!(event, Event::SequenceEnd) {
            return Ok(WalkNode {
                span: span_from_bounds(start_span, span),
                data: WalkData::Sequence(items),
            });
        }
        if matches!(event, Event::DocumentEnd | Event::StreamEnd) {
            bail!("Sequence ended before SequenceEnd was emitted");
        }
        items.push(build_node(&event, span, parser)?);
    }
}

fn build_mapping<'input>(
    start_span: Span,
    parser: &mut impl Iterator<Item = std::result::Result<(Event<'input>, Span), ScanError>>,
) -> Result<WalkNode> {
    let mut items = Vec::new();

    loop {
        let (event, span) = next_data_event(parser)?;
        if matches!(event, Event::MappingEnd) {
            return Ok(WalkNode {
                span: span_from_bounds(start_span, span),
                data: WalkData::Mapping(items),
            });
        }
        if matches!(event, Event::DocumentEnd | Event::StreamEnd) {
            bail!("Mapping ended before MappingEnd was emitted");
        }

        let key = build_node(&event, span, parser)?;
        let (event, span) = next_data_event(parser)?;
        if matches!(
            event,
            Event::MappingEnd | Event::DocumentEnd | Event::StreamEnd
        ) {
            bail!("Mapping key was not followed by a value");
        }
        let value = build_node(&event, span, parser)?;
        items.push((key, value));
    }
}

fn next_data_event<'input>(
    parser: &mut impl Iterator<Item = std::result::Result<(Event<'input>, Span), ScanError>>,
) -> Result<(Event<'input>, Span)> {
    loop {
        let (event, span) = next_event(parser)?;
        if matches!(event, Event::Comment(..)) {
            continue;
        }
        return Ok((event, span));
    }
}

fn next_event<'input>(
    parser: &mut impl Iterator<Item = std::result::Result<(Event<'input>, Span), ScanError>>,
) -> Result<(Event<'input>, Span)> {
    match parser.next() {
        Some(Ok(event)) => Ok(event),
        Some(Err(err)) => Err(miette!("{err}")),
        None => bail!("Unexpected end of parser event stream"),
    }
}

fn span_from_bounds(start: Span, end: Span) -> Span {
    Span::new(start.start, end.end)
}

fn source_range(contents: &str, span: Span) -> Range<usize> {
    span.byte_range().unwrap_or_else(|| {
        char_to_byte_index(contents, span.start.index())
            ..char_to_byte_index(contents, span.end.index())
    })
}

fn char_to_byte_index(contents: &str, char_index: usize) -> usize {
    contents
        .char_indices()
        .nth(char_index)
        .map_or(contents.len(), |(byte_index, _)| byte_index)
}

fn step_in(stack: &mut Stack<'_>) -> Result<()> {
    match &stack.last().unwrap().data {
        WalkData::Sequence(seq) => do_step_in_seq(stack, seq)?,
        WalkData::Mapping(map) => do_step_in_value(stack, map)?,
        WalkData::Scalar => bail!("Not in a mapping or a sequence"),
    }
    Ok(())
}

fn step_in_key(stack: &mut Stack<'_>) -> Result<()> {
    match &stack.last().unwrap().data {
        WalkData::Mapping(map) => do_step_in_key(stack, map),
        _ => bail!("Not in a mapping"),
    }
}

fn step_in_value(stack: &mut Stack<'_>) -> Result<()> {
    match &stack.last().unwrap().data {
        WalkData::Mapping(map) => do_step_in_value(stack, map),
        _ => bail!("Not in a mapping"),
    }
}

fn next(stack: &mut Stack<'_>) -> Result<()> {
    if stack.len() == 1 {
        bail!("Can't next from top-level");
    }
    let node = stack.pop().unwrap();
    let parent = stack.last().unwrap();
    let mut pos = pos_in_parent(node, parent);
    pos.idx += 1;

    match &parent.data {
        WalkData::Sequence(seq) => {
            if pos.idx == seq.len() {
                bail!("Reached end of the sequence");
            } else {
                stack.push(&seq[pos.idx]);
            }
        }
        WalkData::Mapping(map) => {
            if pos.idx == map.len() {
                bail!("Reached end of the map");
            } else {
                let (key, value) = &map[pos.idx];
                if pos.kvtype == KVType::Key {
                    stack.push(key);
                } else {
                    stack.push(value);
                }
            }
        }
        WalkData::Scalar => unreachable!(),
    }
    Ok(())
}

fn prev(stack: &mut Stack<'_>) -> Result<()> {
    if stack.len() == 1 {
        bail!("Can't prev from top-level");
    }
    let node = stack.pop().unwrap();
    let parent = stack.last().unwrap();
    let mut pos = pos_in_parent(node, parent);
    if pos.idx == 0 {
        bail!("Already at the beginning of the collection");
    }
    pos.idx -= 1;

    match &parent.data {
        WalkData::Sequence(seq) => {
            stack.push(&seq[pos.idx]);
        }
        WalkData::Mapping(map) => {
            let (key, value) = &map[pos.idx];
            if pos.kvtype == KVType::Key {
                stack.push(key);
            } else {
                stack.push(value);
            }
        }
        WalkData::Scalar => unreachable!(),
    }
    Ok(())
}

fn fin(stack: &mut Stack<'_>) -> Result<()> {
    if stack.len() > 1 {
        stack.pop();
        Ok(())
    } else {
        bail!("Already at the top-level");
    }
}

fn do_step_in_seq<'a>(stack: &mut Stack<'a>, seq: &'a YamlSeq) -> Result<()> {
    if seq.is_empty() {
        bail!("Sequence is empty");
    } else {
        stack.push(&seq[0]);
        Ok(())
    }
}

fn do_step_in_key<'a>(stack: &mut Stack<'a>, map: &'a YamlMap) -> Result<()> {
    if let Some((key, _)) = map.first() {
        stack.push(key);
        Ok(())
    } else {
        bail!("Mapping is empty");
    }
}

fn do_step_in_value<'a>(stack: &mut Stack<'a>, map: &'a YamlMap) -> Result<()> {
    if let Some((_, value)) = map.first() {
        stack.push(value);
        Ok(())
    } else {
        bail!("Mapping is empty");
    }
}

type Stack<'a> = Vec<&'a WalkNode>;
type YamlMap = Vec<(WalkNode, WalkNode)>;
type YamlSeq = Vec<WalkNode>;

struct WalkNode {
    span: Span,
    data: WalkData,
}

enum WalkData {
    Scalar,
    Sequence(YamlSeq),
    Mapping(YamlMap),
}

#[derive(Error, Debug, Diagnostic)]
#[error("")]
#[diagnostic()]
pub struct FakeErr {
    #[source_code]
    src: NamedSource<String>,
    #[label("Current node")]
    span: SourceSpan,
}

struct PositionInParent {
    idx: usize,
    kvtype: KVType,
}

#[derive(Eq, PartialEq)]
enum KVType {
    Key,
    Value,
}

fn pos_in_parent<'a>(node: &'a WalkNode, parent: &'a WalkNode) -> PositionInParent {
    let mut pos = PositionInParent {
        idx: 0,
        kvtype: KVType::Key,
    };
    match &parent.data {
        WalkData::Sequence(seq) => {
            for (idx, sibling) in seq.iter().enumerate() {
                if core::ptr::eq(sibling, node) {
                    pos.idx = idx;
                    return pos;
                }
            }
            unreachable!();
        }
        WalkData::Mapping(map) => {
            for (idx, (key, value)) in map.iter().enumerate() {
                pos.idx = idx;
                if core::ptr::eq(key, node) {
                    return pos;
                } else if core::ptr::eq(value, node) {
                    pos.kvtype = KVType::Value;
                    return pos;
                }
            }
            unreachable!();
        }
        WalkData::Scalar => unreachable!(),
    }
}

enum Action {
    StepIn,
    StepInKey,
    StepInValue,
    Next,
    Prev,
    Fin,
    Stop,
}

fn read_action(io: &mut DefaultEditor) -> Action {
    loop {
        match io.readline(">> ") {
            Ok(line) => match line.as_str() {
                "q" | "quit" => return Action::Stop,
                "n" | "next" => return Action::Next,
                "p" | "prev" => return Action::Prev,
                "s" | "si" | "i" => return Action::StepIn,
                "sk" => return Action::StepInKey,
                "sv" => return Action::StepInValue,
                "fin" | "out" | "up" => return Action::Fin,
                _ => {}
            },
            Err(ReadlineError::Interrupted | ReadlineError::Eof) => return Action::Stop,
            Err(e) => panic!("{e:?}"),
        }
    }
}
