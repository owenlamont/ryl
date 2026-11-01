//! Inline rule-disable comment directives.
//!
//! Mirrors yamllint's `# yamllint disable` / `enable` / `disable-line` /
//! `disable-file` directives (`yamllint/linter.py`), with a preferred `# ryl ...`
//! spelling that may also share a comment with other `#` text.
//! [`crate::lint::lint_str`] filters every diagnostic through
//! [`Directives::is_disabled`], and `--fix` keeps disabled lines untouched via
//! [`Directives::reconcile`]. A first-line [`disables_file`] directive skips the whole
//! buffer (a file or an embedded Markdown region).

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;
use ryl_yaml_parser::Placement;

use crate::rules::ALL_RULE_IDS;
use crate::rules::support::line_syntax::{
    split_lines_inclusive, split_lines_preserve_endings,
};

// The patterns match the parser's comment payload (text after the leading `#`); the
// leading space is the one between `#` and the keyword, keeping parity with yamllint's
// `^# ...`.
static DISABLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^ (?:yamllint|ryl) disable(?: rule:\S+)*\s*$").unwrap()
});
static ENABLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^ (?:yamllint|ryl) enable(?: rule:\S+)*\s*$").unwrap()
});
static DISABLE_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^ (?:yamllint|ryl) disable-line(?: rule:\S+)*\s*$").unwrap()
});
static SEGMENT_BREAK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[ \t]#").unwrap());
static RULE_TOKEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"rule:(\S+)").unwrap());
// Matched on the raw first line (including `#`), more lenient than the other directives,
// mirroring yamllint's `^#\s*yamllint disable-file`.
static DISABLE_FILE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^#\s*(?:yamllint|ryl) disable-file\s*$").unwrap());

/// Whether the buffer's first line is a `disable-file` directive, matching yamllint, or a
/// Jinja renderer header; either skips the whole buffer (no diagnostics, not even syntax
/// errors, no `--fix`). For embedded Markdown the buffer is one region, so this disables
/// the region opening with it.
#[must_use]
pub fn disables_file(buffer: &str) -> bool {
    // First line on the YAML 1.2 break set, so a bare-`\r`-terminated directive is seen.
    let first_line = split_lines_preserve_endings(buffer)
        .next()
        .map_or("", |(_, content, _)| content);
    starts_with_jinja_header(first_line)
        || first_line.contains("disable-file")
            && (DISABLE_FILE.is_match(first_line)
                || first_line.strip_prefix('#').is_some_and(|payload| {
                    SEGMENT_BREAK
                        .split(payload)
                        .any(|segment| segment.trim_end() == " ryl disable-file")
                }))
}

/// Whether decoded `text` opens with a Salt renderer header that runs Jinja
/// (`#!jinja|yaml`, `#!yaml_jinja`), parsed as `template_shebang` in Salt's
/// `salt/template.py` does: a single-token pipe may name a combined renderer.
#[must_use]
pub fn starts_with_jinja_header(text: &str) -> bool {
    let Some(rest) = text
        .strip_prefix("#!")
        .filter(|rest| !rest.starts_with('/'))
    else {
        return false;
    };
    let pipe = rest
        .split(['\n', '\r'])
        .next()
        .unwrap_or_default()
        .trim_end();
    ["yaml_jinja", "json_jinja", "yamlex_jinja"].contains(&pipe)
        || pipe
            .split('|')
            .any(|part| part.trim().split(' ').next() == Some("jinja"))
}

enum Action {
    Disable,
    Enable,
    DisableLine,
}

struct Parsed {
    action: Action,
    /// `None` means "all rules" (a bare directive with no `rule:` token).
    rules: Option<Vec<&'static str>>,
}

fn parse_comment(text: &str) -> impl Iterator<Item = Parsed> + '_ {
    let shared = SEGMENT_BREAK.is_match(text);
    SEGMENT_BREAK
        .split(text)
        .filter(move |segment| !shared || segment.starts_with(" ryl "))
        .filter_map(parse_segment)
}

fn parse_segment(text: &str) -> Option<Parsed> {
    // Compiling the directive regexes costs more than formatting a small file, and the
    // formatter asks about every comment it folds around.
    if ![" yamllint ", " ryl "]
        .iter()
        .any(|prefix| text.starts_with(prefix))
    {
        return None;
    }
    let action = if DISABLE_LINE.is_match(text) {
        Action::DisableLine
    } else if DISABLE.is_match(text) {
        Action::Disable
    } else if ENABLE.is_match(text) {
        Action::Enable
    } else {
        return None;
    };
    let rules = if RULE_TOKEN.is_match(text) {
        Some(
            RULE_TOKEN
                .captures_iter(text)
                .filter_map(|caps| resolve_rule(&caps[1]))
                .collect(),
        )
    } else {
        None
    };
    Some(Parsed { action, rules })
}

