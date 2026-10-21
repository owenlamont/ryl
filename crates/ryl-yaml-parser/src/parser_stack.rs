use crate::{
    error::{ErrorKind, ScanError},
    input::{str::StrInput, BorrowedInput, BufferedInput},
    parser::{Event, ParseResult, Parser, ParserTrait, SpannedEventReceiver},
    scanner::Span,
    Options,
};
use alloc::{borrow::Cow, boxed::Box, string::String, vec::Vec};

/// A lightweight parser that replays a pre-collected event stream.
pub struct ReplayParser<'input> {
    events: alloc::vec::IntoIter<(Event<'input>, Span)>,
    anchor_offset: usize,
}

impl<'input> ReplayParser<'input> {
    /// Create a parser that replays `events` and starts anchor allocation at `anchor_offset`.
    #[must_use]
    pub fn new(events: Vec<(Event<'input>, Span)>, anchor_offset: usize) -> Self {
        Self {
            events: events.into_iter(),
            anchor_offset,
        }
    }

    /// Return the next anchor ID that should be assigned after replayed events.
    #[must_use]
    pub fn anchor_offset(&self) -> usize {
        self.anchor_offset
    }

    /// Set the next anchor ID that should be assigned after replayed events.
    pub fn set_anchor_offset(&mut self, offset: usize) {
        self.anchor_offset = offset;
    }

    fn advance_anchor_offset(&mut self, event: &Event<'input>) {
        let anchor_id = match event {
            Event::Scalar(_, _, anchor_id, _)
            | Event::SequenceStart(_, anchor_id, _)
            | Event::MappingStart(_, anchor_id, _) => *anchor_id,
            _ => 0,
        };

        if anchor_id > 0 {
            self.anchor_offset = self.anchor_offset.max(anchor_id.saturating_add(1));
        }
    }
}

impl<'input> ParserTrait<'input> for ReplayParser<'input> {
    fn peek(&mut self) -> Option<Result<&(Event<'input>, Span), ScanError>> {
        self.events.as_slice().first().map(Ok)
    }

    fn next_event(&mut self) -> Option<ParseResult<'input>> {
        let event = self.events.next()?;
        self.advance_anchor_offset(&event.0);
        Some(Ok(event))
    }

    fn load<R: SpannedEventReceiver<'input>>(
        &mut self,
        recv: &mut R,
        multi: bool,
    ) -> Result<(), ScanError> {
        while let Some(res) = self.next_event() {
            let (ev, span) = res?;
            let is_doc_end = matches!(ev, Event::DocumentEnd);
            let is_stream_end = matches!(ev, Event::StreamEnd);
            recv.on_event(ev, span);
            if is_stream_end {
                break;
            }
            if !multi && is_doc_end {
                break;
            }
        }
        Ok(())
    }
}

impl<'input> Iterator for ReplayParser<'input> {
    type Item = ParseResult<'input>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_event()
    }
}

impl core::iter::FusedIterator for ReplayParser<'_> {}

/// A wrapper for different types of parsers.
enum AnyParser<'input, I, T>
where
    I: Iterator<Item = char>,
    T: BorrowedInput<'input>,
{
    /// A parser over borrowed string input.
    String {
        /// Parser currently producing events for this stack entry.
        parser: Parser<'input, StrInput<'input>>,
        /// Human-readable source name returned by [`ParserStack::stack`].
        name: String,
    },
    /// A parser over an iterator of characters.
    Iter {
        /// Parser currently producing events for this stack entry.
        parser: Parser<'static, BufferedInput<I>>,
        /// Human-readable source name returned by [`ParserStack::stack`].
        name: String,
    },
    /// A parser over a custom input.
    Custom {
        /// Parser currently producing events for this stack entry.
        parser: Parser<'input, T>,
        /// Human-readable source name returned by [`ParserStack::stack`].
        name: String,
    },
    /// A parser over a replayed event stream.
    Replay {
        /// Replay parser currently producing pre-collected events for this stack entry.
        parser: ReplayParser<'input>,
        /// Human-readable source name returned by [`ParserStack::stack`].
        name: String,
    },
}

impl<'input, I, T> AnyParser<'input, I, T>
where
    I: Iterator<Item = char>,
    T: BorrowedInput<'input>,
{
    fn anchor_offset(&self) -> usize {
        match self {
            AnyParser::String { parser, .. } => parser.anchor_offset(),
            AnyParser::Iter { parser, .. } => parser.anchor_offset(),
            AnyParser::Custom { parser, .. } => parser.anchor_offset(),
            AnyParser::Replay { parser, .. } => parser.anchor_offset(),
        }
    }

    fn set_anchor_offset(&mut self, offset: usize) {
        match self {
            AnyParser::String { parser, .. } => parser.set_anchor_offset(offset),
            AnyParser::Iter { parser, .. } => parser.set_anchor_offset(offset),
            AnyParser::Custom { parser, .. } => parser.set_anchor_offset(offset),
            AnyParser::Replay { parser, .. } => parser.set_anchor_offset(offset),
        }
    }
}

