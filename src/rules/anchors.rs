//! `anchors` rule: flags undeclared aliases, duplicated anchors, unused anchors, and
//! (ryl-only, TOML `forbid-ambiguous-anchor-alias-names`) a name with a welded `:`
//! (`&foo:`, `*foo:`, `&foo:bar`, `&:`/`&:foo`); a separating space (`*foo : bar`) is
//! never flagged.
//!
//! `:` is a legal anchor-name char (YAML 1.2.2 §6.9.2 excludes only `,[]{}`), so the
//! scanned name includes it (`&foo:bar` -> `foo:bar`) and the ambiguity check is a
//! plain `name.contains(':')`; the other checks compare full names, making `&foo:bar`
//! and `&foo:baz` distinct. yamllint narrows names at `:` (`PyYAML`'s non-conformant
//! behaviour); divergence catalogued in
//! `docs/getting-started/migrating-from-yamllint.md`.
//!
//! Detection reads granit's scanner tokens (`TokenType::Anchor`/`Alias`), so a literal
//! `&`/`*` in a plain/block scalar, a glob, or after a tag is not an anchor/alias. The
//! scanner is resolution-independent, so an undefined or forward alias is still
//! tokenised (the parser would error on it), as `forbid-undeclared-aliases` needs.
//!
//! Sources: YAML 1.2.2 §6.9.2, §7.1; adrienverge/yamllint#780.

use std::collections::HashMap;

use granit_parser::{Scanner, StrInput, TokenType};

use crate::config::YamlLintConfig;
use crate::rules::support::punctuation::{build_line_starts, line_and_column};
use crate::rules::support::span_utils::CharPos;

pub const ID: &str = "anchors";
pub const MESSAGE_UNDECLARED_ALIAS: &str = "found undeclared alias";
pub const MESSAGE_DUPLICATED_ANCHOR: &str = "found duplicated anchor";
pub const MESSAGE_UNUSED_ANCHOR: &str = "found unused anchor";
pub const MESSAGE_AMBIGUOUS_ANCHOR: &str = "found ambiguous anchor name";
pub const MESSAGE_AMBIGUOUS_ALIAS: &str = "found ambiguous alias name";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // independent yamllint-style toggles, not a state machine
pub struct Config {
    forbid_undeclared_aliases: bool,
    forbid_duplicated_anchors: bool,
    forbid_unused_anchors: bool,
    forbid_ambiguous_anchor_alias_names: bool,
}

