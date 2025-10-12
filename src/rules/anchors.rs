use std::collections::HashMap;

use crate::config::YamlLintConfig;

pub const ID: &str = "anchors";
pub const MESSAGE_UNDECLARED_ALIAS: &str = "found undeclared alias";
pub const MESSAGE_DUPLICATED_ANCHOR: &str = "found duplicated anchor";
pub const MESSAGE_UNUSED_ANCHOR: &str = "found unused anchor";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    forbid_undeclared_aliases: bool,
    forbid_duplicated_anchors: bool,
    forbid_unused_anchors: bool,
}

impl Config {
    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        let forbid_undeclared_aliases = cfg
            .rule_option(ID, "forbid-undeclared-aliases")
            .and_then(saphyr::YamlOwned::as_bool)
            .unwrap_or(true);
        let forbid_duplicated_anchors = cfg
            .rule_option(ID, "forbid-duplicated-anchors")
            .and_then(saphyr::YamlOwned::as_bool)
            .unwrap_or(false);
        let forbid_unused_anchors = cfg
            .rule_option(ID, "forbid-unused-anchors")
            .and_then(saphyr::YamlOwned::as_bool)
            .unwrap_or(false);

        Self {
            forbid_undeclared_aliases,
            forbid_duplicated_anchors,
            forbid_unused_anchors,
        }
    }

    #[must_use]
    pub const fn new_for_tests(
        forbid_undeclared_aliases: bool,
        forbid_duplicated_anchors: bool,
        forbid_unused_anchors: bool,
    ) -> Self {
        Self {
            forbid_undeclared_aliases,
            forbid_duplicated_anchors,
            forbid_unused_anchors,
        }
    }

    #[must_use]
    pub const fn forbid_undeclared_aliases(&self) -> bool {
        self.forbid_undeclared_aliases
    }

    #[must_use]
    pub const fn forbid_duplicated_anchors(&self) -> bool {
        self.forbid_duplicated_anchors
    }

    #[must_use]
    pub const fn forbid_unused_anchors(&self) -> bool {
        self.forbid_unused_anchors
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
    let mut analyzer = Analyzer::new(buffer, cfg);
    analyzer.run();
    analyzer.into_violations()
}

struct Analyzer<'cfg, 'src> {
    source: &'src str,
    cfg: &'cfg Config,
    doc: DocState,
    block_state: Option<BlockState>,
    in_single_quote: bool,
    in_double_quote: bool,
    violations: Vec<Violation>,
}

impl<'cfg, 'src> Analyzer<'cfg, 'src> {
    fn new(source: &'src str, cfg: &'cfg Config) -> Self {
        Self {
            source,
            cfg,
            doc: DocState::new(),
            block_state: None,
            in_single_quote: false,
            in_double_quote: false,
            violations: Vec::new(),
        }
    }

    fn run(&mut self) {
        if self.source.is_empty() {
            return;
        }

        let mut line_start = 0usize;
        let mut line_number = 1usize;
        while line_start <= self.source.len() {
            let line_end = match self.source[line_start..].find('\n') {
                Some(rel) => line_start + rel + 1,
                None => self.source.len(),
            };
            let mut line = &self.source[line_start..line_end];
            if line.ends_with('\n') {
                line = &line[..line.len() - 1];
            }
            if line.ends_with('\r') {
                line = &line[..line.len() - 1];
            }
            self.process_line(line, line_number);
            if line_end == self.source.len() {
                break;
            }
            line_start = line_end;
            line_number += 1;
        }

        self.finish_doc();
    }

