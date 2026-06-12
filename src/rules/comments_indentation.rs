//! `comments-indentation`: a comment must be indented like the content that follows
//! it (else like the content it trails). Mirrors yamllint's `comments-indentation`.
//! Safe `--fix` re-indents the comment — comments carry no YAML structure, so moving
//! one cannot change the parse.
//!
//! The ryl-only, TOML-only `allow-any-open-indent` option (default off; origin
//! adrienverge/yamllint#141) additionally accepts a comment whose indent matches any
//! still-open enclosing block level, not just the following content — e.g. a comment
//! at the parent mapping's indent marking where a nested block ends. The open levels
//! are derived from granit's parsed block structure (`compute_open_indents`), not a
//! line scan, so multiline scalars, URLs, and flow collections never create a false
//! level; on a parse error the option degrades to the base next/reference rule.

use granit_parser::{Event, Parser, Span, SpannedEventReceiver};

use crate::config::YamlLintConfig;
use crate::rules::support::line_syntax::{
    block_scalar_marker_index, leading_whitespace_width, split_lines_preserve_endings,
    strip_trailing_comment_preserving_quotes,
};
use crate::rules::support::span_utils::marker_byte_offset;

pub const ID: &str = "comments-indentation";
pub const MESSAGE: &str = "comment not indented like content";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Config {
    allow_any_open_indent: bool,
}

