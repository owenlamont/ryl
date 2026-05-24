use crate::config::YamlLintConfig;
use crate::rules::support::line_syntax::{
    BlockScalarTracker, is_at_value_position, leading_whitespace_width,
    split_lines_preserve_endings,
};

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

#[must_use]
pub fn check(buffer: &str, cfg: &Config) -> Vec<Violation> {
    let mut violations = Vec::new();
    let mut quote_state = QuoteState::default();
    let mut block_tracker = BlockScalarTracker::default();

    for (line_idx, line) in buffer.lines().enumerate() {
        let indent = leading_whitespace_width(line);
        let content = &line[indent..];

        if block_tracker.consume_line(indent, content) {
            continue;
        }

        let Some(comment_start) = find_comment_start(line, &mut quote_state) else {
            block_tracker.observe_indicator(indent, content);
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

        if cfg.ignore_shebangs()
            && line_idx == 0
            && comment_start == 0
            && next_char == '!'
        {
            continue;
        }

        if next_char != ' ' {
            violations.push(Violation {
                line: line_idx + 1,
                column: column_at(line, after_hash_idx),
                message: "missing starting space in comment".to_string(),
            });
        }

        block_tracker.observe_indicator(indent, content);
    }

    violations
}

#[must_use]
pub fn fix(buffer: &str, cfg: &Config) -> Option<String> {
    let mut quote_state = QuoteState::default();
    let mut block_tracker = BlockScalarTracker::default();
    let mut output = String::with_capacity(buffer.len());
    let mut changed = false;

    for (line_idx, raw_line, ending) in split_lines_preserve_endings(buffer) {
        let indent = leading_whitespace_width(raw_line);
        let content = &raw_line[indent..];

        let consumed = block_tracker.consume_line(indent, content);
        let updated_line = if consumed {
            raw_line.to_string()
        } else if let Some(comment_start) =
            find_comment_start(raw_line, &mut quote_state)
        {
            fix_comment_line(raw_line, line_idx, comment_start, cfg, &mut changed)
        } else {
            raw_line.to_string()
        };

        if !consumed {
            let indent = leading_whitespace_width(&updated_line);
            let content = &updated_line[indent..];
            block_tracker.observe_indicator(indent, content);
        }

        output.push_str(&updated_line);
        output.push_str(ending);
    }

    changed.then_some(output)
}

#[derive(Debug, Default, Clone, Copy)]
struct QuoteState {
    in_single: bool,
    in_double: bool,
    escaped: bool,
    flow_depth: u32,
}

fn find_comment_start(line: &str, state: &mut QuoteState) -> Option<usize> {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        let (byte_idx, ch) = chars[i];

        if ch == '\\' && !state.in_single {
            state.escaped = !state.escaped;
            i += 1;
            continue;
        }

        if state.escaped {
            state.escaped = false;
            i += 1;
            continue;
        }

        match ch {
            '\'' if !state.in_double => {
                if state.in_single {
                    if chars.get(i + 1).map(|(_, c)| *c) == Some('\'') {
                        i += 2;
                        continue;
                    }
                    state.in_single = false;
                } else if is_at_value_position(&chars, i, state.flow_depth) {
                    state.in_single = true;
                }
            }
            '"' if !state.in_single => {
                if state.in_double || is_at_value_position(&chars, i, state.flow_depth)
                {
                    state.in_double = !state.in_double;
                }
            }
            '[' | '{' if !state.in_single && !state.in_double => {
                state.flow_depth = state.flow_depth.saturating_add(1);
            }
            ']' | '}' if !state.in_single && !state.in_double => {
                state.flow_depth = state.flow_depth.saturating_sub(1);
            }
            '#' if !state.in_single && !state.in_double => {
                if is_comment_position(line, byte_idx) {
                    return Some(byte_idx);
                }
            }
            _ => {}
        }
        i += 1;
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

fn is_comment_position(line: &str, idx: usize) -> bool {
    line[..idx].chars().last().is_none_or(char::is_whitespace)
}

fn fix_comment_line(
    line: &str,
    line_idx: usize,
    comment_start: usize,
    cfg: &Config,
    changed: &mut bool,
) -> String {
    let mut line = line.to_string();
    let mut inserted_before_comment = 0usize;

    if let Some(required) = cfg.min_spaces_from_content()
        && is_inline_comment(&line, comment_start)
    {
        let spacing = inline_spacing_width(&line, comment_start);
        if spacing < required {
            inserted_before_comment = required - spacing;
            line.insert_str(comment_start, &" ".repeat(inserted_before_comment));
            *changed = true;
        }
    }

    if !cfg.require_starting_space() {
        return line;
    }

    let comment_start = comment_start + inserted_before_comment;
    let after_hash_idx = comment_start + skip_hashes(&line[comment_start..]);
    if after_hash_idx >= line.len() {
        return line;
    }

    let next_char = line[after_hash_idx..].chars().next().unwrap_or(' ');
    if cfg.ignore_shebangs() && line_idx == 0 && comment_start == 0 && next_char == '!'
    {
        return line;
    }

    if next_char != ' ' {
        line.insert(after_hash_idx, ' ');
        *changed = true;
    }

    line
}
