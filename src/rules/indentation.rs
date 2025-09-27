use crate::config::YamlLintConfig;

pub const ID: &str = "indentation";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    spaces: SpacesSetting,
    indent_sequences: IndentSequencesSetting,
    check_multi_line_strings: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpacesSetting {
    Fixed(usize),
    Consistent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentSequencesSetting {
    True,
    False,
    Whatever,
    Consistent,
}

impl Config {
    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        let spaces = cfg
            .rule_option(ID, "spaces")
            .map_or(SpacesSetting::Consistent, |node| {
                node.as_integer()
                    .map_or(SpacesSetting::Consistent, |value| {
                        let non_negative = value.max(0);
                        let fixed = usize::try_from(non_negative).unwrap_or(usize::MAX);
                        SpacesSetting::Fixed(fixed)
                    })
            });

        let indent_sequences =
            cfg.rule_option(ID, "indent-sequences")
                .map_or(IndentSequencesSetting::True, |node| {
                    node.as_bool().map_or_else(
                        || match node.as_str() {
                            Some("whatever") => IndentSequencesSetting::Whatever,
                            Some("consistent") => IndentSequencesSetting::Consistent,
                            _ => IndentSequencesSetting::True,
                        },
                        |value| {
                            if value {
                                IndentSequencesSetting::True
                            } else {
                                IndentSequencesSetting::False
                            }
                        },
                    )
                });

        let check_multi_line_strings = cfg
            .rule_option(ID, "check-multi-line-strings")
            .and_then(saphyr::YamlOwned::as_bool)
            .unwrap_or(false);

        Self {
            spaces,
            indent_sequences,
            check_multi_line_strings,
        }
    }

    #[must_use]
    pub const fn new_for_tests(
        spaces: SpacesSetting,
        indent_sequences: IndentSequencesSetting,
        check_multi_line_strings: bool,
    ) -> Self {
        Self {
            spaces,
            indent_sequences,
            check_multi_line_strings,
        }
    }
}

#[must_use]
pub fn check(buffer: &str, cfg: &Config) -> Vec<Violation> {
    let mut analyzer = Analyzer::new(buffer, cfg);
    analyzer.run();
    analyzer.diagnostics
}

struct Analyzer<'a> {
    cfg: &'a Config,
    lines: Vec<&'a str>,
    contexts: Vec<Context>,
    spaces: SpacesRuntime,
    indent_seq: IndentSequencesRuntime,
    pending_child: Option<ContextKind>,
    multiline: Option<MultilineState>,
    diagnostics: Vec<Violation>,
}

impl<'a> Analyzer<'a> {
    fn new(text: &'a str, cfg: &'a Config) -> Self {
        let lines: Vec<&str> = text.split_inclusive(['\n']).collect();
        Self {
            cfg,
            lines,
            contexts: vec![Context {
                indent: 0,
                kind: ContextKind::Root,
            }],
            spaces: SpacesRuntime::new(cfg.spaces),
            indent_seq: IndentSequencesRuntime::new(cfg.indent_sequences),
            pending_child: None,
            multiline: None,
            diagnostics: Vec::new(),
        }
    }

    fn run(&mut self) {
        for line_index in 0..self.lines.len() {
            let line_number = line_index + 1;
            let raw_line = self.lines[line_index];
            self.process_line(line_number, raw_line);
        }
    }

