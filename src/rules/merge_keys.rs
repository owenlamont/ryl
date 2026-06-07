//! `merge-keys` rule &mdash; flags the `<<` merge key (issue #256).
//!
//! The merge key is a YAML 1.1 type (yaml.org/type/merge.html): `<<: *base`
//! splices the keys of the mapping `*base` resolves to into the current mapping.
//! YAML 1.2 removed it &mdash; "The merge `<<` and value `=` special mapping keys
//! have been removed" (YAML 1.2.2 changes page) &mdash; and ryl resolves scalars
//! under the YAML 1.2 core schema, where `<<` is an ordinary string key. Whether a
//! `<<` key performs a merge therefore depends entirely on the parsing library, so
//! it is a portability trap; this off-by-default rule lets portability-sensitive
//! repositories forbid it.
//!
//! A key is flagged only when YAML would actually merge on it: an untagged plain
//! `<<`, or ANY scalar explicitly tagged `!!merge` regardless of its text
//! (`!!merge foo` merges just like `<<`). This is the shared
//! [`crate::rules::support::merge_key::is_merge_directive`] definition, also used
//! by `key-duplicates`. A quoted `"<<"` is a plain string key that never merges
//! (verified against `PyYAML` and ruamel.yaml) and is the portable way to use the
//! literal text, so it is not flagged.
//!
//! Sources: YAML 1.2.2 changes page; YAML merge type (yaml.org/type/merge.html).
//! There is no safe `--fix`: removing a merge requires inlining the merged
//! mapping's resolved values (see AGENTS.md "Rules Without A Safe `--fix`").

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