    #[allow(clippy::too_many_lines)]
    fn process_line(&mut self, line: &str, line_number: usize) {
        let chars: Vec<char> = line.chars().collect();
        let indent_count = chars
            .iter()
            .take_while(|ch| matches!(ch, ' ' | '\t'))
            .count();

        if self.handle_block_state(indent_count, line) {
            return;
        }

        if chars.is_empty() {
            return;
        }

        let skip_until = if self.detect_doc_boundary(&chars, indent_count) {
            (indent_count + 3).min(chars.len())
        } else {
            0usize
        };

        let mut idx = 0usize;
        let mut column = 1usize;
        let mut comment_active = false;

        while idx < chars.len() {
            if idx < skip_until {
                idx += 1;
                column += 1;
                continue;
            }

            let ch = chars[idx];
            if comment_active {
                break;
            }

            if self.in_single_quote {
                if ch == '\'' {
                    if idx + 1 < chars.len() && chars[idx + 1] == '\'' {
                        idx += 2;
                        column += 2;
                    } else {
                        self.in_single_quote = false;
                        idx += 1;
                        column += 1;
                    }
                } else {
                    idx += 1;
                    column += 1;
                }
                continue;
            }

            if self.in_double_quote {
                if ch == '"' {
                    let escaped = idx > 0 && chars[idx - 1] == '\\';
                    if !escaped {
                        self.in_double_quote = false;
                    }
                }
                idx += 1;
                column += 1;
                continue;
            }

            match ch {
                '\'' => {
                    self.in_single_quote = true;
                    idx += 1;
                    column += 1;
                }
                '"' => {
                    self.in_double_quote = true;
                    idx += 1;
                    column += 1;
                }
                '#' => {
                    comment_active = true;
                }
                '|' | '>' => {
                    if self.is_block_indicator(&chars, idx, indent_count) {
                        let explicit = parse_explicit_indent(&chars, idx + 1);
                        self.block_state = Some(BlockState {
                            indent_base: indent_count,
                            explicit_indent: explicit,
                            required_indent: None,
                            activate_next_line: true,
                            active: false,
                        });
                    }
                    idx += 1;
                    column += 1;
                }
                '&' => {
                    if let Some((anchor_name, len)) = parse_name(&chars, idx + 1) {
                        self.register_anchor(&anchor_name, line_number, column);
                        idx += len + 1;
                        column += len + 1;
                    } else {
                        idx += 1;
                        column += 1;
                    }
                }
                '*' => {
                    if self.is_alias_indicator(&chars, idx) {
                        if let Some((alias_name, len)) = parse_name(&chars, idx + 1) {
                            self.register_alias(&alias_name, line_number, column);
                            idx += len + 1;
                            column += len + 1;
                        } else {
                            idx += 1;
                            column += 1;
                        }
                    } else {
                        idx += 1;
                        column += 1;
                    }
                }
                _ => {
                    idx += 1;
                    column += 1;
                }
            }
        }
    }

    fn handle_block_state(&mut self, indent_count: usize, line: &str) -> bool {
        if let Some(block) = self.block_state.as_mut() {
            if block.activate_next_line {
                block.activate_next_line = false;
                block.active = true;
                if line.trim().is_empty() {
                    return true;
                }
            }

            if line.trim().is_empty() {
                return true;
            }
            let explicit_indent = block.explicit_indent.map(|value| block.indent_base + value);
            let required_indent = match (explicit_indent, block.required_indent) {
                (Some(explicit), _) => explicit,
                (None, Some(required)) => required,
                (None, None) => {
                    if indent_count > block.indent_base {
                        block.required_indent = Some(indent_count);
                        indent_count
                    } else {
                        self.block_state = None;
                        return false;
                    }
                }
            };
            let stays_in_block = indent_count >= required_indent;
            if !stays_in_block {
                self.block_state = None;
            }
            return stays_in_block;
        }
        false
    }

    fn detect_doc_boundary(&mut self, chars: &[char], indent_count: usize) -> bool {
        if self.in_single_quote || self.in_double_quote {
            return false;
        }
        if chars.len() < indent_count + 3 {
            return false;
        }

        let candidate = &chars[indent_count..];
        let marker = &candidate[..3];
        let is_boundary_marker = matches!(marker, ['-', '-', '-'] | ['.', '.', '.']);
        if is_boundary_marker
            && (candidate.len() == 3 || candidate.get(3).is_some_and(|ch| ch.is_whitespace()))
        {
            self.finish_doc();
            self.reset_block_and_quotes();
            return true;
        }
        false
    }

    const fn reset_block_and_quotes(&mut self) {
        self.block_state = None;
        self.in_single_quote = false;
        self.in_double_quote = false;
    }

    fn register_anchor(&mut self, name: &str, line: usize, column: usize) {
        let is_duplicate = self.doc.add_anchor(name.to_string(), line, column);
        if self.cfg.forbid_duplicated_anchors() && is_duplicate {
            self.violations.push(Violation {
                line,
                column,
                message: format!("{MESSAGE_DUPLICATED_ANCHOR} \"{name}\""),
            });
        }
    }

