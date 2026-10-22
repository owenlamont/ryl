/// Options controlling parser and scanner behavior and resource usage.
///
/// Construct this type with [`crate::options!`] so that code remains compatible when new options
/// are added in future releases.
///
/// # Examples
///
/// ```rust
/// let options = ryl_yaml_parser::options! {
///     strict_indentation: true,
///     flow_nesting_limit: 512,
///     block_nesting_limit: 256,
/// };
///
/// assert!(options.strict_indentation);
/// assert_eq!(options.flow_nesting_limit, 512);
/// assert_eq!(options.block_nesting_limit, 256);
/// ```
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Options {
    /// Whether to enforce strict YAML indentation rules for flow collections, scalar
    /// continuations, and content following explicit-key anchor or tag properties.
    ///
    /// The default parsing is compatible with `PyYAML` and `ruamel.yaml`.
    /// `true` sets stricter indentation rules as required by the YAML specification,
    /// some non-compliant YAML that passes with the two libraries above will fail.
    /// Default is false.
    pub strict_indentation: bool,
    /// Whether scanners emit comment tokens and parsers emit comment events.
    ///
    /// The default is `true`. When this is `false`, comments are still recognized and validated as YAML syntax, but their
    /// text is not captured and no comment tokens or events are emitted.
    /// Comment bytes are still consumed, so this is not an input-size or processing-time limit.
    /// [`Self::max_buffered_comment_events`] has no effect while comment emission is disabled.
    pub emit_comments: bool,
    /// Maximum number of consecutive comment events buffered while resolving an ambiguous
    /// collection entry.
    ///
    /// The default is 96. A value of zero rejects the first comment that would need buffering.
    pub max_buffered_comment_events: usize,
    /// Maximum number of characters inspected while resolving a simple key.
    ///
    /// The default is 1024, matching YAML's simple-key length restriction. A key at exactly this
    /// limit is accepted. Lower values impose a stricter resource limit; higher values relax that
    /// YAML restriction.
    pub simple_key_max_lookahead: usize,
    /// Maximum number of simultaneously nested flow collections.
    ///
    /// The default is 255. A value of zero rejects the first flow collection opener.
    pub flow_nesting_limit: usize,
    /// Maximum number of simultaneously nested block collections parsed, or block indentation
    /// levels retained by a scanner used directly.
    ///
    /// The default is 255. A value of zero rejects the first block sequence or mapping. This
    /// bounds both indentation state retained while scanning and the number of closing tokens
    /// queued when nested block collections end together. Parsers also count indentless block
    /// sequences, which do not add scanner indentation state.
    pub block_nesting_limit: usize,
    /// Maximum number of UTF-8 source bytes retained for a directive name and payload.
    ///
    /// The default is 1024. Separating blanks before retained values count toward the limit.
    /// `%YAML` version components instead have a fixed nine-digit limit and are stored as integers.
    /// Real directives are far shorter than the default.
    pub max_directive_bytes: usize,
    /// Maximum number of parameters retained for a single reserved directive.
    ///
    /// The default is 16. The parser ignores reserved directives, so their parameters only reach
    /// code driving [`crate::Scanner`] directly. A value of zero rejects the first parameter.
    pub max_reserved_directive_params: usize,
}

impl Options {
    /// Controls whether scanners emit comment tokens and parsers emit comment events.
    ///
    /// When emission is disabled, comments are still recognized and validated, but their text
    /// is not captured. This sets [`Self::emit_comments`].
    pub fn set_emit_comments(&mut self, emit_comments: bool) {
        self.emit_comments = emit_comments;
    }
}

impl Default for Options {
    fn default() -> Self {
        Self {
            strict_indentation: false,
            emit_comments: true,
            max_buffered_comment_events: 96,
            simple_key_max_lookahead: 1024,
            flow_nesting_limit: 255,
            block_nesting_limit: 255,
            max_directive_bytes: 1024,
            max_reserved_directive_params: 16,
        }
    }
}
