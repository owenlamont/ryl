//! `truthy`: flag YAML 1.1 boolean-ish words (`yes`/`no`/`on`/`off`/`True`/…) that are
//! plain strings under the YAML 1.2 core schema and so a portability trap. Mirrors
//! yamllint's `truthy`. Safe `--fix` re-cases `True`/`FALSE`/… to an allowed spelling of
//! the same boolean. In a document YAML 1.1 and 1.2 readers must both load, an unsafe
//! fix rewrites `yes`/`no`/`on`/`off` to the spelling both read as `source-version` does:
//! quoted under `"1.2"`, an allowed `true`/`false` under `"1.1"`.

use std::collections::HashSet;

use ryl_yaml_parser::{Event, ScalarStyle, Span, SpannedEventReceiver};

use crate::config::TargetVersion;
use crate::config::YamlLintConfig;
use crate::rules::support::mapping_key_walker::Walker;
use crate::rules::support::span_utils::{
    BytePos, apply_replacements, marker_byte_offset,
};
use crate::rules::support::yaml_version::{
    Readers, Respelling, Version, event_version,
};

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
    target: TargetVersion,
    respelling: Respelling,
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
            target: cfg.target_version(),
            respelling: Respelling::resolve(cfg, ID),
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
    pub hidden_unsafe_fix: bool,
}

struct TruthyState<'cfg> {
    config: &'cfg Config,
    walker: Walker<()>,
    readers: Readers,
}

impl<'cfg> TruthyState<'cfg> {
    const fn new(config: &'cfg Config) -> Self {
        Self {
            config,
            walker: Walker::new(),
            readers: Readers::of(None, config.target),
        }
    }

    fn document_start(&mut self, version: Option<Version>) {
        self.readers = Readers::of(version, self.config.target);
        self.walker.reset();
    }

    fn document_end(&mut self) {
        self.walker.reset();
    }

    fn is_bad_truthy(&self, value: &str) -> bool {
        let base = if self.readers == Readers::Yaml12 {
            &TRUTHY_VALUES_YAML_1_2[..]
        } else {
            &TRUTHY_VALUES_YAML_1_1[..]
        };
        base.contains(&value) && !self.config.allows(value)
    }

    fn handle_scalar(
        &mut self,
        buffer: &str,
        plain_untagged: bool,
        value: &str,
        span: Span,
        diagnostics: &mut Vec<Violation>,
        replacements: &mut Vec<Replacement>,
    ) {
        let context = self.walker.begin_node();
        if plain_untagged
            && (!context.active() || self.config.check_keys)
            && self.is_bad_truthy(value)
        {
            let mut message = format!(
                "truthy value should be one of {}",
                self.config.allowed_display()
            );
            let respelling = self.config.respelling;
            let mut hidden_unsafe_fix = false;
            if let Some(target) = self.config.fix_target(value) {
                let start = marker_byte_offset(span.start);
                let end = BytePos::new(start.get() + value.len());
                replacements.push((start, end, target.to_owned()));
            } else if let Some((start, end, spelling)) = (self.readers == Readers::Both)
                .then(|| respelling.edit(buffer, span, value))
                .flatten()
                .and_then(|(start, end, spelling)| {
                    let spelling = match self.config.fix_target(&spelling) {
                        Some(target) => target.to_owned(),
                        None if spelling.starts_with(['"', '\'']) => spelling,
                        None => return None,
                    };
                    Some((start, end, spelling))
                })
            {
                message.push_str(&respelling.note(value, &spelling));
                hidden_unsafe_fix = respelling.hidden();
                if respelling.enabled() {
                    replacements.push((start, end, spelling));
                }
            }
            diagnostics.push(Violation {
                line: span.start.line(),
                column: span.start.col() + 1,
                message,
                hidden_unsafe_fix,
            });
        }

        self.walker.finish_node(context);
    }
}

type Replacement = (BytePos, BytePos, String);

struct TruthyReceiver<'cfg, 'text> {
    buffer: &'text str,
    state: TruthyState<'cfg>,
    diagnostics: Vec<Violation>,
    replacements: Vec<Replacement>,
}

#[allow(clippy::missing_const_for_fn)]
impl<'cfg, 'text> TruthyReceiver<'cfg, 'text> {
    fn new(cfg: &'cfg Config, buffer: &'text str) -> Self {
        Self {
            buffer,
            state: TruthyState::new(cfg),
            diagnostics: Vec::new(),
            replacements: Vec::new(),
        }
    }
}

impl SpannedEventReceiver<'_> for TruthyReceiver<'_, '_> {
    fn on_event(&mut self, event: Event<'_>, span: Span) {
        match event {
            Event::StreamStart => self.state.document_start(None),
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
                self.state.handle_scalar(
                    self.buffer,
                    tag.is_none() && style == ScalarStyle::Plain,
                    value.as_ref(),
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

fn walk<'cfg, 'text>(
    input: crate::text_version::Analysis<'text>,
    cfg: &'cfg Config,
) -> TruthyReceiver<'cfg, 'text> {
    let mut receiver = TruthyReceiver::new(cfg, input.text());
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
