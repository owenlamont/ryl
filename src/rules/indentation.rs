use crate::config::YamlLintConfig;
use crate::rules::support::line_syntax::block_scalar_marker_index;

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
        let spaces =
            cfg.rule_option(ID, "spaces")
                .map_or(SpacesSetting::Consistent, |node| {
                    node.as_integer()
                        .map_or(SpacesSetting::Consistent, |value| {
                            let non_negative = value.max(0);
                            let fixed =
                                usize::try_from(non_negative).unwrap_or(usize::MAX);
                            SpacesSetting::Fixed(fixed)
                        })
                });

        let indent_sequences = cfg.rule_option(ID, "indent-sequences").map_or(
            IndentSequencesSetting::True,
            |node| {
                if let Some(choice) = node.as_str() {
                    return if choice == "whatever" {
                        IndentSequencesSetting::Whatever
                    } else {
                        IndentSequencesSetting::Consistent
                    };
                }

                if node.as_bool() == Some(false) {
                    IndentSequencesSetting::False
                } else {
                    IndentSequencesSetting::True
                }
            },
        );

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
    sequence_expectations: Vec<Option<bool>>,
    spaces: SpacesRuntime,
    indent_seq: IndentSequencesRuntime,
    pending_child: Option<ContextKind>,
    multiline: Option<MultilineState>,
    active_sequence_mapping_parent: Option<SequenceMappingParent>,
    compact_sequence_parent_indent: Option<usize>,
    compact_flow_mapping: Option<CompactFlowMapping>,
    prev_line_kind: Option<LineKind>,
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
            sequence_expectations: vec![None],
            spaces: SpacesRuntime::new(cfg.spaces),
            indent_seq: IndentSequencesRuntime::new(cfg.indent_sequences),
            pending_child: None,
            multiline: None,
            active_sequence_mapping_parent: None,
            compact_sequence_parent_indent: None,
            compact_flow_mapping: None,
            prev_line_kind: None,
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
        self.reset_transient_state(indent, content);

        if content.trim().is_empty() {
            self.prev_line_kind = Some(LineKind::Other);
            return;
        }

        if let Some(state) = self.multiline.as_mut() {
            if !self.cfg.check_multi_line_strings {
                self.prev_line_kind = Some(LineKind::Other);
                return;
            }
            let expected = state.expected_indent(indent, &mut self.spaces);
            if indent != expected {
                self.diagnostics.push(Violation {
                    line: line_number,
                    column: indent + 1,
                    message: format!(
                        "wrong indentation: expected {expected} but found {indent}"
                    ),
                });
            }
            return;
        }

        if content.trim_start().starts_with('#') {
            return;
        }

        let analysis = LineAnalysis::analyze(content);
        let compact_mapping_continuation =
            self.is_compact_mapping_continuation(indent, analysis);

        let Some(pushing_child) = self.update_context_for_indent(
            line_number,
            indent,
            analysis,
            compact_mapping_continuation,
        ) else {
            return;
        };

        if pushing_child
            && analysis.is_sequence_entry()
            && let Some(ctx) = self.contexts.last_mut()
        {
            ctx.kind = ContextKind::Sequence;
        }

        if analysis.is_sequence_entry()
            && self
                .compact_sequence_parent_indent
                .is_none_or(|parent| indent <= parent)
        {
            self.check_sequence_indent(indent, line_number);
        }

        if analysis.is_mapping_key()
            && (!analysis.is_sequence_entry() || pushing_child)
            && let Some(ctx) = self.contexts.last_mut()
        {
            ctx.kind = analysis.context_kind();
        }
        self.update_post_analysis_state(indent, content, analysis);
    }

    fn reset_transient_state(&mut self, indent: usize, content: &str) {
        if self
            .compact_sequence_parent_indent
            .is_some_and(|parent| indent <= parent)
        {
            self.compact_sequence_parent_indent = None;
        }
        if self
            .compact_flow_mapping
            .is_some_and(|state| indent <= state.parent_indent)
        {
            self.compact_flow_mapping = None;
        }

        if let Some(state) = &self.multiline
            && indent <= state.base_indent
            && !content.trim().is_empty()
        {
            self.multiline = None;
        }

        if self.active_sequence_mapping_parent.is_some_and(|state| {
            !content.trim().is_empty() && indent <= state.owner_indent
        }) {
            self.active_sequence_mapping_parent = None;
        }
    }

    fn update_post_analysis_state(
        &mut self,
        indent: usize,
        content: &str,
        analysis: LineAnalysis,
    ) {
        if analysis.starts_multiline {
            self.multiline = Some(MultilineState::new(indent));
        }

        if analysis.opens_child_context() {
            self.pending_child = Some(analysis.context_kind());
        } else {
            self.pending_child = None;
        }

        if analysis.is_sequence_entry() && analysis.opens_child_context() {
            self.active_sequence_mapping_parent = Some(SequenceMappingParent {
                owner_indent: indent,
                parent_indent: indent.saturating_add(analysis.sequence_offset),
            });
        }

        if is_compact_sequence_start(content) {
            self.compact_sequence_parent_indent = Some(indent);
        }
        if let Some(continuation_indent) =
            compact_flow_mapping_continuation_indent(content, indent)
        {
            self.compact_flow_mapping = Some(CompactFlowMapping {
                parent_indent: indent,
                continuation_indent,
            });
        }

        self.prev_line_kind = Some(analysis.kind);
    }

    fn current_indent(&self) -> usize {
        self.contexts.last().map_or(0, |ctx| ctx.indent)
    }

    fn update_context_for_indent(
        &mut self,
        line_number: usize,
        indent: usize,
        analysis: LineAnalysis,
        compact_mapping_continuation: bool,
    ) -> Option<bool> {
        while self.current_indent() > indent {
            self.contexts.pop();
            self.sequence_expectations.pop();
        }

        let parent_indent = self.current_indent();
        if indent > parent_indent {
            if matches!(analysis.kind, LineKind::Other)
                && matches!(
                    self.prev_line_kind,
                    Some(LineKind::Sequence | LineKind::Mapping { .. })
                )
            {
                return None;
            }
            let kind = self
                .pending_child
                .take()
                .unwrap_or_else(|| analysis.context_kind());
            self.contexts.push(Context { indent, kind });
            self.sequence_expectations.push(None);
            if !compact_mapping_continuation {
                self.spaces.observe_increase(
                    parent_indent,
                    indent,
                    line_number,
                    &mut self.diagnostics,
                );
            }
            Some(true)
        } else {
            if !compact_mapping_continuation {
                self.spaces
                    .observe_indent(indent, line_number, &mut self.diagnostics);
            }
            self.pending_child = None;
            Some(false)
        }
    }

    fn is_compact_mapping_continuation(
        &self,
        indent: usize,
        analysis: LineAnalysis,
    ) -> bool {
        if !analysis.is_mapping_key() {
            return false;
        }
        if self
            .compact_flow_mapping
            .is_some_and(|state| state.continuation_indent == indent)
        {
            return true;
        }
        self.contexts.iter().rev().any(|ctx| {
            let ContextKind::Mapping { sequence_offset } = ctx.kind else {
                return false;
            };
            sequence_offset > 0 && ctx.indent.saturating_add(sequence_offset) == indent
        })
    }

    fn find_mapping_parent_indent(
        &self,
        current_indent: usize,
    ) -> Option<(usize, usize)> {
        let mut saw_mapping = false;
        let mut last_mapping_index = None;
        for (idx, ctx) in self.contexts.iter().enumerate().rev() {
            let ContextKind::Mapping { sequence_offset } = ctx.kind else {
                continue;
            };
            saw_mapping = true;
            last_mapping_index = Some(idx);
            let base_indent = ctx.indent.saturating_add(sequence_offset);
            if base_indent <= current_indent {
                return Some((idx, base_indent));
            }
        }
        if saw_mapping {
            Some((last_mapping_index.unwrap(), current_indent))
        } else {
            None
        }
    }

    fn check_sequence_indent(&mut self, indent: usize, line_number: usize) {
        let (ctx_index, parent_indent) = if let Some((ctx_index, parent_indent)) =
            self.find_mapping_parent_indent(indent)
        {
            (Some(ctx_index), parent_indent)
        } else if let Some(state) = self.active_sequence_mapping_parent
            && indent > state.owner_indent
        {
            (None, state.parent_indent)
        } else {
            return;
        };

        let is_indented = indent > parent_indent;
        let expected = self
            .spaces
            .expected_step()
            .map(|step| parent_indent.saturating_add(step));

        let Some(message) = (match ctx_index {
            Some(ctx_index) => self.indent_seq.check(
                parent_indent,
                indent,
                is_indented,
                expected,
                &mut self.sequence_expectations[ctx_index],
            ),
            None => self.indent_seq.check(
                parent_indent,
                indent,
                is_indented,
                expected,
                &mut None,
            ),
        }) else {
            return;
        };

        self.diagnostics.push(Violation {
            line: line_number,
            column: indent + 1,
            message,
        });
    }
}