impl Config {
    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        Self {
            forbid_undeclared_aliases: cfg.rule_option_bool(
                ID,
                "forbid-undeclared-aliases",
                true,
            ),
            forbid_duplicated_anchors: cfg.rule_option_bool(
                ID,
                "forbid-duplicated-anchors",
                false,
            ),
            forbid_unused_anchors: cfg.rule_option_bool(
                ID,
                "forbid-unused-anchors",
                false,
            ),
            forbid_ambiguous_anchor_alias_names: cfg.rule_option_bool(
                ID,
                "forbid-ambiguous-anchor-alias-names",
                false,
            ),
        }
    }

    #[must_use]
    pub const fn new_for_tests(
        forbid_undeclared_aliases: bool,
        forbid_duplicated_anchors: bool,
        forbid_unused_anchors: bool,
    ) -> Self {
        Self {
            forbid_undeclared_aliases,
            forbid_duplicated_anchors,
            forbid_unused_anchors,
            forbid_ambiguous_anchor_alias_names: false,
        }
    }

    #[must_use]
    pub const fn with_forbid_ambiguous_anchor_alias_names(
        mut self,
        value: bool,
    ) -> Self {
        self.forbid_ambiguous_anchor_alias_names = value;
        self
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
    let char_indices: Vec<(usize, char)> = buffer.char_indices().collect();
    let line_starts = build_line_starts(&char_indices);
    let mut doc = DocState::new();
    let mut violations = Vec::new();

    for token in Scanner::new(StrInput::new(buffer)) {
        let span = token.0;
        match token.1 {
            TokenType::DocumentStart | TokenType::DocumentEnd => {
                finish_doc(&doc, *cfg, &mut violations);
                doc = DocState::new();
            }
            TokenType::Anchor(name) => {
                let (line, column) =
                    line_and_column(&line_starts, CharPos::new(span.start.index()));
                let duplicate = doc.add_anchor(name.to_string(), line, column);
                if cfg.forbid_duplicated_anchors && duplicate {
                    violations.push(Violation {
                        line,
                        column,
                        message: format!("{MESSAGE_DUPLICATED_ANCHOR} \"{name}\""),
                    });
                }
                violations.extend(ambiguous_violation(
                    *cfg,
                    &name,
                    MESSAGE_AMBIGUOUS_ANCHOR,
                    line,
                    column,
                ));
            }
            TokenType::Alias(name) => {
                let (line, column) =
                    line_and_column(&line_starts, CharPos::new(span.start.index()));
                if !doc.mark_alias(&name) && cfg.forbid_undeclared_aliases {
                    violations.push(Violation {
                        line,
                        column,
                        message: format!("{MESSAGE_UNDECLARED_ALIAS} \"{name}\""),
                    });
                }
                violations.extend(ambiguous_violation(
                    *cfg,
                    &name,
                    MESSAGE_AMBIGUOUS_ALIAS,
                    line,
                    column,
                ));
            }
            _ => {}
        }
    }
    finish_doc(&doc, *cfg, &mut violations);
    violations
}

fn ambiguous_violation(
    cfg: Config,
    name: &str,
    message: &str,
    line: usize,
    column: usize,
) -> Option<Violation> {
    (cfg.forbid_ambiguous_anchor_alias_names && name.contains(':')).then(|| Violation {
        line,
        column,
        message: format!("{message} \"{name}\""),
    })
}

/// Only the last declaration of each name carries the live binding (yamllint's
/// name-keyed model, matching `mark_alias`); earlier same-name records are shadowed
/// re-declarations, so a name is reported unused at most once and never when used.
fn finish_doc(doc: &DocState, cfg: Config, violations: &mut Vec<Violation>) {
    if cfg.forbid_unused_anchors {
        for (index, anchor) in doc.anchors.iter().enumerate() {
            let is_last_of_name = doc
                .name_to_indices
                .get(&anchor.name)
                .and_then(|indices| indices.last())
                == Some(&index);
            if is_last_of_name && !anchor.used {
                violations.push(Violation {
                    line: anchor.line,
                    column: anchor.column,
                    message: format!("{MESSAGE_UNUSED_ANCHOR} \"{}\"", anchor.name),
                });
            }
        }
    }
}

struct DocState {
    anchors: Vec<AnchorRecord>,
    name_to_indices: HashMap<String, Vec<usize>>,
}

impl DocState {
    fn new() -> Self {
        Self {
            anchors: Vec::new(),
            name_to_indices: HashMap::new(),
        }
    }

    fn add_anchor(&mut self, name: String, line: usize, column: usize) -> bool {
        let entry_indices = self.name_to_indices.entry(name.clone()).or_default();
        let duplicate = !entry_indices.is_empty();
        let index = self.anchors.len();
        entry_indices.push(index);
        self.anchors.push(AnchorRecord {
            name,
            line,
            column,
            used: false,
        });
        duplicate
    }

    fn mark_alias(&mut self, name: &str) -> bool {
        let Some(indices) = self.name_to_indices.get(name) else {
            return false;
        };
        let last_index = *indices
            .last()
            .expect("anchor indices should contain at least one entry");
        let anchor = self
            .anchors
            .get_mut(last_index)
            .expect("anchor record must exist for referenced name");
        anchor.used = true;
        true
    }
}

struct AnchorRecord {
    name: String,
    line: usize,
    column: usize,
    used: bool,
}
