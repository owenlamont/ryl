//! Inline rule-disable comment directives.
//!
//! Mirrors yamllint's `# yamllint disable` / `enable` / `disable-line` /
//! `disable-file` directives (`yamllint/linter.py`) and adds a preferred `# ryl …`
//! spelling kept in lockstep with the grammar. The lint engine
//! ([`crate::lint::lint_str`]) filters every rule's diagnostics through
//! [`Directives::is_disabled`], and `--fix` ([`crate::fix`]) keeps disabled lines
//! untouched via [`Directives::reconcile`], so behaviour is uniform across all rules.
//! A first-line [`disables_file`] directive skips the whole file (and `--fix`).

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use granit_parser::Placement;
use regex::Regex;

use crate::rules::ALL_RULE_IDS;
use crate::rules::support::comments_scan::collect_comments;

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

/// Whether the buffer's first line is a `disable-file` directive. Such a file is
/// skipped entirely &mdash; no diagnostics (not even syntax errors) and no `--fix`
/// rewrites &mdash; matching yamllint (`yamllint/linter.py`).
#[must_use]
pub fn disables_file(buffer: &str) -> bool {
    DISABLE_FILE.is_match(buffer.lines().next().unwrap_or(""))
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
/// rules each line disables via `disable-line`.
#[derive(Default)]
pub struct Directives {
    /// `(line, rules disabled from that line onward)`, in ascending line order.
    block_snapshots: Vec<(usize, HashSet<&'static str>)>,
    line_disabled: HashMap<usize, HashSet<&'static str>>,
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

    /// Whether `rule` is disabled on `line` (1-based) by a block or `disable-line`
    /// directive.
    #[must_use]
    pub fn is_disabled(&self, rule: &str, line: usize) -> bool {
        self.block_snapshots
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
        self.line_disabled.values().any(|set| set.contains(rule))
            || self
                .block_snapshots
                .iter()
                .any(|(_, set)| set.contains(rule))
    }

    /// Rebuild `after` so that any line `rule`'s fixer changed while disabled keeps its
    /// `before` text. Fixers are pure replace (line count unchanged) or pure
    /// insert/delete (count changed); both are reconciled per disabled line.
    #[must_use]
    pub fn reconcile(&self, rule: &str, before: &str, after: &str) -> String {
        let before_lines: Vec<&str> = before.split_inclusive('\n').collect();
        let after_lines: Vec<&str> = after.split_inclusive('\n').collect();
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