#[derive(Debug, Clone, Copy)]
struct Context {
    indent: usize,
    kind: ContextKind,
}

#[derive(Debug, Clone, Copy)]
struct CompactFlowMapping {
    parent_indent: usize,
    continuation_indent: usize,
}

#[derive(Debug, Clone, Copy)]
struct SequenceMappingParent {
    owner_indent: usize,
    parent_indent: usize,
}

#[derive(Debug, Clone, Copy)]
enum ContextKind {
    Root,
    Mapping { sequence_offset: usize },
    Sequence,
    Other,
}

#[derive(Debug, Clone, Copy)]
struct LineAnalysis {
    kind: LineKind,
    starts_multiline: bool,
    is_sequence_entry: bool,
    sequence_offset: usize,
}

#[derive(Debug, Clone, Copy)]
enum LineKind {
    Mapping {
        opens_block: bool,
        sequence_offset: usize,
    },
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
        let sequence_offset = if is_mapping_key {
            sequence_prefix_width(trimmed)
        } else {
            0
        };
        let kind = if is_mapping_key {
            LineKind::Mapping {
                opens_block,
                sequence_offset,
            }
        } else if is_sequence_entry {
            LineKind::Sequence
        } else {
            LineKind::Other
        };
        let starts_multiline = detect_multiline_indicator(trimmed);
        Self {
            kind,
            starts_multiline,
            is_sequence_entry,
            sequence_offset,
        }
    }

    const fn context_kind(self) -> ContextKind {
        match self.kind {
            LineKind::Mapping {
                sequence_offset, ..
            } => ContextKind::Mapping { sequence_offset },
            LineKind::Sequence => ContextKind::Sequence,
            LineKind::Other => ContextKind::Other,
        }
    }

    const fn opens_child_context(self) -> bool {
        matches!(
            self.kind,
            LineKind::Mapping {
                opens_block: true,
                ..
            }
        )
    }

    const fn is_mapping_key(self) -> bool {
        matches!(self.kind, LineKind::Mapping { .. })
    }

    const fn is_sequence_entry(self) -> bool {
        self.is_sequence_entry
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

    const fn expected_step(&self) -> Option<usize> {
        match self.setting {
            SpacesSetting::Fixed(value) => Some(value),
            SpacesSetting::Consistent => self.value,
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
                let delta = found.saturating_sub(base);
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
            SpacesSetting::Consistent => {
                let delta = found.saturating_sub(base);
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

    fn observe_indent(
        &self,
        indent: usize,
        line: usize,
        diagnostics: &mut Vec<Violation>,
    ) {
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
                        message: format!(
                            "wrong indentation: expected {exp} but found {indent}"
                        ),
                    });
                }
            }
        }
    }
}

