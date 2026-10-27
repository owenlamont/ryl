//! `comments`: `#` comment formatting: a required space after the `#`, a minimum and
//! (ryl-only) maximum gap from preceding inline content, and an optional exemption
//! for a line-1 shebang or `#cloud-config`. Mirrors yamllint's `comments`. Safe
//! `--fix` pads or trims the spaces.
use crate::rules::support::span_utils::{BytePos, marker_byte_offset};
use crate::text_version::edits::Edit;

use ryl_yaml_parser::Placement;

use crate::config::YamlLintConfig;
use crate::rules::support::comments_scan::CommentInfo;
use crate::rules::support::line_syntax::is_magic_first_line_comment;
use crate::text_version::{Analysis, TextVersion};

mod input;
mod target;
pub(crate) use target::Magic;

pub const ID: &str = "comments";

pub(crate) struct Trailing(Option<std::ops::Range<usize>>);

impl Trailing {
    pub(crate) fn observe(buffer: &str, after: usize, end: usize) -> Self {
        let text = buffer[after..end].trim_matches([' ', '\t', '\n', '\r']);
        let start = text.as_ptr() as usize - buffer.as_ptr() as usize;
        Self(text.starts_with('#').then_some(start..start + text.len()))
    }

    pub(crate) const fn present(&self) -> bool {
        self.0.is_some()
    }
    pub(crate) fn range(&self) -> Option<std::ops::Range<usize>> {
        self.0.clone()
    }
}

#[derive(Default)]
pub(crate) struct RetainedComments(std::collections::HashSet<usize>);

#[derive(Default)]
pub(crate) struct Attachments(Vec<(usize, usize)>);

impl Attachments {
    pub(crate) fn record(&mut self, span: ryl_yaml_parser::Span) {
        self.0.push((
            marker_byte_offset(span.start).get(),
            marker_byte_offset(span.end).get(),
        ));
    }

    pub(crate) fn between(&self, from: usize, to: usize) -> bool {
        self.0
            .get(self.0.partition_point(|(start, _)| *start < from))
            .is_some_and(|(start, _)| *start < to)
    }

    pub(crate) fn touches(&self, at: usize) -> bool {
        self.0
            .get(self.0.partition_point(|(_, end)| *end < at))
            .is_some_and(|(_, end)| *end == at)
    }
}

impl RetainedComments {
    pub(crate) fn record(&mut self, span: ryl_yaml_parser::Span) {
        self.0.insert(span.start.line() - 1);
    }

