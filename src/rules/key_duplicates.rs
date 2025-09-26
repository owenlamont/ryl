use saphyr_parser::{Event, Parser, Span, SpannedEventReceiver};

use crate::config::YamlLintConfig;

pub const ID: &str = "key-duplicates";

#[derive(Debug, Clone, Copy)]
pub struct Config {
    forbid_duplicated_merge_keys: bool,
}

impl Config {
    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        let forbid_duplicated_merge_keys = cfg
            .rule_option(ID, "forbid-duplicated-merge-keys")
            .and_then(saphyr::YamlOwned::as_bool)
            .unwrap_or(false);

        Self {
            forbid_duplicated_merge_keys,
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
    containers: Vec<ContainerState>,
    key_depth: usize,
}

impl<'cfg> KeyDuplicatesState<'cfg> {
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
        let context = self.begin_node();
        self.containers.push(ContainerState {
            key_context: context.active,
            mapping: Some(MappingState::new()),
        });
    }

    fn enter_sequence(&mut self) {
        let context = self.begin_node();
        self.containers.push(ContainerState {
            key_context: context.active,
            mapping: None,
        });
    }

    fn exit_container(&mut self) {
        if let Some(container) = self.containers.pop()
            && container.key_context
            && self.key_depth > 0
        {
            self.key_depth -= 1;
        }
    }

    fn handle_scalar(&mut self, value: &str, span: Span, diagnostics: &mut Vec<Violation>) {
        let context = self.begin_node();
        if !context.key_root {
            self.finish_node(context);
            return;
        }

        let state = self
            .containers
            .last_mut()
            .and_then(|container| container.mapping.as_mut())
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

        self.finish_node(context);
    }

    fn begin_node(&mut self) -> NodeContext {
        let mut key_root = false;
        if let Some(ContainerState {
            mapping: Some(mapping),
            ..
        }) = self.containers.last_mut()
        {
            if mapping.expect_key {
                key_root = true;
                mapping.expect_key = false;
            } else {
                mapping.expect_key = true;
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

    fn handle_alias(&mut self) {
        let context = self.begin_node();
        self.finish_node(context);
    }
}

struct ContainerState {
    key_context: bool,
    mapping: Option<MappingState>,
}

struct MappingState {
    expect_key: bool,
    seen_keys: Vec<String>,
}

impl MappingState {
    const fn new() -> Self {
        Self {
            expect_key: true,
            seen_keys: Vec::new(),
        }
    }
}

#[derive(Copy, Clone)]
struct NodeContext {
    active: bool,
    key_root: bool,
}
