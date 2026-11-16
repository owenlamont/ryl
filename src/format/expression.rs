use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use regex::Regex;
use ryl_yaml_parser::Event;

use crate::config::PathGlob;
use crate::config_schema::ProseEntry;
use crate::rules::quoted_strings::LayoutStyle;
use crate::rules::support::key_path::{self, Selector, Step};
use crate::rules::support::mapping_key_walker::Walker;
use crate::rules::support::span_utils::{BytePos, marker_byte_offset};
use crate::text_version::Analysis;
use crate::yaml_dom::YamlOwned;

#[derive(Debug)]
pub(crate) struct Preference {
    files: Vec<PathGlob>,
    path: Vec<Selector>,
    tokens: Vec<String>,
    protected: Vec<Regex>,
}

impl Preference {
    pub(crate) fn new(entry: &ProseEntry, base: &Path, insensitive: bool) -> Self {
        Self {
            files: entry
                .files
                .iter()
                .map(|glob| {
                    PathGlob::new(glob, base, insensitive)
                        .expect("validated prose file glob")
                })
                .collect(),
            path: key_path::parse(&entry.path).expect("validated prose path"),
            tokens: entry.break_before.clone(),
            protected: entry
                .protected_patterns
                .iter()
                .map(|pattern| Regex::new(pattern).expect("validated protected regex"))
                .collect(),
        }
    }

    pub(crate) fn matches_file(&self, path: &Path) -> bool {
        PathGlob::selects(&self.files, path)
    }
}

#[derive(Clone)]
pub(crate) struct Selected {
    preference: Arc<Preference>,
    value: String,
    protected: Vec<Range<usize>>,
    cursor: usize,
}

impl Selected {
    fn new(preference: Arc<Preference>, value: String) -> Self {
        let mut protected: Vec<Range<usize>> = preference
            .protected
            .iter()
            .flat_map(|pattern| {
                pattern
                    .find_iter(&value)
                    .filter(|found| !found.is_empty())
                    .map(|found| found.range())
            })
            .collect();
        protected.sort_unstable_by_key(|range| range.start);
        // Merging makes both endpoints monotonic for boundary lookup.
        protected.dedup_by(|next, previous| {
            if next.start <= previous.end {
                previous.end = previous.end.max(next.end);
                true
            } else {
                false
            }
        });
        Self {
            preference,
            value,
            protected,
            cursor: 0,
        }
    }

    fn overlaps_protected(&self, start: usize, end: usize) -> bool {
        let index = self.protected.partition_point(|range| range.end <= start);
        self.protected
            .get(index)
            .is_some_and(|range| range.start < end)
    }

    pub(crate) fn boundaries(
        &mut self,
        text: &str,
        scalar: &Range<usize>,
        style: LayoutStyle,
    ) -> Vec<usize> {
        let mut decoded = String::new();
        let mut offsets = Vec::new();
        let start = scalar.start.min(scalar.end);
        let start = start + text[start..scalar.end].len()
            - text[start..scalar.end]
                .trim_start_matches([' ', '\t'])
                .len();
        let mut chars = text[start..scalar.end].char_indices().peekable();
        while let Some((byte, ch)) = chars.next() {
            let at = byte + start;
            if ch == ' ' {
                offsets.push((BytePos::new(at), decoded.len()));
            }
            if style.is_double() && ch == '\\' {
                let Some((_, escape)) = chars.next() else {
                    break;
                };
                let digits = match escape {
                    'x' => 2,
                    'u' => 4,
                    'U' => 8,
                    _ => 0,
                };
                for _ in 0..digits {
                    chars.next();
                }
                let end = chars.peek().map_or(scalar.end, |&(byte, _)| byte + start);
                let spelling = format!("\"{}\"", &text[at..end]);
                let values = YamlOwned::load_from_str(&spelling)
                    .expect("projected YAML escape was validated before folding");
                let value = values[0]
                    .as_str()
                    .expect("a quoted YAML escape decodes to a string");
                decoded.push_str(value);
            } else {
                if style.quote_width() == 1
                    && !style.is_double()
                    && ch == '\''
                    && chars.peek().is_some_and(|&(_, next)| next == '\'')
                {
                    chars.next();
                }
                decoded.push(ch);
            }
        }
        let cursor = self.cursor;
        let Some(skip) = self.value[cursor..].find(&decoded) else {
            self.cursor = self.value.len();
            return Vec::new();
        };
        let origin = cursor + skip;
        self.cursor = origin + decoded.len();
        offsets
            .into_iter()
            .filter_map(|(byte, at)| {
                self.preference
                    .tokens
                    .iter()
                    .any(|token| {
                        decoded[at + 1..].starts_with(token)
                            && !self.overlaps_protected(
                                origin + at,
                                origin + at + 1 + token.len(),
                            )
                    })
                    .then_some(byte.get())
            })
            .collect()
    }
}

pub(crate) fn select(
    input: Analysis<'_>,
    entries: &[Arc<Preference>],
) -> Vec<(Range<usize>, Selected)> {
    let mut walker: Walker<Option<String>, Vec<Step>> = Walker::new();
    let mut selected = Vec::new();
    for (event, span) in input.event_iter() {
        match event {
            Event::StreamStart | Event::DocumentStart(..) | Event::DocumentEnd => {
                walker.reset();
            }
            Event::MappingStart(..) | Event::SequenceStart(..) => {
                let path = child_path(&mut walker);
                if matches!(event, Event::MappingStart(..)) {
                    walker.enter_mapping(None, path);
                } else {
                    walker.enter_sequence(path);
                }
            }
            Event::MappingEnd | Event::SequenceEnd => walker.exit_container(),
            Event::Scalar(value, ..) => {
                let path = child_path(&mut walker);
                let context = walker.begin_node();
                if context.key_root() {
                    let key = walker
                        .current_mapping_mut()
                        .expect("a key root belongs to a mapping");
                    *key = Some(value.into_owned());
                } else if !context.active()
                    && let Some(entry) = entries
                        .iter()
                        .find(|entry| key_path::matches(&entry.path, &path))
                {
                    selected.push((
                        marker_byte_offset(span.start).get()
                            ..marker_byte_offset(span.end).get(),
                        Selected::new(Arc::clone(entry), value.into_owned()),
                    ));
                }
                walker.finish_node(context);
            }
            Event::Alias(_) => {
                child_path(&mut walker);
                walker.skip_node();
            }
            _ => {}
        }
    }
    selected
}

fn child_path(walker: &mut Walker<Option<String>, Vec<Step>>) -> Vec<Step> {
    let key = walker.expects_key();
    let step = match walker.current_mapping_mut() {
        Some(name) if key => {
            *name = None;
            Step::Opaque
        }
        Some(name) => name.take().map_or(Step::Opaque, Step::Key),
        None => Step::Item,
    };
    walker
        .current_metadata_mut()
        .map_or_else(Vec::new, |path| [path.as_slice(), &[step]].concat())
}