struct IndentSequencesRuntime {
    setting: IndentSequencesSetting,
}

impl IndentSequencesRuntime {
    const fn new(setting: IndentSequencesSetting) -> Self {
        Self { setting }
    }

    fn check(
        &self,
        parent_indent: usize,
        found_indent: usize,
        is_indented: bool,
        expected_indent: Option<usize>,
        state: &mut Option<bool>,
    ) -> Option<String> {
        match self.setting {
            IndentSequencesSetting::True => {
                if !is_indented {
                    let expected = expected_indent.unwrap_or(parent_indent + 2);
                    return Some(format!(
                        "wrong indentation: expected {expected} but found {found_indent}"
                    ));
                }
                if let Some(expected) = expected_indent
                    && found_indent != expected
                {
                    return Some(format!(
                        "wrong indentation: expected {expected} but found {found_indent}"
                    ));
                }
                None
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
                if let Some(expected) = expected_indent
                    && is_indented
                    && found_indent != expected
                {
                    return Some(format!(
                        "wrong indentation: expected {expected} but found {found_indent}"
                    ));
                }
                match state {
                    Some(expected) if *expected == is_indented => None,
                    Some(expected) => {
                        let exp_indent = if *expected {
                            parent_indent + 2
                        } else {
                            parent_indent
                        };
                        Some(format!(
                            "wrong indentation: expected {exp_indent} but found {found_indent}"
                        ))
                    }
                    None => {
                        *state = Some(is_indented);
                        None
                    }
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

fn is_compact_sequence_start(content: &str) -> bool {
    let trimmed = content.trim();
    if !is_sequence_entry(trimmed) {
        return false;
    }
    let stripped = trimmed
        .strip_prefix('-')
        .expect("sequence entry starts with '-'");
    is_sequence_entry(stripped.trim_start())
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
            ':' if !in_single
                && !in_double
                && brace_depth == 0
                && bracket_depth == 0 =>
            {
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
    block_scalar_marker_index(content).is_some()
}

fn sequence_prefix_width(content: &str) -> usize {
    if !content.starts_with('-') {
        return 0;
    }
    1 + content
        .chars()
        .skip(1)
        .take_while(|ch| matches!(ch, ' ' | '\t'))
        .count()
}

fn compact_flow_mapping_continuation_indent(
    content: &str,
    indent: usize,
) -> Option<usize> {
    let trimmed = content.trim();
    if !is_sequence_entry(trimmed) {
        return None;
    }
    let base_prefix = 1 + trimmed
        .chars()
        .skip(1)
        .take_while(|ch| matches!(ch, ' ' | '\t'))
        .count();
    trimmed[base_prefix..]
        .starts_with('{')
        .then_some(indent.saturating_add(base_prefix + 1))
}