impl Config {
    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        Self {
            allow_any_open_indent: cfg.rule_option_bool(
                ID,
                "allow-any-open-indent",
                false,
            ),
        }
    }

    /// Construct a config for tests, bypassing YAML/TOML resolution.
    #[must_use]
    pub const fn new_for_tests(allow_any_open_indent: bool) -> Self {
        Self {
            allow_any_open_indent,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
}

#[must_use]
pub fn check(buffer: &str, cfg: &Config) -> Vec<Violation> {
    let mut diagnostics: Vec<Violation> = Vec::new();
    if buffer.is_empty() {
        return diagnostics;
    }

    let lines = build_lines(buffer);

    let prev_content_indents = compute_prev_content_indents(&lines);
    let next_content_indents = compute_next_content_indents(&lines);
    // Only the option's open-block check consults this, so skip the parse entirely
    // when it's off (the common default path).
    let open_indents = if cfg.allow_any_open_indent {
        compute_open_indents(buffer, lines.len())
    } else {
        Vec::new()
    };

    let mut last_comment_indent: Option<usize> = None;

    for (idx, line) in lines.iter().enumerate() {
        match line.kind {
            LineKind::Comment => {
                let prev_indent = prev_content_indents[idx].unwrap_or(0);
                let next_indent = next_content_indents[idx].unwrap_or(0);
                let reference_indent =
                    last_comment_indent.unwrap_or_else(|| prev_indent.max(next_indent));

                if !comment_is_aligned(
                    line.indent,
                    reference_indent,
                    next_indent,
                    open_indents.get(idx).map_or(&[], Vec::as_slice),
                ) {
                    diagnostics.push(Violation {
                        line: idx + 1,
                        column: line.indent + 1,
                    });
                }

                last_comment_indent = Some(line.indent);
            }
            LineKind::Other | LineKind::DirectiveComment => {
                last_comment_indent = None;
            }
            LineKind::Empty | LineKind::BlockScalarContent => {}
        }
    }

    diagnostics
}

#[must_use]
pub fn fix(buffer: &str, cfg: &Config) -> Option<String> {
    if buffer.is_empty() {
        return None;
    }

    let lines = build_lines(buffer);

    let prev_content_indents = compute_prev_content_indents(&lines);
    let next_content_indents = compute_next_content_indents(&lines);
    let open_indents = if cfg.allow_any_open_indent {
        compute_open_indents(buffer, lines.len())
    } else {
        Vec::new()
    };

    let mut changed = false;
    let mut last_comment_indent: Option<usize> = None;
    let mut output = String::with_capacity(buffer.len());

    for ((line_idx, raw_line, ending), line) in
        split_lines_preserve_endings(buffer).zip(lines.iter())
    {
        match line.kind {
            LineKind::Comment => {
                let prev_indent = prev_content_indents[line_idx].unwrap_or(0);
                let next_indent = next_content_indents[line_idx].unwrap_or(0);
                let reference_indent =
                    last_comment_indent.unwrap_or_else(|| prev_indent.max(next_indent));
                let target_indent = if comment_is_aligned(
                    line.indent,
                    reference_indent,
                    next_indent,
                    open_indents.get(line_idx).map_or(&[], Vec::as_slice),
                ) {
                    line.indent
                } else {
                    changed = true;
                    reference_indent
                };
                last_comment_indent = Some(target_indent);
                output.push_str(&" ".repeat(target_indent));
                output.push_str(raw_line.trim_start_matches([' ', '\t']));
            }
            LineKind::Other | LineKind::DirectiveComment => {
                last_comment_indent = None;
                output.push_str(raw_line);
            }
            LineKind::Empty | LineKind::BlockScalarContent => {
                output.push_str(raw_line);
            }
        }
        output.push_str(ending);
    }

    changed.then_some(output)
}

/// A standalone comment lines up when it matches the content below (`next_indent`),
/// the active reference indent, or — under `allow-any-open-indent` — any still-open
/// enclosing block level.
fn comment_is_aligned(
    indent: usize,
    reference_indent: usize,
    next_indent: usize,
    open_indents: &[usize],
) -> bool {
    indent == reference_indent
        || indent == next_indent
        || open_indents.contains(&indent)
}

/// Classify every line once for both `check` and `fix`: its indent and kind.
fn build_lines(buffer: &str) -> Vec<LineInfo> {
    let mut block_tracker = BlockScalarTracker::default();
    let mut lines: Vec<LineInfo> = Vec::new();
    for (_, line, _) in split_lines_preserve_endings(buffer) {
        let indent = leading_whitespace_width(line);
        let content = &line[indent..];

        let consumed = block_tracker.consume_line(indent, content);
        let kind = if consumed {
            LineKind::BlockScalarContent
        } else {
            classify_line_kind(content)
        };

        lines.push(LineInfo { indent, kind });
        block_tracker.observe_indicator(indent, content);
    }
    lines
}

/// For each line (1-based line `L` → `result[L - 1]`), the columns of the block
/// collections that enclose it — the levels `allow-any-open-indent` accepts a comment
/// against. Derived from granit's parsed structure: each block mapping/sequence
/// contributes its start column to every line of its span. Because the levels come
/// from real nodes, a multiline scalar's continuation lines, a URL's `:`, and flow
/// `{}`/`[]` collections never appear as a level. On a parse error every set is empty,
/// so the option degrades to the base next/reference rule (the input is a syntax error
/// regardless).
fn compute_open_indents(buffer: &str, line_count: usize) -> Vec<Vec<usize>> {
    struct Collector<'b> {
        buffer: &'b str,
        /// Open collections: `(start column, is block, start line)`.
        stack: Vec<(usize, bool, usize)>,
        /// Closed block collections: `(column, start line, end line)`.
        intervals: Vec<(usize, usize, usize)>,
    }
    impl SpannedEventReceiver<'_> for Collector<'_> {
        fn on_event(&mut self, event: Event<'_>, span: Span) {
            match event {
                Event::MappingStart(..) | Event::SequenceStart(..) => {
                    // A collection carries a block indentation level only when it is
                    // itself block (starts at a key/`-`, not `{`/`[`) AND is not nested
                    // inside a flow collection -- flow children such as the implicit
                    // mappings in `[a: 1, b: 2]` start at a key but have no block level.
                    let byte = marker_byte_offset(span.start).get();
                    let parent_is_flow =
                        self.stack.last().is_some_and(|&(_, block, _)| !block);
                    let is_block = !parent_is_flow
                        && !buffer_starts_with_flow_indicator(self.buffer, byte);
                    self.stack
                        .push((span.start.col(), is_block, span.start.line()));
                }
                Event::MappingEnd | Event::SequenceEnd => {
                    if let Some((col, is_block, start_line)) = self.stack.pop()
                        && is_block
                    {
                        self.intervals.push((col, start_line, span.start.line()));
                    }
                }
                _ => {}
            }
        }
    }

    let mut collector = Collector {
        buffer,
        stack: Vec::new(),
        intervals: Vec::new(),
    };
    if Parser::new_from_str(buffer)
        .load(&mut collector, true)
        .is_err()
    {
        return vec![Vec::new(); line_count];
    }

    let mut result = vec![Vec::new(); line_count];
    for (col, start, end) in collector.intervals {
        // Slice directly to the interval's lines (`take().skip()` would re-walk from 0
        // each time, making this O(sum of end lines) — quadratic for deep nesting).
        let hi = end.min(line_count);
        let lo = (start - 1).min(hi);
        for entry in &mut result[lo..hi] {
            entry.push(col);
        }
    }
    result
}