    fn process_line(&mut self, line_number: usize, raw: &str) {
        let line = raw.trim_end_matches(['\r', '\n']);
        let (indent, content) = split_indent(line);

        if let Some(state) = &self.multiline
            && indent <= state.base_indent
            && !content.trim().is_empty()
        {
            self.multiline = None;
        }

        if content.trim().is_empty() {
            return;
        }

        if let Some(state) = self.multiline.as_mut() {
            if !self.cfg.check_multi_line_strings {
                return;
            }
            let expected = state.expected_indent(indent, &mut self.spaces);
            if indent != expected {
                self.diagnostics.push(Violation {
                    line: line_number,
                    column: indent + 1,
                    message: format!("wrong indentation: expected {expected}but found {indent}"),
                });
            }
            return;
        }

        let analysis = LineAnalysis::analyze(content);

        while self.current_indent() > indent {
            self.contexts.pop();
        }

        let parent_indent = self.current_indent();

        if indent > parent_indent {
            let kind = self
                .pending_child
                .take()
                .unwrap_or_else(|| analysis.context_kind());
            self.contexts.push(Context { indent, kind });
            self.spaces
                .observe_increase(parent_indent, indent, line_number, &mut self.diagnostics);
        } else {
            self.spaces
                .observe_indent(indent, line_number, &mut self.diagnostics);
            self.pending_child = None;
        }

        if analysis.is_mapping_key()
            && let Some(ctx) = self.contexts.last_mut()
        {
            ctx.kind = ContextKind::Mapping;
        }

        if analysis.is_sequence_entry() {
            self.check_sequence_indent(indent, line_number);
        }

        self.check_mapping_indent(indent, line_number, analysis);

        if analysis.starts_multiline {
            self.multiline = Some(MultilineState::new(indent));
        }

        if analysis.opens_child_context() {
            self.pending_child = Some(analysis.context_kind());
        } else {
            self.pending_child = None;
        }
    }

    fn current_indent(&self) -> usize {
        self.contexts.last().map_or(0, |ctx| ctx.indent)
    }

    fn find_mapping_parent_indent(&self, current_indent: usize) -> Option<usize> {
        self.contexts
            .iter()
            .rev()
            .find(|ctx| ctx.indent < current_indent && matches!(ctx.kind, ContextKind::Mapping))
            .map_or_else(
                || {
                    self.contexts
                        .iter()
                        .rev()
                        .find(|ctx| {
                            ctx.indent == current_indent && matches!(ctx.kind, ContextKind::Mapping)
                        })
                        .map(|ctx| ctx.indent)
                },
                |ctx| Some(ctx.indent),
            )
    }

    fn check_sequence_indent(&mut self, indent: usize, line_number: usize) {
        let Some(parent_indent) = self.find_mapping_parent_indent(indent) else {
            return;
        };

        let is_indented = indent > parent_indent;
        if let Some(message) = self.indent_seq.check(parent_indent, indent, is_indented) {
            self.diagnostics.push(Violation {
                line: line_number,
                column: indent + 1,
                message,
            });
        }
    }

