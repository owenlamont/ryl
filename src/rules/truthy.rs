use std::collections::HashSet;

use saphyr_parser::{Event, Parser, ScalarStyle, Span, SpannedEventReceiver};

use crate::config::YamlLintConfig;

pub const ID: &str = "truthy";

const TRUTHY_VALUES_YAML_1_1: [&str; 18] = [
    "YES", "Yes", "yes", "NO", "No", "no", "TRUE", "True", "true", "FALSE", "False", "false", "ON",
    "On", "on", "OFF", "Off", "off",
];

const TRUTHY_VALUES_YAML_1_2: [&str; 6] = ["TRUE", "True", "true", "FALSE", "False", "false"];

#[derive(Debug, Clone)]
pub struct Config {
    allowed: HashSet<String>,
    allowed_display: String,
    pub check_keys: bool,
}

impl Config {
    /// Resolve the rule configuration from the parsed yamllint config.
    ///
    /// # Panics
    ///
    /// Panics when `allowed-values` contains a non-string entry. The parser rejects
    /// that configuration, so this only occurs with manual construction in tests.
    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        let mut allowed: HashSet<String> = HashSet::new();
        allowed.insert("true".to_string());
        allowed.insert("false".to_string());
        let mut check_keys = true;

        if let Some(node) = cfg.rule_option(ID, "allowed-values")
            && let Some(seq) = node.as_sequence()
        {
            allowed.clear();
            for value in seq {
                let text = value
                    .as_str()
                    .expect("truthy allowed-values should be strings");
                allowed.insert(text.to_owned());
            }
        }

        if let Some(node) = cfg.rule_option(ID, "check-keys")
            && let Some(flag) = node.as_bool()
        {
            check_keys = flag;
        }

        let mut display_values: Vec<&str> = allowed.iter().map(String::as_str).collect();
        display_values.sort_unstable();
        let allowed_display = format!("[{}]", display_values.join(", "));

        Self {
            allowed,
            allowed_display,
            check_keys,
        }
    }

    fn allows(&self, value: &str) -> bool {
        self.allowed.contains(value)
    }

    fn allowed_display(&self) -> &str {
        &self.allowed_display
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

#[derive(Debug)]
struct ContainerState {
    kind: ContainerKind,
    key_context: bool,
}

#[derive(Debug)]
enum ContainerKind {
    Sequence,
    Mapping { expect_key: bool },
}

#[derive(Debug)]
struct TruthyState<'cfg> {
    config: &'cfg Config,
    containers: Vec<ContainerState>,
    key_depth: usize,
    current_version: (u32, u32),
    bad_truthy: Option<HashSet<String>>,
    directives: Vec<(usize, (u32, u32))>,
    directive_index: usize,
}

impl<'cfg> TruthyState<'cfg> {
    const fn new(config: &'cfg Config, directives: Vec<(usize, (u32, u32))>) -> Self {
        Self {
            config,
            containers: Vec::new(),
            key_depth: 0,
            current_version: (1, 1),
            bad_truthy: None,
            directives,
            directive_index: 0,
        }
    }

    fn document_start(&mut self, span: Span) {
        self.current_version = self.version_for_document(span.start.index());
        self.bad_truthy = None;
        self.key_depth = 0;
        self.containers.clear();
    }

    fn document_end(&mut self) {
        self.key_depth = 0;
        self.containers.clear();
    }

    fn version_for_document(&mut self, doc_start: usize) -> (u32, u32) {
        let mut version = None;
        while self.directive_index < self.directives.len()
            && self.directives[self.directive_index].0 < doc_start
        {
            version = Some(self.directives[self.directive_index].1);
            self.directive_index += 1;
        }
        version.unwrap_or((1, 1))
    }

    fn begin_node(&mut self) -> bool {
        let mut is_key_node = false;
        if let Some(ContainerState {
            kind: ContainerKind::Mapping { expect_key },
            ..
        }) = self.containers.last_mut()
        {
            if *expect_key {
                is_key_node = true;
                *expect_key = false;
            } else {
                *expect_key = true;
            }
        }

        let active_key = is_key_node || self.key_depth > 0;
        if active_key {
            self.key_depth += 1;
        }
        active_key
    }

    fn enter_mapping(&mut self) {
        let active_key = self.begin_node();
        self.containers.push(ContainerState {
            kind: ContainerKind::Mapping { expect_key: true },
            key_context: active_key,
        });
    }

