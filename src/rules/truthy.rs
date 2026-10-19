//! `truthy`: flag YAML 1.1 boolean-ish words (`yes`/`no`/`on`/`off`/`True`/…) that are
//! plain strings under the YAML 1.2 core schema and so a portability trap. Mirrors
//! yamllint's `truthy`. Safe `--fix` re-cases `True`/`FALSE`/… to an allowed spelling of
//! the same boolean; `yes`/`no`/`on`/`off` need a quoting-or-type choice, so stay manual.

use std::collections::HashSet;

use granit_parser::{Event, ScalarStyle, Span, SpannedEventReceiver};

use crate::config::YamlLintConfig;
use crate::rules::support::mapping_key_walker::Walker;
use crate::rules::support::span_utils::{
    BytePos, apply_replacements, marker_byte_offset,
};
use crate::rules::support::yaml_version::{Version, event_version};

pub const ID: &str = "truthy";

const TRUTHY_VALUES_YAML_1_1: [&str; 18] = [
    "YES", "Yes", "yes", "NO", "No", "no", "TRUE", "True", "true", "FALSE", "False",
    "false", "ON", "On", "on", "OFF", "Off", "off",
];

const TRUTHY_VALUES_YAML_1_2: [&str; 6] =
    ["TRUE", "True", "true", "FALSE", "False", "false"];

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

        let mut display_values: Vec<&str> =
            allowed.iter().map(String::as_str).collect();
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

    fn fix_target(&self, value: &str) -> Option<&'static str> {
        let family: [&'static str; 3] = match value {
            "true" | "True" | "TRUE" => ["true", "True", "TRUE"],
            "false" | "False" | "FALSE" => ["false", "False", "FALSE"],
            _ => return None,
        };
        family.into_iter().find(|spelling| self.allows(spelling))
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

struct TruthyState<'cfg> {
    config: &'cfg Config,
    walker: Walker<()>,
    current_version: Version,
    bad_truthy: Option<HashSet<String>>,
}

impl<'cfg> TruthyState<'cfg> {
    const fn new(config: &'cfg Config) -> Self {
        Self {
            config,
            walker: Walker::new(),
            current_version: (1, 1),
            bad_truthy: None,
        }
    }

    fn document_start(&mut self, version: Option<Version>) {
        self.current_version = version.unwrap_or((1, 1));
        self.bad_truthy = None;
        self.walker.reset();
    }

    fn document_end(&mut self) {
        self.walker.reset();
    }

    fn is_bad_truthy(&mut self, value: &str) -> bool {
        if self.bad_truthy.is_none() {
            // A higher minor (`%YAML 1.3`) is processed as 1.2, so anything at 1.2 or
            // above uses the 1.2 set; 1.0/1.1 and the directive-less default use 1.1.
            let base = if self.current_version >= (1, 2) {
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
        replacements: &mut Vec<Replacement>,
    ) {
        let context = self.walker.begin_node();
        if !tagged
            && matches!(style, ScalarStyle::Plain)
            && (!context.active() || self.config.check_keys)
            && self.is_bad_truthy(value)
        {
            diagnostics.push(Violation {
                line: span.start.line(),
                column: span.start.col() + 1,
                message: format!(
                    "truthy value should be one of {}",
                    self.config.allowed_display()
                ),
            });
            if let Some(target) = self.config.fix_target(value) {
                let start = marker_byte_offset(span.start);
                let end = BytePos::new(start.get() + value.len());
                replacements.push((start, end, target.to_owned()));
            }
        }

        self.walker.finish_node(context);
    }
}

type Replacement = (BytePos, BytePos, String);

struct TruthyReceiver<'cfg> {
    state: TruthyState<'cfg>,
    diagnostics: Vec<Violation>,
    replacements: Vec<Replacement>,
}

#[allow(clippy::missing_const_for_fn)]
impl<'cfg> TruthyReceiver<'cfg> {
    fn new(cfg: &'cfg Config) -> Self {
        Self {
            state: TruthyState::new(cfg),
            diagnostics: Vec::new(),
            replacements: Vec::new(),
        }
    }
}

impl SpannedEventReceiver<'_> for TruthyReceiver<'_> {
    fn on_event(&mut self, event: Event<'_>, span: Span) {
        match event {
            Event::StreamStart => {
                self.state.current_version = (1, 1);
                self.state.bad_truthy = None;
            }
            Event::DocumentStart(_, version) => {
                self.state.document_start(version.map(event_version));
            }
            Event::DocumentEnd => self.state.document_end(),
            Event::SequenceStart(_, _, _) => self.state.walker.enter_sequence(()),
            Event::SequenceEnd | Event::MappingEnd => {
                self.state.walker.exit_container();
            }
            Event::MappingStart(_, _, _) => self.state.walker.enter_mapping((), ()),
            Event::Scalar(value, style, _, tag) => {
                let tagged = tag.is_some();
                self.state.handle_scalar(
                    style,
                    value.as_ref(),
                    tagged,
                    span,
                    &mut self.diagnostics,
                    &mut self.replacements,
                );
            }
            Event::Alias(_) => self.state.walker.skip_node(),
            _ => {}
        }
    }
}

fn walk<'cfg>(
    input: crate::text_version::Analysis<'_>,
    cfg: &'cfg Config,
) -> TruthyReceiver<'cfg> {
    let mut receiver = TruthyReceiver::new(cfg);
    for (event, span) in input.event_iter() {
        receiver.on_event(event, span);
    }
    receiver
}

#[must_use]
pub fn check(buffer: &str, cfg: &Config) -> Vec<Violation> {
    check_with_analysis(
        crate::text_version::TextVersion::new(buffer).analysis(),
        cfg,
    )
}

pub(crate) fn check_with_analysis(
    input: crate::text_version::Analysis<'_>,
    cfg: &Config,
) -> Vec<Violation> {
    walk(input, cfg).diagnostics
}

#[must_use]
pub fn fix(buffer: &str, cfg: &Config) -> Option<String> {
    let replacements = walk(
        crate::text_version::TextVersion::new(buffer).analysis(),
        cfg,
    )
    .replacements;
    (!replacements.is_empty()).then(|| apply_replacements(buffer, replacements))
}
