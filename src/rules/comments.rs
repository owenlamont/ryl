use saphyr::YamlOwned;

use crate::config::YamlLintConfig;

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
        let require_starting_space = cfg
            .rule_option(ID, "require-starting-space")
            .and_then(YamlOwned::as_bool)
            .unwrap_or(true);

        let ignore_shebangs = cfg
            .rule_option(ID, "ignore-shebangs")
            .and_then(YamlOwned::as_bool)
            .unwrap_or(true);

        let min_spaces_value = cfg
            .rule_option(ID, "min-spaces-from-content")
            .and_then(YamlOwned::as_integer)
            .unwrap_or(2);

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

#[must_use]
pub fn check(buffer: &str, cfg: &Config) -> Vec<Violation> {
    let mut violations = Vec::new();
    let mut quote_state = QuoteState::default();

    for (line_idx, line) in buffer.lines().enumerate() {
        let Some(comment_start) = find_comment_start(line, &mut quote_state) else {
            continue;
        };

        if let Some(required) = cfg.min_spaces_from_content()
            && is_inline_comment(line, comment_start)
            && inline_spacing_width(line, comment_start) < required
        {
            violations.push(Violation {
                line: line_idx + 1,
                column: column_at(line, comment_start),
                message: format!("too few spaces before comment: expected {required}"),
            });
        }

        if !cfg.require_starting_space() {
            continue;
        }

        let after_hash_idx = comment_start + skip_hashes(&line[comment_start..]);
        if after_hash_idx >= line.len() {
            continue;
        }

        let next_char = line[after_hash_idx..].chars().next().unwrap_or(' ');

        if cfg.ignore_shebangs() && line_idx == 0 && comment_start == 0 && next_char == '!' {
            continue;
        }

        if next_char != ' ' {
            violations.push(Violation {
                line: line_idx + 1,
                column: column_at(line, after_hash_idx),
                message: "missing starting space in comment".to_string(),
            });
        }
    }

    violations
}

#[derive(Debug, Default, Clone, Copy)]
struct QuoteState {
    in_single: bool,
    in_double: bool,
    escaped: bool,
}

fn find_comment_start(line: &str, state: &mut QuoteState) -> Option<usize> {
    for (idx, ch) in line.char_indices() {
        if ch == '\\' && !state.in_single {
            state.escaped = !state.escaped;
            continue;
        }

        if state.escaped {
            state.escaped = false;
            continue;
        }

        match ch {
            '\'' if !state.in_double => {
                state.in_single = !state.in_single;
            }
            '"' if !state.in_single => {
                state.in_double = !state.in_double;
            }
            '#' if !state.in_single && !state.in_double => {
                return Some(idx);
            }
            _ => {}
        }
    }

    state.escaped = false;
    None
}

fn is_inline_comment(line: &str, comment_start: usize) -> bool {
    !line[..comment_start].trim().is_empty()
}

fn inline_spacing_width(line: &str, comment_start: usize) -> usize {
    line[..comment_start]
        .chars()
        .rev()
        .take_while(|ch| ch.is_whitespace())
        .count()
}

fn skip_hashes(slice: &str) -> usize {
    slice
        .chars()
        .take_while(|ch| *ch == '#')
        .map(char::len_utf8)
        .sum()
}

fn column_at(line: &str, byte_idx: usize) -> usize {
    line[..byte_idx].chars().count() + 1
}