    fn enter_sequence(&mut self) {
        let active_key = self.begin_node();
        self.containers.push(ContainerState {
            kind: ContainerKind::Sequence,
            key_context: active_key,
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

    const fn finish_scalar(&mut self, active_key: bool) {
        if active_key && self.key_depth > 0 {
            self.key_depth -= 1;
        }
    }

    fn is_bad_truthy(&mut self, value: &str) -> bool {
        if self.bad_truthy.is_none() {
            let base = if self.current_version == (1, 2) {
                &TRUTHY_VALUES_YAML_1_2[..]
            } else {
                &TRUTHY_VALUES_YAML_1_1[..]
            };
            let mut set: HashSet<String> = HashSet::new();
            for candidate in base {
                if !self.config.allows(candidate) {
                    set.insert((*candidate).to_string());
                }
            }
            self.bad_truthy = Some(set);
        }
        self.bad_truthy
            .as_ref()
            .expect("bad truthy set initialised")
            .contains(value)
    }

    fn handle_scalar(
        &mut self,
        style: ScalarStyle,
        value: &str,
        tagged: bool,
        span: Span,
        diagnostics: &mut Vec<Violation>,
    ) {
        let active_key = self.begin_node();

        if tagged {
            self.finish_scalar(active_key);
            return;
        }

        if !matches!(style, ScalarStyle::Plain) {
            self.finish_scalar(active_key);
            return;
        }

        if active_key && !self.config.check_keys {
            self.finish_scalar(active_key);
            return;
        }

        if self.is_bad_truthy(value) {
            diagnostics.push(Violation {
                line: span.start.line(),
                column: span.start.col() + 1,
                message: format!(
                    "truthy value should be one of {}",
                    self.config.allowed_display()
                ),
            });
        }

        self.finish_scalar(active_key);
    }
}

struct TruthyReceiver<'cfg> {
    state: TruthyState<'cfg>,
    diagnostics: Vec<Violation>,
}

#[allow(clippy::missing_const_for_fn)]
impl<'cfg> TruthyReceiver<'cfg> {
    fn new(cfg: &'cfg Config, directives: Vec<(usize, (u32, u32))>) -> Self {
        Self {
            state: TruthyState::new(cfg, directives),
            diagnostics: Vec::new(),
        }
    }
}

impl SpannedEventReceiver<'_> for TruthyReceiver<'_> {
    fn on_event(&mut self, event: Event<'_>, span: Span) {
        match event {
            Event::StreamStart => {
                self.state.current_version = (1, 1);
                self.state.bad_truthy = None;
                self.state.directive_index = 0;
            }
            Event::DocumentStart(_) => self.state.document_start(span),
            Event::DocumentEnd => self.state.document_end(),
            Event::SequenceStart(_, _) => self.state.enter_sequence(),
            Event::SequenceEnd | Event::MappingEnd => self.state.exit_container(),
            Event::MappingStart(_, _) => self.state.enter_mapping(),
            Event::Scalar(value, style, _, tag) => {
                let tagged = tag.is_some();
                self.state.handle_scalar(
                    style,
                    value.as_ref(),
                    tagged,
                    span,
                    &mut self.diagnostics,
                );
            }
            Event::Alias(_) | Event::StreamEnd | Event::Nothing => {}
        }
    }
}

#[must_use]
pub fn check(buffer: &str, cfg: &Config) -> Vec<Violation> {
    let directives = collect_yaml_directives(buffer);
    let mut parser = Parser::new_from_str(buffer);
    let mut receiver = TruthyReceiver::new(cfg, directives);
    let _ = parser.load(&mut receiver, true);
    receiver.diagnostics
}

fn collect_yaml_directives(buffer: &str) -> Vec<(usize, (u32, u32))> {
    let mut directives = Vec::new();
    let mut offset = 0;
    for segment in buffer.split_inclusive(['\n']) {
        let line = segment.trim_end_matches(['\n', '\r']);
        if let Some(version) = parse_yaml_directive(line) {
            let leading = line.len() - line.trim_start().len();
            directives.push((offset + leading, version));
        }
        offset += segment.len();
    }
    directives
}

fn parse_yaml_directive(line: &str) -> Option<(u32, u32)> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with("%YAML") {
        return None;
    }
    let mut parts = trimmed.split_whitespace();
    let _ = parts.next();
    let version = parts.next()?;
    let (major_raw, minor_raw) = version.split_once('.')?;
    let major = major_raw.parse().ok()?;
    let minor = minor_raw.parse().ok()?;
    Some((major, minor))
}