fn buffer_starts_with_flow_indicator(buffer: &str, byte: usize) -> bool {
    buffer[byte..].starts_with(['{', '['])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LineInfo {
    indent: usize,
    kind: LineKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineKind {
    Empty,
    Comment,
    DirectiveComment,
    BlockScalarContent,
    Other,
}

fn classify_line_kind(content: &str) -> LineKind {
    let trimmed = content.trim_start_matches([' ', '\t']);

    if trimmed.is_empty() {
        LineKind::Empty
    } else if trimmed.starts_with("# yamllint ") {
        LineKind::DirectiveComment
    } else if trimmed.starts_with('#') {
        LineKind::Comment
    } else {
        LineKind::Other
    }
}

#[derive(Debug, Default)]
struct BlockScalarTracker {
    state: Option<BlockScalarState>,
}

#[derive(Debug)]
struct BlockScalarState {
    indicator_indent: usize,
    content_indent: Option<usize>,
}

impl BlockScalarTracker {
    fn consume_line(&mut self, indent: usize, content: &str) -> bool {
        let Some(state) = self.state.as_mut() else {
            return false;
        };

        if content.trim().is_empty() {
            return true;
        }

        let updated_indent = if let Some(content_indent) = state.content_indent {
            if indent >= content_indent {
                return true;
            }
            if indent <= state.indicator_indent {
                self.state = None;
                return false;
            }
            content_indent.min(indent)
        } else {
            if indent <= state.indicator_indent {
                self.state = None;
                return false;
            }
            indent
        };
        state.content_indent = Some(updated_indent);
        true
    }

    fn observe_indicator(&mut self, indent: usize, content: &str) {
        let candidate = strip_trailing_comment_for_block(content).trim_end();
        if is_block_scalar_indicator(candidate) {
            self.state = Some(BlockScalarState {
                indicator_indent: indent,
                content_indent: None,
            });
        }
    }
}

fn compute_prev_content_indents(lines: &[LineInfo]) -> Vec<Option<usize>> {
    let mut result: Vec<Option<usize>> = Vec::with_capacity(lines.len());
    let mut latest: Option<usize> = None;
    for line in lines {
        if line.kind == LineKind::Other {
            latest = Some(line.indent);
        }
        result.push(latest);
    }
    result
}

fn compute_next_content_indents(lines: &[LineInfo]) -> Vec<Option<usize>> {
    let mut result: Vec<Option<usize>> = vec![None; lines.len()];
    let mut upcoming: Option<usize> = None;
    for (idx, line) in lines.iter().enumerate().rev() {
        if line.kind == LineKind::Other {
            upcoming = Some(line.indent);
        }
        result[idx] = upcoming;
    }
    result
}

fn strip_trailing_comment_for_block(content: &str) -> &str {
    strip_trailing_comment_preserving_quotes(content)
}

fn is_block_scalar_indicator(content: &str) -> bool {
    let Some(marker_idx) = block_scalar_marker_index(content) else {
        return false;
    };
    let trimmed = content.trim_end_matches(|ch: char| ch.is_whitespace());
    let prefix = trimmed[..marker_idx].trim_end();
    prefix.ends_with(':') || prefix.ends_with('-') || prefix.ends_with('?')
}
