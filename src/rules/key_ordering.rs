//! `key-ordering`: mapping keys must appear in order (optionally locale-aware, with
//! an ignore list). Mirrors yamllint's `key-ordering`. `--fix` moves each entry with its
//! comments, and leaves a mapping unsorted (reported by [`unfixed`]) wherever that could
//! misattach a comment or change the loaded data.

use std::borrow::Cow;
use std::collections::HashMap;

use granit_parser::{Event, Parser, Span, SpannedEventReceiver};
use regex::Regex;
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

use crate::config::YamlLintConfig;
use crate::directives::{Directives, PerLineRuleApply};
use crate::rules::support::line_syntax::split_lines_inclusive;
use crate::rules::support::mapping_key_walker::Walker;
use crate::rules::support::mapping_layout::{Entry, Layout, Mapping, byte_at};
use crate::yaml_dom::{Scalar, YamlOwned};

pub const ID: &str = "key-ordering";

#[derive(Debug, Clone)]
pub struct Config {
    ignored: Vec<Regex>,
    comparator: Comparator,
}

impl Config {
    #[must_use]
    /// Resolve the rule configuration from the parsed yamllint config.
    ///
    /// # Panics
    ///
    /// Panics when `ignored-keys` contains non-string entries or invalid regexes.
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        let mut ignored: Vec<Regex> = Vec::new();
        if let Some(node) = cfg.rule_option(ID, "ignored-keys")
            && let crate::yaml_dom::YamlOwned::Sequence(seq) = node
        {
            for entry in seq {
                let pattern = entry
                    .as_str()
                    .expect("key-ordering ignored-keys should be strings");
                ignored.push(
                    Regex::new(pattern).expect("key-ordering ignored-keys regex"),
                );
            }
        }

        let comparator = cfg
            .locale()
            .map_or_else(Comparator::codepoint, Comparator::with_locale);

        Self {
            ignored,
            comparator,
        }
    }

    fn is_ignored(&self, key: &str) -> bool {
        self.ignored.iter().any(|re| re.is_match(key))
    }

    fn in_order(&self, previous: Option<&str>, current: &str) -> bool {
        let Some(prev) = previous else {
            return true;
        };
        !matches!(
            self.comparator.compare(prev, current),
            std::cmp::Ordering::Greater
        )
    }
}

#[derive(Debug, Clone)]
enum Comparator {
    Codepoint,
    Locale(LocaleComparator),
}

impl Comparator {
    const fn codepoint() -> Self {
        Self::Codepoint
    }

    fn with_locale(locale: &str) -> Self {
        let base = if let Some((head, _)) = locale.split_once(['.', '@']) {
            head
        } else {
            locale
        };
        if base.eq_ignore_ascii_case("C") || base.eq_ignore_ascii_case("POSIX") {
            Self::Codepoint
        } else {
            Self::Locale(LocaleComparator::new(locale))
        }
    }

    fn compare(&self, left: &str, right: &str) -> std::cmp::Ordering {
        match self {
            Self::Codepoint => left.cmp(right),
            Self::Locale(_locale) => LocaleComparator::compare(left, right),
        }
    }
}

#[derive(Debug, Clone)]
struct LocaleComparator;

impl LocaleComparator {
    const fn new(_locale: &str) -> Self {
        Self
    }

    fn compare(left: &str, right: &str) -> std::cmp::Ordering {
        let lhs = normalize_for_locale(left);
        let rhs = normalize_for_locale(right);
        lhs.cmp(&rhs)
    }
}

fn normalize_for_locale(value: &str) -> String {
    let decomposed: String = value.nfkd().filter(|c| !is_combining_mark(*c)).collect();
    decomposed.to_lowercase()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

#[must_use]
pub fn check(buffer: &str, cfg: &Config) -> Vec<Violation> {
    let mut parser = Parser::new_from_str(buffer);
    let mut receiver = KeyOrderingReceiver::new(cfg);
    let _ = parser.load(&mut receiver, true);
    receiver.violations
}

struct KeyOrderingReceiver<'cfg> {
    state: KeyOrderingState<'cfg>,
    violations: Vec<Violation>,
}

