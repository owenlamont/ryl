//! `merge-keys` rule (off by default): flags the `<<` merge key, removed in YAML 1.2.
//!
//! Flagged only when YAML would merge on it: an untagged plain `<<`, or any scalar
//! tagged `!!merge` regardless of text. A quoted `"<<"` is a plain string key that
//! never merges (verified against `PyYAML` and ruamel.yaml), so it is not flagged. Shared
//! [`crate::rules::support::merge_key::is_merge_directive`], also used by
//! `key-duplicates`.
//!
//! Detection covers a scalar key node (every merge key in practice); a merge tag on a
//! non-scalar key (`!!merge {k: 1}: *base`) is not detected, since the key arrives as a
//! mapping/sequence event. No safe `--fix`: removing a merge requires inlining the
//! merged mapping's resolved values (see AGENTS.md "Rules Without A Safe `--fix`").
//!
//! Sources: YAML 1.2.2 changes page; yaml.org/type/merge.html.

use granit_parser::{Event, Parser, Span, SpannedEventReceiver};

use crate::rules::support::mapping_key_walker::Walker;
use crate::rules::support::merge_key::is_merge_directive;

pub const ID: &str = "merge-keys";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

#[must_use]
pub fn check(buffer: &str) -> Vec<Violation> {
    let mut parser = Parser::new_from_str(buffer);
    let mut receiver = MergeKeysReceiver {
        walker: Walker::new(),
        violations: Vec::new(),
    };
    let _ = parser.load(&mut receiver, true);
    receiver.violations
}

struct MergeKeysReceiver {
    walker: Walker<()>,
    violations: Vec<Violation>,
}

impl SpannedEventReceiver<'_> for MergeKeysReceiver {
    fn on_event(&mut self, event: Event<'_>, span: Span) {
        match event {
            Event::StreamStart | Event::DocumentStart(_) | Event::DocumentEnd => {
                self.walker.reset();
            }
            Event::MappingStart(..) => self.walker.enter_mapping((), ()),
            Event::SequenceStart(..) => self.walker.enter_sequence(()),
            Event::MappingEnd | Event::SequenceEnd => self.walker.exit_container(),
            Event::Scalar(value, style, _anchor, tag) => {
                let context = self.walker.begin_node();
                if context.key_root()
                    && is_merge_directive(value.as_ref(), style, tag.as_ref())
                {
                    self.violations.push(Violation {
                        line: span.start.line(),
                        column: span.start.col() + 1,
                        message: format!("forbidden merge key \"{value}\""),
                    });
                }
                self.walker.finish_node(context);
            }
            Event::Alias(_) => self.walker.skip_node(),
            Event::Comment(_, _) | Event::StreamEnd | Event::Nothing => {}
        }
    }
}