/// `Some(true)` when a comment payload holds only `disable-line` directives, `Some(false)`
/// when it holds a `disable`/`enable`.
pub(crate) fn directive_scope(text: &str) -> Option<bool> {
    parse_comment(text)
        .map(|parsed| matches!(parsed.action, Action::DisableLine))
        .reduce(|a, b| a && b)
}

/// Resolve a `rule:` token to the canonical rule id; an unknown id resolves to nothing
/// (inert either way), matching yamllint's `if id in all_rules` guard.
fn resolve_rule(token: &str) -> Option<&'static str> {
    ALL_RULE_IDS.into_iter().find(|&id| id == token)
}

/// A resolved `per-line-ignores` entry to layer onto a buffer's directives: suppress
/// `rules` on every line whose content matches `regex` (all lines if `regex` is `None`).
/// `rules` follows [`insert_rules`]' convention (`None` means every rule). The regex
/// matches line content excluding its break, so the user's `$` anchors to the line.
#[derive(Clone, Copy)]
pub struct PerLineRuleApply<'a> {
    pub regex: Option<&'a Regex>,
    pub rules: Option<&'a [&'static str]>,
}

/// Resolved disable state for one buffer.
#[derive(Clone, Default)]
pub struct Directives {
    /// `(line, rules disabled from that line onward)`, in ascending line order.
    block_snapshots: Vec<(usize, HashSet<&'static str>)>,
    line_disabled: HashMap<usize, HashSet<&'static str>>,
    /// Rules disabled on *every* line, from a `per-line-ignores` entry with a matching
    /// `path` but no `regex`. Recorded once here (not per line) so a file-wide suppression
    /// costs O(rules), not O(lines x rules).
    file_wide: HashSet<&'static str>,
}

impl Directives {
    /// Parse all directives in `buffer`. A buffer with no directive keyword skips the
    /// comment scan and returns an empty set, so directive-less files pay no parsing cost.
    #[must_use]
    pub fn parse(buffer: &str) -> Self {
        Self::parse_with_analysis(
            crate::text_version::TextVersion::new(buffer).analysis(),
        )
    }

    pub(crate) fn parse_with_analysis(
        input: crate::text_version::Analysis<'_>,
    ) -> Self {
        input.directives().clone()
    }

    pub(crate) fn parse_uncached(input: crate::text_version::Analysis<'_>) -> Self {
        let buffer = input.text();
        if !buffer.contains("yamllint ") && !buffer.contains("ryl ") {
            return Self::default();
        }

        let comments =
            crate::rules::support::comments_scan::collect_comments_with_analysis(input);
        Self::from_comments(comments.iter().map(|comment| {
            (
                comment.span.start.line(),
                comment.placement,
                comment.text.as_str(),
            )
        }))
    }

    pub(crate) fn from_comments<'a>(
        comments: impl Iterator<Item = (usize, Placement, &'a str)>,
    ) -> Self {
        let mut block: HashSet<&'static str> = HashSet::new();
        let mut directives = Self::default();
        for (line, placement, text) in comments {
            let disable_line_target = if placement == Placement::Right {
                line
            } else {
                line + 1
            };
            for parsed in parse_comment(text) {
                match parsed.action {
                    Action::Disable => {
                        insert_rules(&mut block, parsed.rules.as_deref());
                        directives.block_snapshots.push((line, block.clone()));
                    }
                    Action::Enable => {
                        remove_rules(&mut block, parsed.rules.as_deref());
                        directives.block_snapshots.push((line, block.clone()));
                    }
                    Action::DisableLine => {
                        insert_rules(
                            directives
                                .line_disabled
                                .entry(disable_line_target)
                                .or_default(),
                            parsed.rules.as_deref(),
                        );
                    }
                }
            }
        }
        directives
    }