impl<'cfg> KeyOrderingReceiver<'cfg> {
    #[allow(clippy::missing_const_for_fn)]
    fn new(cfg: &'cfg Config) -> Self {
        Self {
            state: KeyOrderingState::new(cfg),
            violations: Vec::new(),
        }
    }
}

impl SpannedEventReceiver<'_> for KeyOrderingReceiver<'_> {
    fn on_event(&mut self, event: Event<'_>, span: Span) {
        match event {
            Event::StreamStart => self.state.reset_stream(),
            Event::DocumentStart(..) => self.state.document_start(),
            Event::DocumentEnd => self.state.document_end(),
            Event::SequenceStart(_, _, _) => self.state.enter_sequence(),
            Event::SequenceEnd | Event::MappingEnd => self.state.exit_container(),
            Event::MappingStart(_, _, _) => self.state.enter_mapping(),
            Event::Scalar(value, _, _, _) => {
                self.state
                    .handle_scalar(value.as_ref(), span, &mut self.violations);
            }
            Event::Alias(_) => self.state.skip_node(),
            _ => {}
        }
    }
}

struct KeyOrderingState<'cfg> {
    config: &'cfg Config,
    walker: Walker<MappingState>,
}

impl<'cfg> KeyOrderingState<'cfg> {
    const fn new(config: &'cfg Config) -> Self {
        Self {
            config,
            walker: Walker::new(),
        }
    }

    fn reset_stream(&mut self) {
        self.walker.reset();
    }

    fn document_start(&mut self) {
        self.walker.reset();
    }

    fn document_end(&mut self) {
        self.walker.reset();
    }

    fn enter_mapping(&mut self) {
        self.walker
            .enter_mapping(MappingState { keys: Vec::new() }, ());
    }

    fn enter_sequence(&mut self) {
        self.walker.enter_sequence(());
    }

    fn skip_node(&mut self) {
        self.walker.skip_node();
    }

    fn exit_container(&mut self) {
        self.walker.exit_container();
    }

    fn handle_scalar(
        &mut self,
        value: &str,
        span: Span,
        diagnostics: &mut Vec<Violation>,
    ) {
        let context = self.walker.begin_node();
        if !context.key_root() || self.config.is_ignored(value) {
            self.walker.finish_node(context);
            return;
        }

        let state = self
            .walker
            .current_mapping_mut()
            .expect("stack should contain mapping when key root is active");
        let keys = &mut state.keys;
        if self.config.in_order(keys.last().map(String::as_str), value) {
            keys.push(value.to_owned());
        } else {
            diagnostics.push(Violation {
                line: span.start.line(),
                column: span.start.col() + 1,
                message: format!("wrong ordering of key \"{value}\" in mapping"),
            });
        }
        self.walker.finish_node(context);
    }
}

struct MappingState {
    keys: Vec<String>,
}

