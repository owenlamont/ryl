//! `comments`: `#` comment formatting: a required space after the `#`, a minimum and
//! (ryl-only) maximum gap from preceding inline content, and an optional shebang
//! exemption. Mirrors yamllint's `comments`. Safe `--fix` pads or trims the spaces.

use granit_parser::Placement;

use crate::config::YamlLintConfig;
use crate::rules::support::comments_scan::{CommentInfo, collect_comments};
use crate::rules::support::span_utils::{BytePos, apply_replacements};

pub const ID: &str = "comments";

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

    const fn require_starting_space(&self) -> bool {
        self.require_starting_space
    }

    const fn ignore_shebangs(&self) -> bool {
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
/// Panics if granit's parser fails to populate byte offsets on comment
/// spans; with [`Parser::new_from_str`] this is always populated.
#[must_use]
pub fn check(buffer: &str, cfg: &Config) -> Vec<Violation> {
    let mut violations = Vec::new();
    for comment in collect_comments(buffer) {
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
        let byte_start = comment_byte_start(&comment);
        let line = comment.span.start.line();
        let hash_column = comment.span.start.col() + 1;

        if comment.placement == Placement::Right {
            let spacing = spacing_before(buffer, byte_start);
            let at = BytePos::new(byte_start);
            match (cfg.min_spaces_from_content, cfg.max_spaces_from_content) {
                (Some(min), _) if spacing < min => {
                    edits.push((at, at, " ".repeat(min - spacing)));
                }
                (_, Some(max)) if spacing > max => {
                    let run_start = BytePos::new(byte_start - spacing);
                    edits.push((run_start, at, " ".repeat(max)));
                }
                _ => {}
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

fn comment_byte_start(comment: &CommentInfo) -> usize {
    comment
        .span
        .start
        .byte_offset()
        .expect("granit Parser::new_from_str always populates byte offsets")
}

fn spacing_before(buffer: &str, byte_start: usize) -> usize {
    buffer[..byte_start]
        .bytes()
        .rev()
        .take_while(|byte| matches!(byte, b' ' | b'\t'))
        .count()
}
