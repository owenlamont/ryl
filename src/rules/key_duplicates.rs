use saphyr_parser::{Event, Parser, Span, SpannedEventReceiver};

use crate::config::YamlLintConfig;
use crate::rules::support::mapping_key_walker::Walker;

pub const ID: &str = "key-duplicates";

#[derive(Debug, Clone, Copy)]
pub struct Config {
    forbid_duplicated_merge_keys: bool,
}

impl Config {
    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        Self {
            forbid_duplicated_merge_keys: cfg.rule_option_bool(
                ID,
                "forbid-duplicated-merge-keys",
                false,
            ),
        }
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
    let mut parser = Parser::new_from_str(buffer);
    let mut receiver = KeyDuplicatesReceiver::new(cfg);
    let _ = parser.load(&mut receiver, true);
    receiver.violations
}

struct KeyDuplicatesReceiver<'cfg> {
    state: KeyDuplicatesState<'cfg>,
    violations: Vec<Violation>,
}

impl<'cfg> KeyDuplicatesReceiver<'cfg> {
    const fn new(config: &'cfg Config) -> Self {
        Self {
            state: KeyDuplicatesState::new(config),
            violations: Vec::new(),
        }
    }
}

impl SpannedEventReceiver<'_> for KeyDuplicatesReceiver<'_> {
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
            Event::Alias(_) => self.state.handle_alias(),
            Event::StreamEnd | Event::Nothing => {}
        }
    }
}

struct KeyDuplicatesState<'cfg> {
    config: &'cfg Config,
    walker: Walker<MappingState>,
}

impl<'cfg> KeyDuplicatesState<'cfg> {
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
        self.walker.enter_mapping(MappingState::new(), ());
    }

    fn enter_sequence(&mut self) {
        self.walker.enter_sequence(());
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
        if !context.key_root() {
            self.walker.finish_node(context);
            return;
        }

        let state = self
            .walker
            .current_mapping_mut()
            .expect("mapping state should exist when key_root is active");

        let is_duplicate = state.seen_keys.iter().any(|key| key == value);
        let is_merge_key = value == "<<";
        if is_duplicate && (!is_merge_key || self.config.forbid_duplicated_merge_keys) {
            diagnostics.push(Violation {
                line: span.start.line(),
                column: span.start.col() + 1,
                message: format!("duplication of key \"{value}\" in mapping"),
            });
        } else {
            state.seen_keys.push(value.to_owned());
        }

        self.walker.finish_node(context);
    }

    fn handle_alias(&mut self) {
        let context = self.walker.begin_node();
        self.walker.finish_node(context);
    }
}

struct MappingState {
    seen_keys: Vec<String>,
}

impl MappingState {
    const fn new() -> Self {
        Self {
            seen_keys: Vec::new(),
        }
    }
}
