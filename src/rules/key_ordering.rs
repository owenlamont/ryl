use regex::Regex;
use saphyr_parser::{Event, Parser, Span, SpannedEventReceiver};
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

use crate::config::YamlLintConfig;

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
    /// Panics when `ignored-keys` entries are not strings. Configuration parsing
    /// validates types before resolution, so this only occurs with manual construction in tests.
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        let mut ignored: Vec<Regex> = Vec::new();
        if let Some(node) = cfg.rule_option(ID, "ignored-keys") {
            match node {
                saphyr::YamlOwned::Sequence(seq) => {
                    for entry in seq {
                        let pattern = entry
                            .as_str()
                            .expect("key-ordering ignored-keys should be strings");
                        ignored.push(Regex::new(pattern).expect("key-ordering ignored-keys regex"));
                    }
                }
                saphyr::YamlOwned::Value(value) => {
                    if let Some(text) = value.as_str() {
                        ignored.push(Regex::new(text).expect("key-ordering ignored-keys regex"));
                    }
                }
                _ => {}
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
            Event::DocumentStart(_) => self.state.document_start(),
            Event::DocumentEnd => self.state.document_end(),
            Event::SequenceStart(_, _) => self.state.enter_sequence(),
            Event::SequenceEnd | Event::MappingEnd => self.state.exit_container(),
            Event::MappingStart(_, _) => self.state.enter_mapping(),
            Event::Scalar(value, _, _, _) => {
                self.state
                    .handle_scalar(value.as_ref(), span, &mut self.violations);
            }
            Event::Alias(_) | Event::StreamEnd | Event::Nothing => {}
        }
    }
}

struct KeyOrderingState<'cfg> {
    config: &'cfg Config,
    containers: Vec<ContainerState>,
    key_depth: usize,
}

impl<'cfg> KeyOrderingState<'cfg> {
    const fn new(config: &'cfg Config) -> Self {
        Self {
            config,
            containers: Vec::new(),
            key_depth: 0,
        }
    }

    fn reset_stream(&mut self) {
        self.containers.clear();
        self.key_depth = 0;
    }

    fn document_start(&mut self) {
        self.containers.clear();
        self.key_depth = 0;
    }

    fn document_end(&mut self) {
        self.containers.clear();
        self.key_depth = 0;
    }

    fn enter_mapping(&mut self) {
        let ctx = self.begin_node();
        self.containers.push(ContainerState {
            key_context: ctx.active,
            kind: ContainerKind::Mapping {
                expect_key: true,
                keys: Vec::new(),
            },
        });
    }

    fn enter_sequence(&mut self) {
        let ctx = self.begin_node();
        self.containers.push(ContainerState {
            key_context: ctx.active,
            kind: ContainerKind::Sequence,
        });
    }

    fn exit_container(&mut self) {
        let container = self
            .containers
            .pop()
            .expect("container stack should not underflow");
        if container.key_context && self.key_depth > 0 {
            self.key_depth -= 1;
        }
    }

    fn handle_scalar(&mut self, value: &str, span: Span, diagnostics: &mut Vec<Violation>) {
        let context = self.begin_node();
        if !context.key_root || self.config.is_ignored(value) {
            self.finish_node(context);
            return;
        }

        if let Some(state) = self.containers.last_mut()
            && let ContainerKind::Mapping { keys, .. } = &mut state.kind
        {
            if self.config.in_order(keys.last().map(String::as_str), value) {
                keys.push(value.to_owned());
            } else {
                diagnostics.push(Violation {
                    line: span.start.line(),
                    column: span.start.col() + 1,
                    message: format!("wrong ordering of key \"{value}\" in mapping"),
                });
            }
        }
        self.finish_node(context);
    }

    fn begin_node(&mut self) -> NodeContext {
        let mut key_root = false;
        if let Some(ContainerState {
            kind: ContainerKind::Mapping { expect_key, .. },
            ..
        }) = self.containers.last_mut()
        {
            if *expect_key {
                key_root = true;
                *expect_key = false;
            } else {
                *expect_key = true;
            }
        }
        let active = key_root || self.key_depth > 0;
        if active {
            self.key_depth += 1;
        }
        NodeContext { active, key_root }
    }

    const fn finish_node(&mut self, context: NodeContext) {
        if context.active && self.key_depth > 0 {
            self.key_depth -= 1;
        }
    }
}

struct ContainerState {
    key_context: bool,
    kind: ContainerKind,
}

enum ContainerKind {
    Sequence,
    Mapping { expect_key: bool, keys: Vec<String> },
}

#[derive(Copy, Clone)]
struct NodeContext {
    active: bool,
    key_root: bool,
}