    pub(crate) fn parse_projected(
        input: crate::text_version::Analysis<'_>,
        frame: &crate::format::layout::frame::Frame,
        lines: &[crate::format::layout::VirtualLine<'_>],
    ) -> Self {
        let comments =
            crate::rules::support::comments_scan::collect_comments_with_analysis(input);
        Self::from_comments(comments.iter().map(|comment| {
            let at = crate::rules::support::span_utils::marker_byte_offset(
                comment.span.start,
            )
            .get();
            let position = frame.source_position(input, at, false);
            let line = &lines[position.line];
            let placement = if position.column > line.prefix {
                Placement::Right
            } else {
                Placement::Above
            };
            (position.line + 1, placement, comment.text.as_str())
        }))
    }

    /// Parse inline directives, then layer config `per-line-ignores` on top as a virtual
    /// `disable-line`, so lint filtering and `--fix` reconcile treat config and inline
    /// suppression identically. A `regex` entry suppresses its rules only on matching
    /// lines, numbered via `Analysis::lines` (same YAML 1.2 break set as the parser, so
    /// a match on line *n* lands on a diagnostic at line *n*); a no-`regex` entry is
    /// file-wide, recorded once in `file_wide`.
    #[must_use]
    pub fn parse_with_per_line(
        buffer: &str,
        per_line: &[PerLineRuleApply<'_>],
    ) -> Self {
        Self::parse_per_line_with_analysis(
            crate::text_version::TextVersion::new(buffer).analysis(),
            per_line,
        )
    }

    pub(crate) fn parse_per_line_with_analysis(
        input: crate::text_version::Analysis<'_>,
        per_line: &[PerLineRuleApply<'_>],
    ) -> Self {
        let mut directives = Self::parse_with_analysis(input);
        if per_line.is_empty() {
            return directives;
        }
        let mut has_regex_entry = false;
        for entry in per_line {
            if entry.regex.is_none() {
                insert_rules(&mut directives.file_wide, entry.rules);
            } else {
                has_regex_entry = true;
            }
        }
        if !has_regex_entry {
            return directives;
        }
        for (index, content, _) in input.lines() {
            for entry in per_line {
                if entry.regex.is_some_and(|regex| regex.is_match(content)) {
                    insert_rules(
                        directives.line_disabled.entry(index + 1).or_default(),
                        entry.rules,
                    );
                }
            }
        }
        directives
    }

    /// Whether `rule` is disabled on `line` (1-based).
    #[must_use]
    pub fn is_disabled(&self, rule: &str, line: usize) -> bool {
        self.file_wide.contains(rule)
            || self
                .block_snapshots
                .iter()
                .rev()
                .find(|(at, _)| *at <= line)
                .is_some_and(|(_, set)| set.contains(rule))
            || self
                .line_disabled
                .get(&line)
                .is_some_and(|set| set.contains(rule))
    }

    /// Whether `rule` is disabled anywhere, used to skip reconciliation in `--fix`.
    #[must_use]
    pub fn disables_any(&self, rule: &str) -> bool {
        self.file_wide.contains(rule)
            || self.line_disabled.values().any(|set| set.contains(rule))
            || self
                .block_snapshots
                .iter()
                .any(|(_, set)| set.contains(rule))
    }

    /// Rebuild `after` so a line `rule`'s fixer changed while disabled keeps its `before`
    /// text.
    ///
    /// Precondition: each fixer is **pure replace** (line count unchanged) **xor** **pure
    /// insert/delete** (count changed) in a single pass, never a mix. Every current
    /// safe-fix rule satisfies this. The equal-length path assumes positional replacement,
    /// so a fixer that reorders lines must leave every line disabled for its rule untouched.
    #[must_use]
    pub fn reconcile(&self, rule: &str, before: &str, after: &str) -> String {
        let before_lines: Vec<&str> = split_lines_inclusive(before).collect();
        let after_lines: Vec<&str> = split_lines_inclusive(after).collect();
        reconcile_lines(&before_lines, &after_lines, |index| {
            self.is_disabled(rule, index + 1)
        })
    }
}

fn insert_rules(set: &mut HashSet<&'static str>, rules: Option<&[&'static str]>) {
    match rules {
        None => set.extend(ALL_RULE_IDS),
        Some(ids) => set.extend(ids.iter().copied()),
    }
}

fn remove_rules(set: &mut HashSet<&'static str>, rules: Option<&[&'static str]>) {
    match rules {
        None => set.clear(),
        Some(ids) => set.retain(|id| !ids.contains(id)),
    }
}

fn reconcile_lines(
    before: &[&str],
    after: &[&str],
    disabled: impl Fn(usize) -> bool,
) -> String {
    let mut out = String::new();
    if before.len() == after.len() {
        for (index, (old, new)) in before.iter().zip(after).enumerate() {
            out.push_str(if disabled(index) { old } else { new });
        }
        return out;
    }

    let inserting = after.len() > before.len();
    let (mut i, mut j) = (0, 0);
    while i < before.len() && j < after.len() {
        if before[i] == after[j] {
            out.push_str(after[j]);
            i += 1;
            j += 1;
        } else if inserting {
            if !disabled(i) {
                out.push_str(after[j]);
            }
            j += 1;
        } else {
            if disabled(i) {
                out.push_str(before[i]);
            }
            i += 1;
        }
    }
    while j < after.len() {
        if before.is_empty() || !disabled(before.len() - 1) {
            out.push_str(after[j]);
        }
        j += 1;
    }
    while i < before.len() {
        if disabled(i) {
            out.push_str(before[i]);
        }
        i += 1;
    }
    out
}
