use crate::config::YamlLintConfig;

pub const ID: &str = "hyphens";
pub const MESSAGE: &str = "too many spaces after hyphen";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    max_spaces_after: i64,
}

impl Config {
    const DEFAULT_MAX: i64 = 1;

    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        let max_spaces_after = cfg
            .rule_option(ID, "max-spaces-after")
            .and_then(saphyr::YamlOwned::as_integer)
            .unwrap_or(Self::DEFAULT_MAX);
        Self { max_spaces_after }
    }

    #[must_use]
    pub const fn new_for_tests(max_spaces_after: i64) -> Self {
        Self { max_spaces_after }
    }

    #[must_use]
    pub const fn max_spaces_after(&self) -> i64 {
        self.max_spaces_after
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
}

#[must_use]
pub fn check(buffer: &str, cfg: &Config) -> Vec<Violation> {
    let mut violations = Vec::new();

    for (idx, raw_line) in buffer.lines().enumerate() {
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }

        let chars = line.char_indices();
        let mut indent_chars = 0usize;
        let mut hyphen_byte = None;

        for (byte_idx, ch) in chars {
            match ch {
                ' ' | '\t' => {
                    indent_chars += 1;
                }
                '-' => {
                    hyphen_byte = Some(byte_idx);
                    break;
                }
                _ => break,
            }
        }

        let Some(hyphen_pos) = hyphen_byte else {
            continue;
        };

        let mut offset = hyphen_pos + 1;
        let mut spaces_after = 0usize;

        while let Some(ch) = line[offset..].chars().next() {
            if matches!(ch, ' ' | '\t') {
                spaces_after += 1;
                offset += ch.len_utf8();
            } else {
                break;
            }
        }

        if offset >= line.len() {
            continue;
        }

        let next_byte = line.as_bytes()[offset];
        if next_byte == b'#' {
            continue;
        }

        let spaces_count = i64::try_from(spaces_after).unwrap_or(i64::MAX);

        if spaces_count > cfg.max_spaces_after {
            let column = indent_chars + 1 + spaces_after;
            violations.push(Violation {
                line: idx + 1,
                column,
            });
        }
    }

    violations
}