/// Sort every out-of-order block mapping that can move safely, or `None` when none can.
#[must_use]
pub fn fix(
    buffer: &str,
    cfg: &Config,
    per_line: &[PerLineRuleApply<'_>],
) -> Option<String> {
    let layout = Layout::parse(buffer)?;
    let directives = Directives::parse_with_per_line(buffer, per_line);
    let sorts: Vec<(usize, Vec<usize>)> = plans(&layout, cfg, &directives)
        .into_iter()
        .filter_map(|plan| Some((plan.mapping, plan.order.ok()?)))
        .collect();
    if sorts.is_empty() {
        return None;
    }
    let fixed = layout.render(&sorts);
    let after = Directives::parse_with_per_line(&fixed, per_line);
    let disabled_change = split_lines_inclusive(buffer)
        .zip(split_lines_inclusive(&fixed))
        .enumerate()
        .any(|(index, (old, new))| old != new && after.is_disabled(ID, index + 1));
    (!disabled_change && same_documents(buffer, &fixed)).then_some(fixed)
}

/// Each mapping with a reported `key-ordering` violation that [`fix`] leaves unsorted,
/// at its first such violation, with the reason as the message.
#[must_use]
pub fn unfixed(
    buffer: &str,
    cfg: &Config,
    per_line: &[PerLineRuleApply<'_>],
) -> Vec<Violation> {
    let Some(layout) = Layout::parse(buffer) else {
        return Vec::new();
    };
    let directives = Directives::parse_with_per_line(buffer, per_line);
    plans(&layout, cfg, &directives)
        .into_iter()
        .map(|plan| Violation {
            line: plan.line + 1,
            column: plan.column + 1,
            message: plan
                .order
                .err()
                .unwrap_or("the sorted output failed verification")
                .to_owned(),
        })
        .collect()
}

struct Plan {
    mapping: usize,
    line: usize,
    column: usize,
    order: Result<Vec<usize>, &'static str>,
}

fn plans(layout: &Layout<'_>, cfg: &Config, directives: &Directives) -> Vec<Plan> {
    let mut plans = Vec::new();
    for (index, mapping) in layout.mappings.iter().enumerate() {
        let mut previous: Option<&str> = None;
        let mut first = None;
        for entry in &mapping.entries {
            let Some(key) = entry.key.as_ref().filter(|key| !cfg.is_ignored(&key.text))
            else {
                continue;
            };
            if cfg.in_order(previous, &key.text) {
                previous = Some(&key.text);
            } else if !directives.is_disabled(ID, entry.key_line + 1) {
                first = Some(entry);
                break;
            }
        }
        if let Some(entry) = first {
            plans.push(Plan {
                mapping: index,
                line: entry.key_line,
                column: entry.key_col,
                order: order(layout, mapping, cfg, directives),
            });
        }
    }
    plans
}

fn order(
    layout: &Layout<'_>,
    mapping: &Mapping,
    cfg: &Config,
    directives: &Directives,
) -> Result<Vec<usize>, &'static str> {
    movable(layout, mapping, directives)?;
    let order = sorted(&mapping.entries, cfg);
    keeps_meaning(layout, &mapping.entries, &order)?;
    Ok(order)
}

fn movable(
    layout: &Layout<'_>,
    mapping: &Mapping,
    directives: &Directives,
) -> Result<(), &'static str> {
    let entries = &mapping.entries;
    let (first, last) = (entries[0].start, entries[entries.len() - 1].end);
    if !mapping.block {
        return Err("flow mappings are not sorted");
    }
    if mapping.keyed_or_tagged {
        return Err("the mapping is tagged or part of a key");
    }
    let col = entries[0].key_col;
    let plain_keys = entries.iter().enumerate().all(|(slot, entry)| {
        let line = layout.lines[entry.key_line].0;
        let prefix = line[..byte_at(line, col)].trim_start_matches('\u{feff}');
        entry.key.is_some()
            && entry.key_col == col
            && if slot == 0 {
                prefix.split_whitespace().all(|token| token == "-")
            } else {
                prefix.bytes().all(|byte| byte == b' ')
            }
    });
    if !plain_keys {
        return Err("a key is complex, explicit, or has an anchor or tag");
    }
    if mapping.loose_comment {
        return Err("a comment between entries is attached to neither");
    }
    if entries.iter().any(|entry| entry.ends_keep) {
        return Err("an entry ends in a keep-chomping block scalar");
    }
    let leading = |line: usize| {
        entries
            .iter()
            .any(|e| (e.start..e.key_line).contains(&line))
    };
    let pinned = layout.directives.iter().any(|&(line, line_scoped)| {
        (first..=last).contains(&line) && !(line_scoped && leading(line))
            || line_scoped && line + 1 == first
    }) || (first..=last).any(|line| directives.is_disabled(ID, line + 1));
    if pinned {
        return Err("a suppression directive applies inside it");
    }
    Ok(())
}

