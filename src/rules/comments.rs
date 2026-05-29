use granit_parser::{Event, Parser, Placement, Span};

use crate::config::YamlLintConfig;
use crate::rules::support::span_utils::{BytePos, apply_replacements};

pub const ID: &str = "comments";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    require_starting_space: bool,
    ignore_shebangs: bool,
    min_spaces_from_content: Option<usize>,
}

impl Config {
    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        let require_starting_space =
            cfg.rule_option_bool(ID, "require-starting-space", true);
        let ignore_shebangs = cfg.rule_option_bool(ID, "ignore-shebangs", true);
        let min_spaces_value = cfg.rule_option_int(ID, "min-spaces-from-content", 2);

        let min_spaces_from_content = if min_spaces_value < 0 {
            None
        } else {
            Some(usize::try_from(min_spaces_value).unwrap_or(usize::MAX))
        };

        Self {
            require_starting_space,
            ignore_shebangs,
            min_spaces_from_content,
        }
    }

    const fn require_starting_space(&self) -> bool {
        self.require_starting_space
    }

    const fn ignore_shebangs(&self) -> bool {
        self.ignore_shebangs
    }

    const fn min_spaces_from_content(&self) -> Option<usize> {
        self.min_spaces_from_content
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
/// Panics if granit's parser fails to populate byte offsets on comment
/// spans; with [`Parser::new_from_str`] this is always populated.
#[must_use]
pub fn check(buffer: &str, cfg: &Config) -> Vec<Violation> {
    let mut violations = Vec::new();
    for comment in collect_comments(buffer) {
        let line = comment.span.start.line();
        let hash_column = comment.span.start.col() + 1;
        let is_inline = comment.placement == Placement::Right;

        if let Some(required) = cfg.min_spaces_from_content()
            && is_inline
        {
            let byte_start =
                comment.span.start.byte_offset().expect(
                    "granit Parser::new_from_str always populates byte offsets",
                );
            let line_start = line_start_byte(buffer, byte_start);
            let spacing = buffer[line_start..byte_start]
                .chars()
                .rev()
                .take_while(|ch| matches!(ch, ' ' | '\t'))
                .count();
            if spacing < required {
                violations.push(Violation {
                    line,
                    column: hash_column,
                    message: format!(
                        "too few spaces before comment: expected {required}"
                    ),
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

        if cfg.ignore_shebangs() && line == 1 && hash_column == 1 && next_char == '!' {
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
/// Panics if granit's parser fails to populate byte offsets on comment
/// spans; with [`Parser::new_from_str`] this is always populated.
#[must_use]
pub fn fix(buffer: &str, cfg: &Config) -> Option<String> {
    let mut edits: Vec<(BytePos, BytePos, String)> = Vec::new();

    for comment in collect_comments(buffer) {
        let byte_start = comment
            .span
            .start
            .byte_offset()
            .expect("granit Parser::new_from_str always populates byte offsets");
        let line = comment.span.start.line();
        let hash_column = comment.span.start.col() + 1;
        let is_inline = comment.placement == Placement::Right;

        if let Some(required) = cfg.min_spaces_from_content()
            && is_inline
        {
            let line_start = line_start_byte(buffer, byte_start);
            let spacing = buffer[line_start..byte_start]
                .chars()
                .rev()
                .take_while(|ch| matches!(ch, ' ' | '\t'))
                .count();
            if spacing < required {
                let at = BytePos::new(byte_start);
                edits.push((at, at, " ".repeat(required - spacing)));
            }
        }

        if !cfg.require_starting_space() {
            continue;
        }

        let extra_hash_bytes: usize = comment
            .text
            .chars()
            .take_while(|c| *c == '#')
            .map(char::len_utf8)
            .sum();
        let after_hashes = &comment.text[extra_hash_bytes..];
        let Some(next_char) = after_hashes.chars().next() else {
            continue;
        };

        if cfg.ignore_shebangs() && line == 1 && hash_column == 1 && next_char == '!' {
            continue;
        }

        if next_char != ' ' {
            let at = BytePos::new(byte_start + '#'.len_utf8() + extra_hash_bytes);
            edits.push((at, at, " ".to_string()));
        }
    }

    if edits.is_empty() {
        return None;
    }

    Some(apply_replacements(buffer, edits))
}

struct CommentInfo {
    span: Span,
    text: String,
    placement: Placement,
}

fn collect_comments(buffer: &str) -> Vec<CommentInfo> {
    let mut parser = Parser::new_from_str(buffer);
    let mut comments = Vec::new();
    let mut last_err_at: Option<usize> = None;
    while let Some(res) = parser.next_event() {
        match res {
            Ok((Event::Comment(text, placement), span)) => {
                comments.push(CommentInfo {
                    span,
                    text: text.into_owned(),
                    placement,
                });
                last_err_at = None;
            }
            Ok(_) => last_err_at = None,
            Err(e) => {
                let pos = e.marker().index();
                if last_err_at == Some(pos) {
                    break;
                }
                last_err_at = Some(pos);
            }
        }
    }
    comments
}

fn line_start_byte(buffer: &str, byte_offset: usize) -> usize {
    buffer[..byte_offset].rfind('\n').map_or(0, |i| i + 1)
}
