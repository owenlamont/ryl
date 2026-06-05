//! `tags` rule &mdash; flags unsafe and non-portable YAML tags (issue #251).
//!
//! Three independent, off-by-default concerns share one tag-inspection pass over
//! the parser's resolved `Scalar`/`SequenceStart`/`MappingStart` tags:
//!
//! * `forbid-unsafe-tags` &mdash; language-specific construction tags whose
//!   suffix begins with a known construction namespace (`python/`, `ruby/`,
//!   `perl/`, `php/`, `java/`, `java.`, `javax.`); the curated list is
//!   best-effort, not exhaustive. These drive arbitrary-object construction in
//!   some loaders; `PyYAML`'s docs warn `yaml.load` "is as powerful as
//!   `pickle.load`" and recommend `safe_load`, and Ruby's Psych and `SnakeYAML`
//!   expose the same pattern via `!ruby/object:` and `!!`-prefixed class tags.
//! * `forbid-removed-types` &mdash; the YAML 1.1 types removed in 1.2 (`!!omap`,
//!   `!!pairs`, `!!set`, `!!timestamp`, `!!binary`; YAML 1.2.2 changes page).
//!   ryl targets YAML 1.2, so these are non-portable.
//! * `allowed-tags` &mdash; when non-empty, any other local / non-core tag
//!   (`!env`, `!include`, …) is flagged. Local tags "may even have different
//!   semantics in different documents" (YAML 1.2.2 spec, tags), so an allowlist
//!   lets a team permit only the handles it actually uses.
//!
//! Detection normalises tag spelling, so shorthand (`!!omap`), local
//! (`!ruby/object:`), and verbatim (`!<tag:yaml.org,2002:omap>`) forms map to
//! the same identity and cannot be used to evade a check.
//!
//! Sources: YAML 1.2.2 spec (tags); YAML 1.2.2 changes page; `PyYAML` docs; The
//! YAML Company. There is no safe `--fix`: rewriting or dropping a tag changes
//! the node's resolved type (see AGENTS.md "Rules Without A Safe `--fix`").

use granit_parser::{Event, Parser, Span, SpannedEventReceiver, Tag};

use crate::config::YamlLintConfig;
use crate::rules::support::span_utils;
use crate::yaml_dom::YamlOwned;

pub const ID: &str = "tags";

const CORE_SCHEMA_PREFIX: &str = "tag:yaml.org,2002:";
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
        // The non-specific "!" tag forces local resolution and carries no
        // safety or portability signal, so no check applies.
        if tag.handle.is_empty() && tag.suffix == "!" {
            return None;
        }
        let core = core_suffix(tag);
        if self.forbid_unsafe_tags
            && unsafe_namespace(tag, core).is_some_and(is_unsafe_suffix)
        {
            return Some(format!("forbidden unsafe tag \"{}\"", shorthand(tag)));
        }
        if self.forbid_removed_types
            && core.is_some_and(|suffix| REMOVED_TYPE_SUFFIXES.contains(&suffix))
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
    let mut diagnostics = receiver.diagnostics;
    clamp_overshoot(buffer, &mut diagnostics);
    diagnostics
}

/// A tag on an implicit/empty scalar that ends the document is positioned by
/// granit at a virtual location that can fall outside the document (see
/// [`span_utils::clamp_position`]); clamp it back onto a real position.
fn clamp_overshoot(buffer: &str, diagnostics: &mut [Violation]) {
    for violation in diagnostics {
        let (line, column) =
            span_utils::clamp_position(buffer, violation.line, violation.column);
        violation.line = line;
        violation.column = column;
    }
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
            self.diagnostics.push(Violation {
                line: span.start.line(),
                column: span.start.col() + 1,
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

/// The core-schema type suffix this tag resolves to regardless of spelling
/// (`!!omap` and verbatim `!<tag:yaml.org,2002:omap>` both yield `omap`), or
/// `None` for any non-core tag.
fn core_suffix(tag: &Tag) -> Option<&str> {
    if tag.handle == CORE_SCHEMA_PREFIX {
        Some(tag.suffix.as_str())
    } else if tag.handle.is_empty() {
        tag.suffix.strip_prefix(CORE_SCHEMA_PREFIX)
    } else {
        None
    }
}

/// The construction namespace this tag could match, or `None` when its handle
/// is a custom `%TAG` prefix (whose suffix is a local name in an unrelated
/// namespace, so it must not be namespace-matched). Only core-schema (`!!`) and
/// local (`!`/verbatim) tags carry a construction namespace in their suffix, so
/// `!!python/…`, `!python/…`, and `!<!python/…>` all reduce to `python/…`.
fn unsafe_namespace<'a>(tag: &'a Tag, core: Option<&'a str>) -> Option<&'a str> {
    if let Some(core) = core {
        Some(core)
    } else if tag.handle == "!" || tag.handle.is_empty() {
        Some(tag.suffix.strip_prefix('!').unwrap_or(&tag.suffix))
    } else {
        None
    }
}

/// Render a tag in shorthand for diagnostics: `!!type` for core-schema tags
/// however they were spelled, otherwise the parser's resolved `!handle`/prefix
/// form (a custom `%TAG` handle renders as its resolved prefix).
fn shorthand(tag: &Tag) -> String {
    match core_suffix(tag) {
        Some(suffix) => format!("!!{suffix}"),
        None => tag.to_string(),
    }
}