    pub(crate) fn line(&self, line: usize) -> bool {
        self.0.contains(&line)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct RetainedPrefix(bool);

impl RetainedPrefix {
    pub(crate) fn observe(text: &str) -> Self {
        Self(text.contains('#'))
    }

    pub(crate) const fn has_comment(self) -> bool {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    require_starting_space: bool,
    ignore_shebangs: bool,
    min_spaces_from_content: Option<usize>,
    max_spaces_from_content: Option<usize>,
}

impl Config {
    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        let require_starting_space =
            cfg.rule_option_bool(ID, "require-starting-space", true);
        let ignore_shebangs = cfg.rule_option_bool(ID, "ignore-shebangs", true);
        let spacing =
            |key, default| usize::try_from(cfg.rule_option_int(ID, key, default)).ok();

        Self {
            require_starting_space,
            ignore_shebangs,
            min_spaces_from_content: spacing("min-spaces-from-content", 2),
            max_spaces_from_content: spacing("max-spaces-from-content", -1),
        }
    }

    /// Exactly `spaces` between content and an inline comment.
    #[must_use]
    pub const fn exact_gap(spaces: usize, require_starting_space: bool) -> Self {
        Self {
            require_starting_space,
            ignore_shebangs: true,
            min_spaces_from_content: Some(spaces),
            max_spaces_from_content: Some(spaces),
        }
    }

    const fn require_starting_space(&self) -> bool {
        self.require_starting_space
    }

    pub(crate) const fn ignore_shebangs(&self) -> bool {
        self.ignore_shebangs
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

/// Run the comments rule against `buffer`.
///
/// # Panics
///
/// Panics if the parser fails to populate byte offsets on comment
/// spans; with [`Parser::new_from_str`] this is always populated.
#[must_use]
pub fn check(buffer: &str, cfg: &Config) -> Vec<Violation> {
    check_with_analysis(TextVersion::new(buffer).analysis(), cfg)
}

pub(crate) fn check_with_analysis(input: Analysis<'_>, cfg: &Config) -> Vec<Violation> {
    let buffer = input.text();
    let mut violations = Vec::new();
    for comment in
        crate::rules::support::comments_scan::collect_comments_with_analysis(input)
    {
        let line = comment.span.start.line();
        let hash_column = comment.span.start.col() + 1;

        if comment.placement == Placement::Right {
            let byte_start = comment_byte_start(&comment);
            let spacing = spacing_before(buffer, byte_start);
            let message =
                match (cfg.min_spaces_from_content, cfg.max_spaces_from_content) {
                    (Some(min), _) if spacing < min => {
                        Some(format!("too few spaces before comment: expected {min}"))
                    }
                    (_, Some(max)) if spacing > max => Some(format!(
                        "too many spaces before comment: expected at most {max}"
                    )),
                    _ => None,
                };
            if let Some(message) = message {
                violations.push(Violation {
                    line,
                    column: hash_column,
                    message,
                });
            }
        }

        if !cfg.require_starting_space() {
            continue;
        }

        let extra_hashes_count = comment.text.chars().take_while(|c| *c == '#').count();
        let after_hashes = comment.text.trim_start_matches('#');
        let Some(next_char) = after_hashes.chars().next() else {
            continue;
        };

        if cfg.ignore_shebangs()
            && line == 1
            && hash_column == 1
            && is_magic_first_line_comment(&comment.text)
        {
            continue;
        }

        if next_char != ' ' {
            violations.push(Violation {
                line,
                column: hash_column + 1 + extra_hashes_count,
                message: "missing starting space in comment".to_string(),
            });
        }
    }

    violations
}

/// Apply the comments rule's auto-fix to `buffer`.
///
/// # Panics
///
/// Panics if the parser fails to populate byte offsets on comment
/// spans; with [`Parser::new_from_str`] this is always populated.
#[must_use]
pub fn fix(buffer: &str, cfg: &Config) -> Option<String> {
    fix_with_analysis(TextVersion::new(buffer).analysis(), cfg)
}

pub(crate) fn fix_with_analysis(input: Analysis<'_>, cfg: &Config) -> Option<String> {
    crate::text_version::edits::render(input.text(), edits(input, cfg))
}

pub(crate) fn edits(input: Analysis<'_>, cfg: &Config) -> Vec<Edit> {
    let buffer = input.text();
    let mut edits: Vec<(BytePos, BytePos, String)> = Vec::new();

    for comment in
        crate::rules::support::comments_scan::collect_comments_with_analysis(input)
    {
        let byte_start = comment_byte_start(&comment);
        let line = comment.span.start.line();
        let hash_column = comment.span.start.col() + 1;

        let model = crate::text_version::model::Comment::new(
            &comment.text,
            line == 1 && hash_column == 1 && is_magic_first_line_comment(&comment.text),
        );
        let initial = target::Initial {
            gap: (comment.placement == Placement::Right)
                .then(|| spacing_before(buffer, byte_start)),
        };
        let target = target::Target::plan(model, initial, cfg);
        let at = BytePos::new(byte_start);
        match target.gap() {
            target::Gap::Keep => {}
            target::Gap::Insert(spaces) => edits.push((at, at, " ".repeat(spaces))),
            target::Gap::Replace { old, target } => {
                edits.push((BytePos::new(byte_start - old), at, " ".repeat(target)));
            }
        }
        if let Some(hashes) = target.starting_space() {
            let at = BytePos::new(byte_start + '#'.len_utf8() + hashes);
            edits.push((at, at, " ".to_string()));
        }
    }

    edits
}

pub(crate) fn edits_with_magic(
    source: Analysis<'_>,
    cfg: &Config,
    permission: crate::format::layout::Permission<'_>,
) -> (Vec<Edit>, Magic) {
    let edits = edits(source, cfg);
    let magic = Magic::plan(input::Magic::new(source.text()), cfg, &edits, permission);
    (edits, magic)
}

fn comment_byte_start(comment: &CommentInfo) -> usize {
    comment
        .span
        .start
        .byte_offset()
        .expect("Parser::new_from_str always populates byte offsets")
}

fn spacing_before(buffer: &str, byte_start: usize) -> usize {
    buffer[..byte_start]
        .bytes()
        .rev()
        .take_while(|byte| matches!(byte, b' ' | b'\t'))
        .count()
}

pub(crate) struct RetainedFirstLine(bool);

impl RetainedFirstLine {
    pub(crate) const fn magic(&self) -> bool {
        self.0
    }
}

pub(crate) fn retained_first_line(line: &str) -> RetainedFirstLine {
    RetainedFirstLine(crate::rules::support::line_syntax::is_magic_first_line(
        line.strip_prefix('\u{feff}').unwrap_or(line),
    ))
}

pub(crate) use input::DirectiveTail;

pub(crate) const fn directive_tail(
    source: crate::text_version::Analysis<'_>,
    end: BytePos,
) -> DirectiveTail<'_> {
    DirectiveTail::new(source, end)
}
