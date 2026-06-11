//! Inline rule-disable comment directives.
//!
//! Mirrors yamllint's `# yamllint disable` / `enable` / `disable-line` /
//! `disable-file` directives (`yamllint/linter.py`) and adds a preferred `# ryl …`
//! spelling kept in lockstep with the grammar. The lint engine
//! ([`crate::lint::lint_str`]) filters every rule's diagnostics through
//! [`Directives::is_disabled`], and `--fix` ([`crate::fix`]) keeps disabled lines
//! untouched via [`Directives::reconcile`], so behaviour is uniform across all rules.
//! A first-line [`disables_file`] directive skips the whole buffer (a file, or an
//! embedded Markdown region) for both linting and `--fix`.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use granit_parser::Placement;
use regex::Regex;

use crate::rules::ALL_RULE_IDS;
use crate::rules::support::comments_scan::collect_comments;
use crate::rules::support::line_syntax::{
    line_contents, split_lines_inclusive, split_lines_preserve_endings,
};

// The patterns match granit's comment payload, which is the text after the leading
// `#` (excluding the line break); the space the patterns start with is the one
// between `#` and the keyword. This keeps exact parity with yamllint's `^# …` regexes.
static DISABLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^ (?:yamllint|ryl) disable(?: rule:\S+)*\s*$").unwrap()
});
static ENABLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^ (?:yamllint|ryl) enable(?: rule:\S+)*\s*$").unwrap()
});
static DISABLE_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^ (?:yamllint|ryl) disable-line(?: rule:\S+)*\s*$").unwrap()
});
static RULE_TOKEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"rule:(\S+)").unwrap());
// `disable-file` is matched on the raw first line (including `#`) and is more lenient
// than the other directives, exactly mirroring yamllint's `^#\s*yamllint disable-file`.
static DISABLE_FILE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^#\s*(?:yamllint|ryl) disable-file\s*$").unwrap());

/// Whether the buffer's first line is a `disable-file` directive. The buffer is then
/// skipped entirely &mdash; no diagnostics (not even syntax errors) and no `--fix`
/// rewrites &mdash; matching yamllint (`yamllint/linter.py`). For embedded Markdown
/// the buffer is one region, so this disables the region that opens with it.
#[must_use]
pub fn disables_file(buffer: &str) -> bool {
    // First line on the YAML 1.2 break set, so a bare-`\r`-terminated directive is seen.
    let first_line = split_lines_preserve_endings(buffer)
        .next()
        .map_or("", |(_, content, _)| content);
    DISABLE_FILE.is_match(first_line)
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

fn parse_comment(text: &str) -> Option<Parsed> {
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

/// Resolve a `rule:` token to the canonical rule id. Unknown ids resolve to nothing,
/// matching yamllint's `if id in all_rules` guard (they are inert either way).
fn resolve_rule(token: &str) -> Option<&'static str> {
    ALL_RULE_IDS.into_iter().find(|&id| id == token)
}

/// Directive state for one buffer: block `disable`/`enable` snapshots plus the set of
/// A resolved `per-line-ignores` entry to layer onto a buffer's directives: suppress
/// `rules` on every line whose content matches `regex` (all lines if `regex` is None).
/// `rules` follows [`insert_rules`]' convention &mdash; `None` means every rule (the
/// `ALL` selector). `config` builds these per file (after path-glob filtering); the
/// regex matches the line content excluding its break, so the user's `$` anchors to
/// the line.
pub struct PerLineRuleApply<'a> {
    pub regex: Option<&'a Regex>,
    pub rules: Option<&'a [&'static str]>,
}

/// rules each line disables via `disable-line`.
#[derive(Default)]
pub struct Directives {
    /// `(line, rules disabled from that line onward)`, in ascending line order.
    block_snapshots: Vec<(usize, HashSet<&'static str>)>,
    line_disabled: HashMap<usize, HashSet<&'static str>>,
    /// Rules disabled on *every* line, from a `per-line-ignores` entry with a matching
    /// `path` but no `regex`. Recorded once here (not materialized per line) so a
    /// file-wide suppression on a large file costs O(rules), not O(lines × rules).
    file_wide: HashSet<&'static str>,
}

impl Directives {
    /// Parse all directives in `buffer`. Buffers with no directive keyword skip the
    /// comment scan entirely and return an empty set, so directive-less files pay no
    /// parsing cost.
    #[must_use]
    pub fn parse(buffer: &str) -> Self {
        if !buffer.contains("yamllint ") && !buffer.contains("ryl ") {
            return Self::default();
        }

        let mut block: HashSet<&'static str> = HashSet::new();
        let mut directives = Self::default();
        for comment in collect_comments(buffer) {
            let Some(parsed) = parse_comment(&comment.text) else {
                continue;
            };
            let line = comment.span.start.line();
            match parsed.action {
                Action::Disable => {
                    insert_rules(&mut block, parsed.rules.as_deref());
                    directives.block_snapshots.push((line, block.clone()));
                }
                Action::Enable => {
                    match parsed.rules.as_deref() {
                        None => block.clear(),
                        Some(ids) => {
                            for id in ids {
                                block.remove(id);
                            }
                        }
                    }
                    directives.block_snapshots.push((line, block.clone()));
                }
                Action::DisableLine => {
                    let target = if comment.placement == Placement::Right {
                        line
                    } else {
                        line + 1
                    };
                    insert_rules(
                        directives.line_disabled.entry(target).or_default(),
                        parsed.rules.as_deref(),
                    );
                }
            }
        }
        directives
    }

    /// Parse inline directives, then layer config-driven `per-line-ignores` on top — a
    /// virtual `disable-line`, so lint filtering ([`Self::is_disabled`]) and `--fix`
    /// reconcile ([`Self::reconcile`]) treat config and inline suppression identically.
    /// A `regex` entry suppresses its rules only on matching lines (line numbers come
    /// from [`line_contents`], which splits on the same YAML 1.2 break set as granit, so
    /// a match on line *n* lands on a diagnostic at line *n*). A no-`regex` entry is
    /// file-wide and recorded once in `file_wide` rather than per line.
    #[must_use]
    pub fn parse_with_per_line(
        buffer: &str,
        per_line: &[PerLineRuleApply<'_>],
    ) -> Self {
        let mut directives = Self::parse(buffer);
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
        // Only scan lines when a regex entry needs per-line matching.
        if has_regex_entry {
            for (index, content) in line_contents(buffer).into_iter().enumerate() {
                for entry in per_line {
                    if entry.regex.is_some_and(|regex| regex.is_match(content)) {
                        insert_rules(
                            directives.line_disabled.entry(index + 1).or_default(),
                            entry.rules,
                        );
                    }
                }
            }
        }
        directives
    }

    /// Whether `rule` is disabled on `line` (1-based) by a block or `disable-line`
    /// directive.
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

    /// Rebuild `after` so that any line `rule`'s fixer changed while disabled keeps its
    /// `before` text, reconciling per disabled line.
    ///
    /// Precondition: each fixer is **pure replace** (line count unchanged) **xor**
    /// **pure insert/delete** (count changed) within a single pass — never a mix that
    /// both inserts and deletes lines. Every current safe-fix rule satisfies this. The
    /// equal-length path assumes positional replacement, so a hypothetical fixer that
    /// reordered lines while keeping the count equal could misalign; introduce a real
    /// line diff here before adding such a fixer.
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