    fn check_mapping_indent(&mut self, indent: usize, line_number: usize, analysis: LineAnalysis) {
        if analysis.is_mapping_key() {
            let expected = self.current_indent();
            if indent != expected {
                self.diagnostics.push(Violation {
                    line: line_number,
                    column: indent + 1,
                    message: format!("wrong indentation: expected {expected} but found {indent}"),
                });
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Context {
    indent: usize,
    kind: ContextKind,
}

#[derive(Debug, Clone, Copy)]
enum ContextKind {
    Root,
    Mapping,
    Sequence,
    Other,
}

#[derive(Debug, Clone, Copy)]
struct LineAnalysis {
    kind: LineKind,
    starts_multiline: bool,
}

#[derive(Debug, Clone, Copy)]
enum LineKind {
    Mapping { opens_block: bool },
    Sequence,
    Other,
}

impl LineAnalysis {
    fn analyze(content: &str) -> Self {
        let stripped = strip_trailing_comment(content);
        let (core, _comment) = stripped;
        let trimmed = core.trim();
        let is_sequence_entry = is_sequence_entry(trimmed);
        let (is_mapping_key, opens_block) = classify_mapping(trimmed);
        let kind = if is_mapping_key {
            LineKind::Mapping { opens_block }
        } else if is_sequence_entry {
            LineKind::Sequence
        } else {
            LineKind::Other
        };
        let starts_multiline = detect_multiline_indicator(trimmed);
        Self {
            kind,
            starts_multiline,
        }
    }

    const fn context_kind(self) -> ContextKind {
        match self.kind {
            LineKind::Mapping { .. } => ContextKind::Mapping,
            LineKind::Sequence => ContextKind::Sequence,
            LineKind::Other => ContextKind::Other,
        }
    }

    const fn opens_child_context(self) -> bool {
        matches!(self.kind, LineKind::Mapping { opens_block: true })
    }

    const fn is_mapping_key(self) -> bool {
        matches!(self.kind, LineKind::Mapping { .. })
    }

    const fn is_sequence_entry(self) -> bool {
        matches!(self.kind, LineKind::Sequence)
    }
}

#[derive(Debug, Clone, Copy)]
struct MultilineState {
    base_indent: usize,
    expected_indent: Option<usize>,
}

impl MultilineState {
    const fn new(base_indent: usize) -> Self {
        Self {
            base_indent,
            expected_indent: None,
        }
    }

    fn expected_indent(&mut self, indent: usize, spaces: &mut SpacesRuntime) -> usize {
        if let Some(expected) = self.expected_indent {
            expected
        } else {
            let expected = spaces.current_or_set(self.base_indent, indent);
            self.expected_indent = Some(expected);
            expected
        }
    }
}

struct SpacesRuntime {
    setting: SpacesSetting,
    value: Option<usize>,
}

impl SpacesRuntime {
    const fn new(setting: SpacesSetting) -> Self {
        Self {
            setting,
            value: None,
        }
    }

    fn current_or_set(&mut self, base: usize, found: usize) -> usize {
        match self.setting {
            SpacesSetting::Fixed(v) => base.saturating_add(v),
            SpacesSetting::Consistent => {
                let delta = found.saturating_sub(base);
                if let Some(val) = self.value {
                    base.saturating_add(val)
                } else {
                    let value = delta.max(1);
                    self.value = Some(value);
                    base.saturating_add(value)
                }
            }
        }
    }

    fn observe_increase(
        &mut self,
        base: usize,
        found: usize,
        line: usize,
        diagnostics: &mut Vec<Violation>,
    ) {
        match self.setting {
            SpacesSetting::Fixed(value) => {
                if found >= base {
                    let delta = found - base;
                    if !delta.is_multiple_of(value) {
                        let expected = base.saturating_add(value);
                        diagnostics.push(Violation {
                            line,
                            column: found + 1,
                            message: format!(
                                "wrong indentation: expected {expected} but found {found}"
                            ),
                        });
                    }
                }
            }
            SpacesSetting::Consistent => {
                if found > base {
                    let delta = found - base;
                    if let Some(val) = self.value {
                        if !delta.is_multiple_of(val) {
                            let expected = base.saturating_add(val);
                            diagnostics.push(Violation {
                                line,
                                column: found + 1,
                                message: format!(
                                    "wrong indentation: expected {expected} but found {found}"
                                ),
                            });
                        }
                    } else {
                        self.value = Some(delta);
                    }
                }
            }
        }
    }

    fn observe_indent(&self, indent: usize, line: usize, diagnostics: &mut Vec<Violation>) {
        match self.setting {
            SpacesSetting::Fixed(value) => {
                if !indent.is_multiple_of(value) {
                    diagnostics.push(Violation {
                        line,
                        column: indent + 1,
                        message: format!(
                            "wrong indentation: expected {} but found {}",
                            indent / value * value,
                            indent
                        ),
                    });
                }
            }
            SpacesSetting::Consistent => {
                if let Some(val) = self.value
                    && !indent.is_multiple_of(val)
                {
                    let exp = indent / val * val;
                    diagnostics.push(Violation {
                        line,
                        column: indent + 1,
                        message: format!("wrong indentation: expected {exp} but found {indent}"),
                    });
                }
            }
        }
    }
}

struct IndentSequencesRuntime {
    setting: IndentSequencesSetting,
    consistent: Option<bool>,
}

impl IndentSequencesRuntime {
    const fn new(setting: IndentSequencesSetting) -> Self {
        Self {
            setting,
            consistent: None,
        }
    }

    fn check(
        &mut self,
        parent_indent: usize,
        found_indent: usize,
        is_indented: bool,
    ) -> Option<String> {
        match self.setting {
            IndentSequencesSetting::True => {
                if is_indented {
                    None
                } else {
                    Some(format!(
                        "wrong indentation: expected {} but found {}",
                        parent_indent + 2,
                        found_indent
                    ))
                }
            }
            IndentSequencesSetting::False => {
                if is_indented {
                    Some(format!(
                        "wrong indentation: expected {parent_indent} but found {found_indent}"
                    ))
                } else {
                    None
                }
            }
            IndentSequencesSetting::Whatever => None,
            IndentSequencesSetting::Consistent => {
                if let Some(expected) = self.consistent {
                    if expected == is_indented {
                        None
                    } else {
                        let exp_indent = if expected {
                            parent_indent + 2
                        } else {
                            parent_indent
                        };
                        Some(format!(
                            "wrong indentation: expected {exp_indent} but found {found_indent}"
                        ))
                    }
                } else {
                    self.consistent = Some(is_indented);
                    None
                }
            }
        }
    }
}

fn split_indent(line: &str) -> (usize, &str) {
    let mut count = 0;
    for ch in line.chars() {
        match ch {
            ' ' | '\t' => count += 1,
            _ => break,
        }
    }
    let content = &line[count..];
    (count, content)
}

fn strip_trailing_comment(line: &str) -> (&str, Option<&str>) {
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    for (idx, ch) in line.char_indices() {
        match ch {
            '\\' => escaped = !escaped,
            '\'' if !escaped && !in_double => in_single = !in_single,
            '"' if !escaped && !in_single => in_double = !in_double,
            '#' if !in_single && !in_double => {
                let core = line[..idx].trim_end();
                return (core, Some(&line[idx..]));
            }
            _ => escaped = false,
        }
    }
    (line.trim_end(), None)
}

fn is_sequence_entry(content: &str) -> bool {
    if !content.starts_with('-') {
        return false;
    }
    matches!(content.chars().nth(1), None | Some(' ' | '\t' | '\r' | '#'))
}

fn classify_mapping(content: &str) -> (bool, bool) {
    let mut in_single = false;
    let mut in_double = false;
    let mut brace_depth = 0;
    let mut bracket_depth = 0;
    let mut escaped = false;
    for (idx, ch) in content.char_indices() {
        match ch {
            '\\' => escaped = !escaped,
            '\'' if !escaped && !in_double => in_single = !in_single,
            '"' if !escaped && !in_single => in_double = !in_double,
            '{' if !in_single && !in_double => brace_depth += 1,
            '}' if !in_single && !in_double && brace_depth > 0 => brace_depth -= 1,
            '[' if !in_single && !in_double => bracket_depth += 1,
            ']' if !in_single && !in_double && bracket_depth > 0 => bracket_depth -= 1,
            ':' if !in_single && !in_double && brace_depth == 0 && bracket_depth == 0 => {
                let before = content[..idx].trim_end();
                if before.is_empty() {
                    return (false, false);
                }
                let after = content[idx + 1..].trim();
                let opens_block = after.is_empty();
                return (true, opens_block);
            }
            _ => escaped = false,
        }
    }
    (false, false)
}

fn detect_multiline_indicator(content: &str) -> bool {
    let base = content.trim_end_matches(|ch: char| ch.is_whitespace());
    base.ends_with("|-")
        || base.ends_with("|+")
        || base.ends_with('|')
        || base.ends_with(">-")
        || base.ends_with(">+")
        || base.ends_with('>')
}