    fn register_alias(&mut self, name: &str, line: usize, column: usize) {
        if self.doc.mark_alias(name) {
            return;
        }
        if self.cfg.forbid_undeclared_aliases() {
            self.violations.push(Violation {
                line,
                column,
                message: format!("{MESSAGE_UNDECLARED_ALIAS} \"{name}\""),
            });
        }
    }

    fn finish_doc(&mut self) {
        if self.cfg.forbid_unused_anchors() {
            for anchor in &self.doc.anchors {
                if !anchor.used {
                    self.violations.push(Violation {
                        line: anchor.line,
                        column: anchor.column,
                        message: format!("{MESSAGE_UNUSED_ANCHOR} \"{}\"", anchor.name),
                    });
                }
            }
        }
        self.doc = DocState::new();
    }

    fn into_violations(self) -> Vec<Violation> {
        self.violations
    }

    fn is_block_indicator(&self, chars: &[char], idx: usize, indent_count: usize) -> bool {
        debug_assert!(!self.in_single_quote && !self.in_double_quote);
        debug_assert!(idx >= indent_count);
        let prefix = &chars[..idx];
        let last_non_ws = prefix.iter().rev().find(|ch| !ch.is_whitespace());
        matches!(last_non_ws, None | Some(':' | '-' | '?'))
    }

    fn is_alias_indicator(&self, chars: &[char], idx: usize) -> bool {
        debug_assert!(!self.in_single_quote && !self.in_double_quote);
        let prev_non_ws = chars[..idx]
            .iter()
            .rev()
            .find(|ch| !matches!(ch, ' ' | '\t'));
        prev_non_ws.is_none_or(|prev| matches!(prev, ':' | '-' | '[' | '{' | ',' | '?'))
    }
}

struct DocState {
    anchors: Vec<AnchorRecord>,
    name_to_indices: HashMap<String, Vec<usize>>,
}

impl DocState {
    fn new() -> Self {
        Self {
            anchors: Vec::new(),
            name_to_indices: HashMap::new(),
        }
    }

    fn add_anchor(&mut self, name: String, line: usize, column: usize) -> bool {
        let entry_indices = self.name_to_indices.entry(name.clone()).or_default();
        let duplicate = !entry_indices.is_empty();
        let index = self.anchors.len();
        entry_indices.push(index);
        self.anchors.push(AnchorRecord {
            name,
            line,
            column,
            used: false,
        });
        duplicate
    }

    fn mark_alias(&mut self, name: &str) -> bool {
        let Some(indices) = self.name_to_indices.get(name) else {
            return false;
        };
        let last_index = *indices
            .last()
            .expect("anchor indices should contain at least one entry");
        let anchor = self
            .anchors
            .get_mut(last_index)
            .expect("anchor record must exist for referenced name");
        anchor.used = true;
        true
    }
}

struct AnchorRecord {
    name: String,
    line: usize,
    column: usize,
    used: bool,
}

struct BlockState {
    indent_base: usize,
    explicit_indent: Option<usize>,
    required_indent: Option<usize>,
    activate_next_line: bool,
    active: bool,
}

fn parse_name(chars: &[char], start: usize) -> Option<(String, usize)> {
    if start >= chars.len() {
        return None;
    }
    let mut idx = start;
    while idx < chars.len() && is_name_char(chars[idx]) {
        idx += 1;
    }
    if idx == start {
        return None;
    }
    let name: String = chars[start..idx].iter().collect();
    Some((name, idx - start))
}

const fn is_name_char(ch: char) -> bool {
    !matches!(
        ch,
        ' ' | '\t'
            | '\r'
            | '\n'
            | ','
            | '['
            | ']'
            | '{'
            | '}'
            | '*'
            | '&'
            | '#'
            | '!'
            | '|'
            | '>'
            | '\''
            | '"'
            | '%'
            | '@'
            | ':'
            | '?'
    )
}

fn parse_explicit_indent(chars: &[char], mut idx: usize) -> Option<usize> {
    let mut indent = None;
    while idx < chars.len() {
        match chars[idx] {
            '+' | '-' => idx += 1,
            ch if ch.is_ascii_digit() => {
                let mut val = 0usize;
                while idx < chars.len() && chars[idx].is_ascii_digit() {
                    val = val
                        .saturating_mul(10)
                        .saturating_add((chars[idx] as u8 - b'0') as usize);
                    idx += 1;
                }
                if val > 0 {
                    indent = Some(val);
                }
                break;
            }
            ' ' | '\t' => idx += 1,
            _ => break,
        }
    }
    indent
}