/// `order[slot]` is the entry that sorts into `slot`; ignored keys keep theirs.
fn sorted(entries: &[Entry], cfg: &Config) -> Vec<usize> {
    let key = |index: usize| {
        entries[index]
            .key
            .as_ref()
            .map_or("", |key| key.text.as_str())
    };
    let mut moving: Vec<usize> = (0..entries.len())
        .filter(|&i| !cfg.is_ignored(key(i)))
        .collect();
    moving.sort_by(|&a, &b| cfg.comparator.compare(key(a), key(b)));
    let mut moving = moving.into_iter();
    (0..entries.len())
        .map(|slot| {
            if cfg.is_ignored(key(slot)) {
                slot
            } else {
                moving.next().unwrap_or(slot)
            }
        })
        .collect()
}

fn keeps_meaning(
    layout: &Layout<'_>,
    entries: &[Entry],
    order: &[usize],
) -> Result<(), &'static str> {
    let mut position = vec![0; order.len()];
    for (slot, &from) in order.iter().enumerate() {
        position[from] = slot;
    }

    let mut anchors: HashMap<&str, (Vec<usize>, bool)> = HashMap::new();
    let mut identities: HashMap<Scalar<'_>, Vec<usize>> = HashMap::new();
    for (index, entry) in entries.iter().enumerate() {
        for (name, defines) in entry
            .anchors
            .iter()
            .map(|name| (name, true))
            .chain(entry.aliases.iter().map(|name| (name, false)))
        {
            let group = anchors.entry(name).or_default();
            if group.0.last() != Some(&index) {
                group.0.push(index);
            }
            group.1 |= defines;
        }
    }
    let resolved = entries.iter().enumerate().filter_map(|(index, entry)| {
        let key = entry.key.as_ref();
        key.and_then(|key| {
            Scalar::resolve_scalar(Cow::Borrowed(&key.text), key.style, None)
        })
        .map(|identity| (identity, index))
    });
    for (identity, index) in resolved {
        identities.entry(identity).or_default().push(index);
    }
    let kept =
        |group: &[usize]| group.windows(2).all(|w| position[w[0]] < position[w[1]]);
    if !anchors
        .values()
        .all(|(group, defines)| !defines || kept(group))
    {
        return Err("an alias would move before its anchor or a redefinition");
    }
    if !identities.values().all(|group| kept(group)) {
        return Err("two differently spelled keys load as the same key");
    }
    let line = layout.lines[entries[0].key_line].0;
    if line[..byte_at(line, entries[0].key_col)].contains('-')
        && entries[order[0]].start < entries[order[0]].key_line
    {
        return Err("an entry's leading comment would land on a `- ` line");
    }
    Ok(())
}

fn same_documents(before: &str, after: &str) -> bool {
    let load = |text| YamlOwned::load_from_str(text).ok();
    load(before)
        .zip(load(after))
        .is_some_and(|(before, after)| {
            before.len() == after.len()
                && before.iter().zip(&after).all(|(a, b)| same_data(a, b))
        })
}

/// Equality that ignores mapping order (the loaded `YamlOwned` compares it).
fn same_data(before: &YamlOwned, after: &YamlOwned) -> bool {
    match (before, after) {
        (YamlOwned::Mapping(a), YamlOwned::Mapping(b)) => {
            a.len() == b.len()
                && a.iter().all(|(key, value)| {
                    b.get(key).is_some_and(|other| same_data(value, other))
                })
        }
        (YamlOwned::Sequence(a), YamlOwned::Sequence(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same_data(x, y))
        }
        (YamlOwned::Tagged(a_tag, a), YamlOwned::Tagged(b_tag, b)) => {
            a_tag == b_tag && same_data(a, b)
        }
        _ => before == after,
    }
}
