//! `tags` rule: flags unsafe and non-portable YAML tags. Three independent,
//! off-by-default concerns share one tag-inspection pass:
//!
//! * `forbid-unsafe-tags`: construction tags whose suffix begins with a known
//!   namespace (`python/`, `ruby/`, `perl/`, `php/`, `java/`, `java.`, `javax.`); the
//!   curated list is best-effort, not exhaustive.
//! * `forbid-removed-types`: the YAML 1.1 types removed in 1.2 (`!!omap`, `!!pairs`,
//!   `!!set`, `!!timestamp`, `!!binary`).
//! * `allowed-tags`: when non-empty, any other local / non-core tag is flagged.
//!
//! Detection normalises tag spelling, so shorthand (`!!omap`), local (`!ruby/object:`),
//! verbatim (`!<tag:yaml.org,2002:omap>`), and `%TAG`-split forms map to one identity
//! and cannot be used to evade a check. No safe `--fix`: rewriting or dropping a tag
//! changes the node's resolved type (see AGENTS.md "Rules Without A Safe `--fix`").
//!
//! Sources: YAML 1.2.2 spec (tags); YAML 1.2.2 changes page; `PyYAML` docs.

use granit_parser::{Event, Parser, Span, SpannedEventReceiver, Tag};

use crate::config::YamlLintConfig;
use crate::yaml_dom::{YamlOwned, core_schema_suffix};

pub const ID: &str = "tags";

const UNSAFE_TAG_PREFIXES: [&str; 7] = [
    "python/", "ruby/", "perl/", "php/", "java/", "java.", "javax.",
];
const REMOVED_TYPE_SUFFIXES: [&str; 5] =
    ["omap", "pairs", "set", "timestamp", "binary"];

#[derive(Debug, Clone)]
pub struct Config {
    forbid_unsafe_tags: bool,
    forbid_removed_types: bool,
    allowed_tags: Vec<String>,
}

impl Config {
    #[must_use]
    /// Resolve the `tags` configuration from the parsed yamllint config.
    ///
    /// # Panics
    ///
    /// Panics if `allowed-tags` holds a non-string entry; typed config
    /// validation rejects such configs before `resolve` runs.
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        let mut allowed_tags = Vec::new();
        if let Some(YamlOwned::Sequence(seq)) = cfg.rule_option(ID, "allowed-tags") {
            for entry in seq {
                allowed_tags.push(
                    entry
                        .as_str()
                        .expect("typed config validation guarantees string allowed-tags items")
                        .to_string(),
                );
            }
        }
        Self {
            forbid_unsafe_tags: cfg.rule_option_bool(ID, "forbid-unsafe-tags", false),
            forbid_removed_types: cfg.rule_option_bool(
                ID,
                "forbid-removed-types",
                false,
            ),
            allowed_tags,
        }
    }

    fn diagnose(&self, tag: &Tag) -> Option<String> {
        // The non-specific "!" tag carries no safety/portability signal. A `%TAG`
        // directive can resolve a non-empty handle onto it while leaving the suffix
        // "!", so key the exemption on the suffix, not the handle.
        if tag.suffix == "!" {
            return None;
        }
        let core = core_schema_suffix(tag);
        if self.forbid_unsafe_tags
            && unsafe_namespace(tag, core.as_deref()).is_some_and(is_unsafe_suffix)
        {
            return Some(format!("forbidden unsafe tag \"{}\"", shorthand(tag)));
        }
        if self.forbid_removed_types
            && core
                .as_deref()
                .is_some_and(|suffix| REMOVED_TYPE_SUFFIXES.contains(&suffix))
        {
            return Some(format!(
                "forbidden removed YAML 1.1 type \"{}\"",
                shorthand(tag)
            ));
        }
        if !self.allowed_tags.is_empty() && core.is_none() {
            let shorthand = shorthand(tag);
            if !self.allowed_tags.contains(&shorthand) {
                return Some(format!("tag \"{shorthand}\" is not in allowed-tags"));
            }
        }
        None
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
    let mut receiver = TagsReceiver {
        cfg,
        diagnostics: Vec::new(),
    };
    let _ = parser.load(&mut receiver, true);
    receiver.diagnostics
}

struct TagsReceiver<'cfg> {
    cfg: &'cfg Config,
    diagnostics: Vec<Violation>,
}

impl<'input> SpannedEventReceiver<'input> for TagsReceiver<'_> {
    fn on_event(&mut self, event: Event<'input>, span: Span) {
        if let Some(tag) = event.tag()
            && let Some(message) = self.cfg.diagnose(tag)
        {
            let tag_start = span
                .tag_start()
                .expect("granit provides tag_start for tagged node events");
            self.diagnostics.push(Violation {
                line: tag_start.line(),
                column: tag_start.col() + 1,
                message,
            });
        }
    }
}

fn is_unsafe_suffix(suffix: &str) -> bool {
    UNSAFE_TAG_PREFIXES
        .iter()
        .any(|prefix| suffix.starts_with(prefix))
}

/// The construction namespace this tag could match, or `None` for a custom `%TAG`
/// prefix (whose suffix is an unrelated-namespace local name, so it must not be
/// namespace-matched). Only core-schema (`!!`) and local (`!`/verbatim) tags carry a
/// namespace in their suffix, so `!!python/...`, `!python/...`, `!<!python/...>` all
/// reduce to `python/...`.
fn unsafe_namespace<'a>(tag: &'a Tag, core: Option<&'a str>) -> Option<&'a str> {
    if let Some(core) = core {
        Some(core)
    } else if tag.handle == "!" || tag.handle.is_empty() {
        Some(tag.suffix.strip_prefix('!').unwrap_or(&tag.suffix))
    } else {
        None
    }
}

/// Render a tag for diagnostics and allowlist matching: `!!type` for
/// core-schema tags however they were spelled, otherwise its author-facing
/// spelling.
fn shorthand(tag: &Tag) -> String {
    match core_schema_suffix(tag) {
        Some(suffix) => format!("!!{suffix}"),
        None => tag.original(),
    }
}
