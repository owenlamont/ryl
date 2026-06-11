//! `unicode-line-breaks` rule &mdash; flags raw NEL / LS / PS characters in the
//! source (issue #253).
//!
//! YAML 1.1 treated a broad Unicode set as line breaks, including NEL (U+0085),
//! LINE SEPARATOR (U+2028) and PARAGRAPH SEPARATOR (U+2029). YAML 1.2 narrowed
//! line breaks to just LF and CR, and the 1.2.2 changes page records that these
//! three "are no longer considered line-break characters." ryl targets YAML 1.2,
//! so a raw occurrence is a portability trap: a 1.1 parser splits the line where a
//! 1.2 parser keeps the character as ordinary scalar content, silently changing
//! the parsed structure. The characters are also invisible in most editors, so a
//! stray one (pasted from a word processor, PDF or web page) is hard to spot.
//!
//! The rule scans the decoded source and reports every raw occurrence regardless
//! of context. The three characters each have a dedicated YAML escape (§5.7) that
//! includes them intentionally and visibly inside a double-quoted scalar, so the
//! diagnostic suggests that escape (`\N` / `\L` / `\P`).
//!
//! There is no safe `--fix`: the escape is only valid inside a double-quoted
//! scalar, so rewriting a plain/single-quoted scalar, comment or block scalar
//! would require changing the quoting style or guessing intent (see AGENTS.md
//! "Rules Without A Safe `--fix`").
//!
//! Sources: YAML 1.2.2 changes page; YAML 1.2.2 spec §5.1 (character set), §5.4
//! (line-break characters), §5.7 (escaped characters).

pub const ID: &str = "unicode-line-breaks";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

/// Report every raw NEL / LS / PS character with a 1-based line/column. Line counting
/// advances on `\n`, `\r\n`, and a bare `\r`; the flagged NEL/LS/PS chars are not YAML
/// 1.2 breaks, so they never advance the counter.
#[must_use]
pub fn check(buffer: &str) -> Vec<Violation> {
    let mut violations = Vec::new();
    let mut line = 1usize;
    let mut column = 1usize;
    let mut chars = buffer.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\n' => {
                line += 1;
                column = 1;
            }
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                line += 1;
                column = 1;
            }
            _ => {
                if let Some((name, escape)) = classify(ch) {
                    violations.push(Violation {
                        line,
                        column,
                        message: format!(
                            "forbidden raw {name} U+{:04X}; escape as \"{escape}\" in a double-quoted scalar",
                            ch as u32
                        ),
                    });
                }
                column += 1;
            }
        }
    }
    violations
}

fn classify(ch: char) -> Option<(&'static str, &'static str)> {
    match ch {
        '\u{85}' => Some(("next line", "\\N")),
        '\u{2028}' => Some(("line separator", "\\L")),
        '\u{2029}' => Some(("paragraph separator", "\\P")),
        _ => None,
    }
}