/// A parser implementation that uses a stack for include-style parsing.
///
/// Note: `ParserStack` deliberately suppresses nested [`Event::StreamStart`] /
/// [`Event::DocumentStart`] events when more than one parser is stacked, and the tests assert
/// outputs where a nested parser starts directly with [`Event::MappingStart`] before the parent
/// stream/document wrapper appears.
///
/// That is exactly what we want for `!include`-style subtree injection.
///
/// Included parser events are replayed through the same event stream as parent events. With the
/// `comments` Cargo feature, this includes comment events unless emission is disabled through
/// [`ParserStack::with_options`]. Without the feature, comment events and their continuation state
/// are absent.
/// Included [`Span`] values remain local to the included source, just like every other event span
/// from an included parser. `ParserStack` does not attach file names, source IDs, or other include
/// provenance to events or spans. Errors do retain the nested source names through
/// [`ScanError::source_stack`].
pub struct ParserStack<'input, I = core::iter::Empty<char>, T = StrInput<'input>>
where
    I: Iterator<Item = char>,
    T: BorrowedInput<'input>,
{
    options: Options,
    parsers: Vec<AnyParser<'input, I, T>>,
    /// Stack depths and end spans of nested documents whose trailing comments are being read.
    #[cfg(feature = "comments")]
    pending_document_ends: Vec<(usize, Span)>,
    current: Option<(Event<'input>, Span)>,
    current_error: Option<ScanError>,
    stream_end_emitted: bool,
    #[allow(clippy::type_complexity)]
    include_resolver: Option<Box<dyn FnMut(&str) -> Result<Cow<'input, str>, ScanError> + 'input>>,
}

impl<'input, I, T> ParserStack<'input, I, T>
where
    I: Iterator<Item = char>,
    T: BorrowedInput<'input>,
{
    /// Creates a new, empty parser stack.
    #[must_use]
    pub fn new() -> Self {
        Self::with_options(Options::default())
    }

    /// Creates a new, empty parser stack with the supplied parsing options.
    ///
    /// Options are applied to sources parsed internally by [`Self::push_include`]. Caller-supplied
    /// parsers retain their own scanning options.
    /// With the `comments` Cargo feature, disabling comment emission also suppresses comment
    /// events from parsers and replay streams supplied by the caller. Construct caller-supplied
    /// parsers with emission disabled as well to avoid capturing comments before filtering them.
    /// Without the feature, comment events are absent and [`Options::emit_comments`] has no effect.
    #[must_use]
    pub fn with_options(options: Options) -> Self {
        Self {
            options,
            parsers: Vec::new(),
            #[cfg(feature = "comments")]
            pending_document_ends: Vec::new(),
            current: None,
            current_error: None,
            stream_end_emitted: false,
            include_resolver: None,
        }
    }

    /// Set the resolver used by [`Self::push_include`].
    ///
    /// The resolver receives the include name and returns the included YAML source text.
    pub fn set_resolver(
        &mut self,
        mut resolver: impl FnMut(&str) -> Result<String, ScanError> + 'input,
    ) {
        self.include_resolver = Some(Box::new(move |name| resolver(name).map(Cow::Owned)));
    }

    /// Set an include resolver whose source text can be borrowed for the stack's input lifetime.
    ///
    /// Unlike [`Self::set_resolver`], this path lets scalar, anchor, and tag token text in included
    /// documents borrow directly from the returned source, as well as comment text when the
    /// `comments` Cargo feature is enabled. The included document is still validated eagerly so
    /// resolution errors retain the same timing and source-stack context.
    pub fn set_borrowed_resolver(
        &mut self,
        mut resolver: impl FnMut(&str) -> Result<&'input str, ScanError> + 'input,
    ) {
        self.include_resolver = Some(Box::new(move |name| resolver(name).map(Cow::Borrowed)));
    }

    /// Resolve an include by name and push the resulting parser onto the stack.
    ///
    /// With the `comments` Cargo feature, comment events from the included content follow the
    /// setting passed to [`Self::with_options`]. All event spans are local to the included content
    /// returned by the resolver.
    ///
    /// # Errors
    /// Returns `ScanError` if no resolver is configured, include resolution fails, or the
    /// included content cannot be parsed.
    pub fn push_include(&mut self, include_str: &str) -> Result<(), ScanError> {
        let resolved = match &mut self.include_resolver {
            Some(resolver) => resolver(include_str),
            None => {
                return Err(self.contextualize_include_error(
                    ScanError::from_kind(
                        crate::scanner::Marker::new(0, 1, 0),
                        ErrorKind::MissingIncludeResolver,
                    ),
                    include_str,
                ));
            }
        };
        let content = match resolved {
            Ok(content) => content,
            Err(error) => return Err(self.contextualize_include_error(error, include_str)),
        };
        let inherited_anchor_offset = self.parsers.last().map(AnyParser::anchor_offset);

        let (events, next_anchor_offset) = match content {
            Cow::Borrowed(content) => {
                let mut parser = Parser::new_from_str_with_options(content, self.options.clone());
                if let Some(anchor_offset) = inherited_anchor_offset {
                    parser.set_anchor_offset(anchor_offset);
                }
                let mut events = Vec::new();
                while let Some(event) = parser.next_event() {
                    match event {
                        Ok(event) => events.push(event),
                        Err(error) => {
                            return Err(self.contextualize_include_error(error, include_str));
                        }
                    }
                }
                (events, parser.anchor_offset())
            }
            Cow::Owned(content) => {
                let mut parser =
                    Parser::new_from_iter_with_options(content.chars(), self.options.clone());
                if let Some(anchor_offset) = inherited_anchor_offset {
                    parser.set_anchor_offset(anchor_offset);
                }
                let mut events = Vec::new();
                while let Some(event) = parser.next_event() {
                    match event {
                        Ok(event) => events.push(event),
                        Err(error) => {
                            return Err(self.contextualize_include_error(error, include_str));
                        }
                    }
                }
                (events, parser.anchor_offset())
            }
        };

        self.push_replay_parser(
            ReplayParser::new(events, next_anchor_offset),
            include_str.into(),
        );
        Ok(())
    }

    fn contextualize_include_error(&self, error: ScanError, include_str: &str) -> ScanError {
        let mut source_stack = self.stack();
        source_stack.push(include_str.into());
        error.with_source_stack(source_stack)
    }

    fn prepare_for_push(&mut self) {
        if matches!(self.current.as_ref(), Some((Event::StreamEnd, _))) {
            self.current = None;
        }
    }

    /// Push a string parser onto the stack.
    ///
    /// The pushed parser inherits the current anchor offset so anchors remain unique across stacked
    /// sources. `name` is returned by [`Self::stack`] for diagnostics.
    pub fn push_str_parser(&mut self, mut parser: Parser<'input, StrInput<'input>>, name: String) {
        self.prepare_for_push();
        if let Some(parent) = self.parsers.last() {
            parser.set_anchor_offset(parent.anchor_offset());
        }
        self.parsers.push(AnyParser::String { parser, name });
    }

    /// Push an iterator-backed parser onto the stack.
    ///
    /// The pushed parser inherits the current anchor offset so anchors remain unique across stacked
    /// sources. `name` is returned by [`Self::stack`] for diagnostics.
    pub fn push_iter_parser(
        &mut self,
        mut parser: Parser<'static, BufferedInput<I>>,
        name: String,
    ) {
        self.prepare_for_push();
        if let Some(parent) = self.parsers.last() {
            parser.set_anchor_offset(parent.anchor_offset());
        }
        self.parsers.push(AnyParser::Iter { parser, name });
    }

    /// Push a custom-input parser onto the stack.
    ///
    /// The pushed parser inherits the current anchor offset so anchors remain unique across stacked
    /// sources. `name` is returned by [`Self::stack`] for diagnostics.
    pub fn push_custom_parser(&mut self, mut parser: Parser<'input, T>, name: String) {
        self.prepare_for_push();
        if let Some(parent) = self.parsers.last() {
            parser.set_anchor_offset(parent.anchor_offset());
        }
        self.parsers.push(AnyParser::Custom { parser, name });
    }

    /// Push a replay parser onto the stack.
    ///
    /// Replay parsers are used for included content that has already been parsed into events.
    /// `name` is returned by [`Self::stack`] for diagnostics.
    pub fn push_replay_parser(&mut self, mut parser: ReplayParser<'input>, name: String) {
        self.prepare_for_push();
        if let Some(parent) = self.parsers.last() {
            let inherited = parent.anchor_offset();
            parser.set_anchor_offset(parser.anchor_offset().max(inherited));
        }

        self.parsers.push(AnyParser::Replay { parser, name });
    }

    /// Push a custom parser and set the first event that should be returned from it.
    ///
    /// This is used when the caller has already consumed the parser's first event before deciding
    /// to place it on the stack.
    pub fn push_custom_parser_with_current(
        &mut self,
        mut parser: Parser<'input, T>,
        name: String,
        current: (Event<'input>, Span),
    ) {
        self.prepare_for_push();
        if let Some(parent) = self.parsers.last() {
            parser.set_anchor_offset(parent.anchor_offset());
        }
        self.parsers.push(AnyParser::Custom { parser, name });
        #[cfg(feature = "comments")]
        {
            self.current = if self.options.emit_comments || !matches!(current.0, Event::Comment(..))
            {
                Some(current)
            } else {
                None
            };
        }
        #[cfg(not(feature = "comments"))]
        {
            self.current = Some(current);
        }
    }

    /// Return the anchor offset that a newly pushed parser should inherit.
    #[must_use]
    pub fn current_anchor_offset(&self) -> usize {
        self.parsers.last().map_or(0, AnyParser::anchor_offset)
    }

    /// Return the names of the parsers currently in the stack, from bottom to top.
    #[must_use]
    pub fn stack(&self) -> Vec<String> {
        self.parsers
            .iter()
            .map(|p| match p {
                AnyParser::String { name, .. }
                | AnyParser::Iter { name, .. }
                | AnyParser::Custom { name, .. }
                | AnyParser::Replay { name, .. } => name.clone(),
            })
            .collect()
    }

    fn contextualize_error(&self, error: ScanError) -> ScanError {
        if self.parsers.len() > 1 {
            error.with_source_stack(self.stack())
        } else {
            error
        }
    }

    fn propagate_anchor_offset_from_popped(&mut self, popped: &AnyParser<'input, I, T>) {
        if let Some(parent) = self.parsers.last_mut() {
            let next_offset = parent.anchor_offset().max(popped.anchor_offset());
            parent.set_anchor_offset(next_offset);
        }
    }

    /// Pop the current parser and propagate its anchor offset to its parent.
    ///
    /// # Panics
    /// Panics if the parser stack is empty.
    #[track_caller]
    fn pop_parser_and_propagate_anchor_offset(&mut self) {
        #[cfg(feature = "comments")]
        if self
            .pending_document_ends
            .last()
            .is_some_and(|(depth, _)| *depth == self.parsers.len())
        {
            self.pending_document_ends.pop();
        }
        let popped = self.parsers.pop().unwrap();
        self.propagate_anchor_offset_from_popped(&popped);
    }

    fn next_event_impl(&mut self) -> Result<(Event<'input>, Span), ScanError> {
        loop {
            let Some(any_parser) = self.parsers.last_mut() else {
                return Ok((
                    Event::StreamEnd,
                    Span::empty(crate::scanner::Marker::new(0, 1, 0)),
                ));
            };

            let res = match any_parser {
                AnyParser::String { parser, .. } => parser.next_event(),
                AnyParser::Iter { parser, .. } => parser.next_event(),
                AnyParser::Custom { parser, .. } => parser.next_event(),
                AnyParser::Replay { parser, .. } => parser.next_event(),
            };

            #[cfg(feature = "comments")]
            if let Some(&(depth, span)) = self.pending_document_ends.last() {
                if depth == self.parsers.len()
                    && matches!(&res, Some(Ok((event, _)))
                        if !matches!(event, Event::Comment(..) | Event::StreamEnd))
                {
                    let error = self.contextualize_error(ScanError::from_kind(
                        span.start,
                        ErrorKind::MultipleDocumentsUnsupported,
                    ));
                    self.pop_parser_and_propagate_anchor_offset();
                    return Err(error);
                }
            }

            match res {
                Some(Ok((Event::StreamEnd, span))) => {
                    if self.parsers.len() == 1 {
                        self.parsers.pop();
                        return Ok((Event::StreamEnd, span));
                    }
                    self.pop_parser_and_propagate_anchor_offset();
                }
                None => {
                    if self.parsers.len() == 1 {
                        self.parsers.pop();
                        return Ok((
                            Event::StreamEnd,
                            Span::empty(crate::scanner::Marker::new(0, 1, 0)),
                        ));
                    }
                    self.pop_parser_and_propagate_anchor_offset();
                }
                Some(Err(e)) => {
                    let e = self.contextualize_error(e);
                    self.pop_parser_and_propagate_anchor_offset();
                    return e.into_result();
                }
                Some(Ok((Event::DocumentEnd, span))) => {
                    if self.parsers.len() == 1 {
                        return Ok((Event::DocumentEnd, span));
                    }

                    #[cfg(feature = "comments")]
                    {
                        // Retain the end span while emitting trailing comments across calls and
                        // pushes of other parsers.
                        self.pending_document_ends.push((self.parsers.len(), span));
                    }
                    #[cfg(not(feature = "comments"))]
                    {
                        // Without comment events, completion can be checked immediately without
                        // retaining a pending end span or allocating continuation state.
                        let next = match self.parsers.last_mut().unwrap() {
                            AnyParser::String { parser, .. } => parser.next_event(),
                            AnyParser::Iter { parser, .. } => parser.next_event(),
                            AnyParser::Custom { parser, .. } => parser.next_event(),
                            AnyParser::Replay { parser, .. } => parser.next_event(),
                        };
                        match next {
                            Some(Ok((Event::StreamEnd, _))) | None => {
                                self.pop_parser_and_propagate_anchor_offset();
                            }
                            Some(Err(error)) => {
                                let error = self.contextualize_error(error);
                                self.pop_parser_and_propagate_anchor_offset();
                                return Err(error);
                            }
                            Some(Ok(_)) => {
                                let error = self.contextualize_error(ScanError::from_kind(
                                    span.start,
                                    ErrorKind::MultipleDocumentsUnsupported,
                                ));
                                self.pop_parser_and_propagate_anchor_offset();
                                return Err(error);
                            }
                        }
                    }
                }
                Some(Ok(event)) => {
                    #[cfg(feature = "comments")]
                    if !self.options.emit_comments && matches!(event.0, Event::Comment(..)) {
                        continue;
                    }
                    if self.parsers.len() > 1
                        && matches!(event.0, Event::StreamStart | Event::DocumentStart(..))
                    {
                        continue;
                    }
                    return Ok(event);
                }
            }
        }
    }
}

impl<'input, I, T> Default for ParserStack<'input, I, T>
where
    I: Iterator<Item = char>,
    T: BorrowedInput<'input>,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<'input, I, T> ParserTrait<'input> for ParserStack<'input, I, T>
where
    I: Iterator<Item = char>,
    T: BorrowedInput<'input>,
{
    fn peek(&mut self) -> Option<Result<&(Event<'input>, Span), ScanError>> {
        if let Some(ref x) = self.current {
            Some(Ok(x))
        } else if let Some(error) = &self.current_error {
            Some(Err(error.clone()))
        } else {
            if self.stream_end_emitted {
                return None;
            }
            match self.next_event_impl() {
                Ok(token) => {
                    self.current = Some(token);
                    Some(Ok(self.current.as_ref().unwrap()))
                }
                Err(e) => {
                    self.current_error = Some(e.clone());
                    Some(Err(e))
                }
            }
        }
    }

    fn next_event(&mut self) -> Option<ParseResult<'input>> {
        if let Some(error) = self.current_error.take() {
            self.stream_end_emitted = true;
            return Some(Err(error));
        }

        if let Some(token) = self.current.take() {
            if let Event::StreamEnd = token.0 {
                self.stream_end_emitted = true;
            }
            return Some(Ok(token));
        }
        if self.stream_end_emitted {
            return None;
        }
        match self.next_event_impl() {
            Ok(token) => {
                if let Event::StreamEnd = token.0 {
                    self.stream_end_emitted = true;
                }
                Some(Ok(token))
            }
            Err(e) => {
                self.stream_end_emitted = true;
                Some(Err(e))
            }
        }
    }

    fn load<R: SpannedEventReceiver<'input>>(
        &mut self,
        recv: &mut R,
        multi: bool,
    ) -> Result<(), ScanError> {
        while let Some(res) = self.next_event() {
            // Fetch the next event from the active stack entry.
            let (ev, span) = res?;

            // Track whether to stop based on `multi`.
            let is_doc_end = matches!(ev, Event::DocumentEnd);
            let is_stream_end = matches!(ev, Event::StreamEnd);

            recv.on_event(ev, span);

            if is_stream_end {
                break;
            }

            // Stop after one document when multi-document parsing is disabled.
            if !multi && is_doc_end {
                break;
            }
        }

        Ok(())
    }
}

impl<'input, I, T> Iterator for ParserStack<'input, I, T>
where
    I: Iterator<Item = char>,
    T: BorrowedInput<'input>,
{
    type Item = Result<(Event<'input>, Span), ScanError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_event()
    }
}

impl<'input, I, T> core::iter::FusedIterator for ParserStack<'input, I, T>
where
    I: Iterator<Item = char>,
    T: BorrowedInput<'input>,
{
}
