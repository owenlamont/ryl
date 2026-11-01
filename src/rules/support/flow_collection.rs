mod input;
mod target;
pub(crate) use input::{CollectionKind, RetainedClosing};
pub(crate) use target::ClosingPolicy;

pub(crate) const fn retained_closing(
    bytes: &[u8],
    comma: usize,
    end: Option<usize>,
) -> RetainedClosing<'_> {
    RetainedClosing::new(bytes, comma, end)
}

use crate::config::YamlLintConfig;
use crate::rules::support::punctuation::{
    AfterResult, Columns, compute_spaces_after, skip_comment,
};

macro_rules! define_rule {
    (
        $rule_id:literal,
        $open:literal,
        $close:literal,
        $forbid_message:literal,
        $min_message:literal,
        $max_message:literal,
        $min_empty_message:literal,
        $max_empty_message:literal $(,)?
    ) => {
        pub use $crate::rules::support::flow_collection::{Forbid, Violation};

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct Config($crate::rules::support::flow_collection::Config);

        pub const ID: &str = $rule_id;

        pub const FORBID_MESSAGE: &str = $forbid_message;

        const DESCRIPTOR:
            $crate::rules::support::flow_collection::FlowCollectionDescriptor =
            $crate::rules::support::flow_collection::FlowCollectionDescriptor {
                open: $open,
                close: $close,
                forbid_message: FORBID_MESSAGE,
                min_message: $min_message,
                max_message: $max_message,
                min_empty_message: $min_empty_message,
                max_empty_message: $max_empty_message,
            };

        #[must_use]
        pub fn check(buffer: &str, cfg: &Config) -> Vec<Violation> {
            check_with_analysis(
                $crate::text_version::TextVersion::new(buffer).analysis(),
                cfg,
            )
        }

        pub(crate) fn check_with_analysis(
            input: $crate::text_version::Analysis<'_>,
            cfg: &Config,
        ) -> Vec<Violation> {
            $crate::rules::support::flow_collection::check(
                input,
                cfg.inner(),
                &DESCRIPTOR,
            )
        }

        #[must_use]
        pub fn fix(buffer: &str, cfg: &Config) -> Option<String> {
            fix_with_analysis(
                $crate::text_version::TextVersion::new(buffer).analysis(),
                cfg,
            )
        }

        pub(crate) fn fix_with_analysis(
            input: $crate::text_version::Analysis<'_>,
            cfg: &Config,
        ) -> Option<String> {
            $crate::text_version::edits::render(input.text(), edits(input, cfg))
        }

        pub(crate) fn edits(
            input: $crate::text_version::Analysis<'_>,
            cfg: &Config,
        ) -> Vec<$crate::text_version::edits::Edit> {
            $crate::rules::support::flow_collection::edits(
                input,
                cfg.inner(),
                &DESCRIPTOR,
            )
        }

        pub(crate) fn format_sites<'a>(
            cfg: &'a Config,
            source: $crate::text_version::Analysis<'a>,
        ) -> Option<$crate::rules::support::flow_collection::Formatting<'a>> {
            $crate::rules::support::flow_collection::Formatting::new(
                source,
                cfg.inner(),
                &DESCRIPTOR,
            )
        }

        pub(crate) fn closing_policy<'a>(
            cfg: Option<&'a Config>,
            permission: $crate::format::layout::Permission<'a>,
        ) -> $crate::rules::support::flow_collection::ClosingPolicy<'a> {
            $crate::rules::support::flow_collection::ClosingPolicy::new(
                cfg.map(Config::inner),
                permission,
            )
        }

        impl Config {
            #[must_use]
            pub fn resolve(cfg: &$crate::config::YamlLintConfig) -> Self {
                Self(
                    $crate::rules::support::flow_collection::Config::resolve_for(
                        cfg, ID,
                    ),
                )
            }

            #[must_use]
            pub const fn new(
                forbid: Forbid,
                min_spaces_inside: i64,
                max_spaces_inside: i64,
                min_spaces_inside_empty: i64,
                max_spaces_inside_empty: i64,
            ) -> Self {
                Self($crate::rules::support::flow_collection::Config::new(
                    forbid,
                    min_spaces_inside,
                    max_spaces_inside,
                    min_spaces_inside_empty,
                    max_spaces_inside_empty,
                ))
            }

            #[must_use]
            pub const fn effective_min_empty(&self) -> i64 {
                self.0.effective_min_empty()
            }

            #[must_use]
            pub const fn effective_max_empty(&self) -> i64 {
                self.0.effective_max_empty()
            }

            #[must_use]
            pub const fn forbid(&self) -> Forbid {
                self.0.forbid()
            }

            #[must_use]
            pub const fn min_spaces_inside(&self) -> i64 {
                self.0.min_spaces_inside()
            }

            #[must_use]
            pub const fn max_spaces_inside(&self) -> i64 {
                self.0.max_spaces_inside()
            }

            const fn inner(&self) -> &$crate::rules::support::flow_collection::Config {
                &self.0
            }
        }
    };
}

pub(crate) use define_rule;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Forbid {
    None,
    All,
    NonEmpty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    forbid: Forbid,
    min_spaces_inside: i64,
    max_spaces_inside: i64,
    min_spaces_inside_empty: i64,
    max_spaces_inside_empty: i64,
}

impl Config {
    const DEFAULT_MIN_SPACES_INSIDE: i64 = 0;
    const DEFAULT_MAX_SPACES_INSIDE: i64 = 0;
    const DEFAULT_MIN_SPACES_INSIDE_EMPTY: i64 = -1;
    const DEFAULT_MAX_SPACES_INSIDE_EMPTY: i64 = -1;

    #[must_use]
    pub fn resolve_for(cfg: &YamlLintConfig, rule_id: &str) -> Self {
        let forbid = cfg
            .rule_option(rule_id, "forbid")
            .map_or(Forbid::None, |node| match (node.as_bool(), node.as_str()) {
                (Some(true), _) => Forbid::All,
                (None, Some("non-empty")) => Forbid::NonEmpty,
                _ => Forbid::None,
            });

        Self {
            forbid,
            min_spaces_inside: cfg.rule_option_int(
                rule_id,
                "min-spaces-inside",
                Self::DEFAULT_MIN_SPACES_INSIDE,
            ),
            max_spaces_inside: cfg.rule_option_int(
                rule_id,
                "max-spaces-inside",
                Self::DEFAULT_MAX_SPACES_INSIDE,
            ),
            min_spaces_inside_empty: cfg.rule_option_int(
                rule_id,
                "min-spaces-inside-empty",
                Self::DEFAULT_MIN_SPACES_INSIDE_EMPTY,
            ),
            max_spaces_inside_empty: cfg.rule_option_int(
                rule_id,
                "max-spaces-inside-empty",
                Self::DEFAULT_MAX_SPACES_INSIDE_EMPTY,
            ),
        }
    }

    #[must_use]
    pub const fn new(
        forbid: Forbid,
        min_spaces_inside: i64,
        max_spaces_inside: i64,
        min_spaces_inside_empty: i64,
        max_spaces_inside_empty: i64,
    ) -> Self {
        Self {
            forbid,
            min_spaces_inside,
            max_spaces_inside,
            min_spaces_inside_empty,
            max_spaces_inside_empty,
        }
    }

    #[must_use]
    pub const fn effective_min_empty(&self) -> i64 {
        if self.min_spaces_inside_empty >= 0 {
            self.min_spaces_inside_empty
        } else {
            self.min_spaces_inside
        }
    }

    #[must_use]
    pub const fn effective_max_empty(&self) -> i64 {
        if self.max_spaces_inside_empty >= 0 {
            self.max_spaces_inside_empty
        } else {
            self.max_spaces_inside
        }
    }

    #[must_use]
    pub const fn forbid(&self) -> Forbid {
        self.forbid
    }

    #[must_use]
    pub const fn min_spaces_inside(&self) -> i64 {
        self.min_spaces_inside
    }

    #[must_use]
    pub const fn max_spaces_inside(&self) -> i64 {
        self.max_spaces_inside
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

pub struct FlowCollectionDescriptor {
    pub open: u8,
    pub close: u8,
    pub forbid_message: &'static str,
    pub min_message: &'static str,
    pub max_message: &'static str,
    pub min_empty_message: &'static str,
    pub max_empty_message: &'static str,
}

#[derive(Debug, Clone, Copy)]
struct CollectionState {
    is_empty: bool,
}

pub(crate) struct Edits<'a> {
    bytes: &'a [u8],
    descriptor: &'a FlowCollectionDescriptor,
    planner: target::Planner<'a>,
}

impl<'a> Edits<'a> {
    pub(crate) fn new(
        bytes: &'a [u8],
        config: &'a Config,
        descriptor: &'a FlowCollectionDescriptor,
    ) -> Option<Self> {
        target::Planner::new(config).map(|planner| Self {
            bytes,
            descriptor,
            planner,
        })
    }

    pub(crate) fn push(&mut self, idx: usize, ch: u8) {
        if ch == self.descriptor.open {
            self.planner.open(input::Opening::new(
                self.bytes,
                idx,
                self.descriptor.close,
            ));
        } else if ch == self.descriptor.close {
            self.planner.close(input::Closing::new(self.bytes, idx));
        } else if !matches!(ch, b',' | b' ' | b'\t' | b'\n' | b'\r') {
            self.planner.content();
        }
    }

    pub(crate) fn finish(self) -> Vec<super::token_spacing::SpaceEdit> {
        self.planner.finish()
    }
}

#[derive(Clone, Copy)]
struct SpacingMessages<'a> {
    min: &'a str,
    max: &'a str,
}

fn sites<'a>(
    input: crate::text_version::Analysis<'a>,
    desc: &FlowCollectionDescriptor,
) -> std::borrow::Cow<'a, [(usize, u8)]> {
    if desc.open == b'[' && input.text().contains("{{") {
        std::borrow::Cow::Owned(super::punctuation::flow_sites(input, false))
    } else {
        std::borrow::Cow::Borrowed(input.flow_sites())
    }
}

#[must_use]
pub fn check(
    input: crate::text_version::Analysis<'_>,
    cfg: &Config,
    desc: &FlowCollectionDescriptor,
) -> Vec<Violation> {
    let bytes = input.text().as_bytes();
    let columns = Columns::new(bytes, input.byte_line_starts());
    let mut stack: Vec<CollectionState> = Vec::new();
    let mut violations = Vec::new();
    for &(idx, ch) in sites(input, desc).iter() {
        if ch == desc.close {
            handle_close(cfg, desc, bytes, idx, &columns, &mut stack, &mut violations);
            continue;
        }
        if ch != b','
            && let Some(state) = stack.last_mut()
        {
            state.is_empty = false;
        }
        if ch == desc.open {
            handle_open(cfg, desc, bytes, idx, &columns, &mut stack, &mut violations);
        }
    }
    violations
}

#[must_use]
pub(crate) fn edits(
    input: crate::text_version::Analysis<'_>,
    cfg: &Config,
    desc: &FlowCollectionDescriptor,
) -> Vec<crate::text_version::edits::Edit> {
    let buffer = input.text();
    let Some(mut edits) = Edits::new(buffer.as_bytes(), cfg, desc) else {
        return Vec::new();
    };

    for &(idx, ch) in sites(input, desc).iter() {
        edits.push(idx, ch);
    }
    edits
        .finish()
        .into_iter()
        .map(|(from, to, spaces)| (from, to, " ".repeat(spaces)))
        .collect()
}

fn handle_open(
    cfg: &Config,
    desc: &FlowCollectionDescriptor,
    bytes: &[u8],
    idx: usize,
    columns: &Columns<'_>,
    stack: &mut Vec<CollectionState>,
    violations: &mut Vec<Violation>,
) {
    let (line, column) = columns.at(idx);
    let next_significant = next_significant_index(bytes, idx);

    let mut skip_open_check = false;
    match cfg.forbid() {
        Forbid::All => {
            violations.push(Violation {
                line,
                column: column + 1,
                message: desc.forbid_message.to_string(),
            });
            skip_open_check = true;
        }
        Forbid::NonEmpty => {
            let is_empty = matches!(next_significant.map(|j| bytes[j]), Some(close) if close == desc.close);
            if !is_empty {
                violations.push(Violation {
                    line,
                    column: column + 1,
                    message: desc.forbid_message.to_string(),
                });
                skip_open_check = true;
            }
        }
        Forbid::None => {}
    }

    let mut state = CollectionState {
        is_empty: matches!(next_significant.map(|j| bytes[j]), Some(close) if close == desc.close),
    };

    if !skip_open_check
        && let AfterResult::SameLine { spaces, next_idx } =
            compute_spaces_after(bytes, idx)
    {
        let (line, next_column) = columns.at(next_idx);
        if state.is_empty && bytes[next_idx] == desc.close {
            record_after_spacing(
                cfg.effective_min_empty(),
                cfg.effective_max_empty(),
                spaces,
                line,
                next_column,
                SpacingMessages {
                    min: desc.min_empty_message,
                    max: desc.max_empty_message,
                },
                violations,
            );
        } else {
            state.is_empty = false;
            record_after_spacing(
                cfg.min_spaces_inside(),
                cfg.max_spaces_inside(),
                spaces,
                line,
                next_column,
                SpacingMessages {
                    min: desc.min_message,
                    max: desc.max_message,
                },
                violations,
            );
        }
    }

    stack.push(state);
}

fn handle_close(
    cfg: &Config,
    desc: &FlowCollectionDescriptor,
    bytes: &[u8],
    idx: usize,
    columns: &Columns<'_>,
    stack: &mut Vec<CollectionState>,
    violations: &mut Vec<Violation>,
) {
    let Some(state) = stack.pop() else {
        return;
    };

    if state.is_empty {
        return;
    }

    if let Some((spaces, _start_idx)) = compute_spaces_before_close(bytes, idx) {
        let spaces_i64 = i64::try_from(spaces).unwrap_or(i64::MAX);
        let (line, close_column) = columns.at(idx);
        if cfg.max_spaces_inside() >= 0 && spaces_i64 > cfg.max_spaces_inside() {
            let highlight = close_column.saturating_sub(1).max(1);
            violations.push(Violation {
                line,
                column: highlight,
                message: desc.max_message.to_string(),
            });
        }
        if cfg.min_spaces_inside() >= 0 && spaces_i64 < cfg.min_spaces_inside() {
            violations.push(Violation {
                line,
                column: close_column,
                message: desc.min_message.to_string(),
            });
        }
    }
}

fn record_after_spacing(
    min: i64,
    max: i64,
    spaces: usize,
    line: usize,
    next_column: usize,
    messages: SpacingMessages<'_>,
    violations: &mut Vec<Violation>,
) {
    let spaces_i64 = i64::try_from(spaces).unwrap_or(i64::MAX);
    if max >= 0 && spaces_i64 > max {
        let highlight = next_column.saturating_sub(1).max(1);
        violations.push(Violation {
            line,
            column: highlight,
            message: messages.max.to_string(),
        });
    }
    if min >= 0 && spaces_i64 < min {
        violations.push(Violation {
            line,
            column: next_column,
            message: messages.min.to_string(),
        });
    }
}

fn compute_spaces_before_close(
    bytes: &[u8],
    close_idx: usize,
) -> Option<(usize, usize)> {
    let mut spaces = 0usize;
    let mut idx = close_idx;
    loop {
        idx = idx
            .checked_sub(1)
            .expect("closing delimiter should have a preceding opening delimiter");
        match bytes[idx] {
            b' ' | b'\t' => spaces += 1,
            b'\n' | b'\r' | b'#' => return None,
            _ => return Some((spaces, idx + 1)),
        }
    }
}

fn next_significant_index(bytes: &[u8], open_idx: usize) -> Option<usize> {
    let mut idx = open_idx + 1;
    while idx < bytes.len() {
        match bytes[idx] {
            b' ' | b'\t' | b'\n' | b'\r' => idx += 1,
            b'#' => idx = skip_comment(bytes, idx) + 1,
            _ => return Some(idx),
        }
    }
    None
}

pub(crate) enum FormattedEdits {
    Spaces(Vec<super::token_spacing::SpaceEdit>),
    Text(Vec<crate::text_version::edits::Edit>),
}

pub(crate) struct Formatting<'a> {
    source: crate::text_version::Analysis<'a>,
    config: &'a Config,
    descriptor: &'a FlowCollectionDescriptor,
    traversal: target::Traversal,
    streamed: Option<Edits<'a>>,
}

impl<'a> Formatting<'a> {
    pub(crate) fn new(
        source: crate::text_version::Analysis<'a>,
        config: &'a Config,
        descriptor: &'a FlowCollectionDescriptor,
    ) -> Option<Self> {
        let traversal = target::Traversal::select(
            &input::Traversal::new(source.text(), descriptor),
            config,
        )?;
        let streamed = matches!(traversal, target::Traversal::Flow)
            .then(|| Edits::new(source.text().as_bytes(), config, descriptor))
            .flatten();
        Some(Self {
            source,
            config,
            descriptor,
            traversal,
            streamed,
        })
    }
    pub(crate) fn push(&mut self, at: usize, ch: u8) {
        if let Some(edits) = &mut self.streamed {
            edits.push(at, ch);
        }
    }
    pub(crate) fn finish(self) -> FormattedEdits {
        match self.traversal {
            target::Traversal::Flow => FormattedEdits::Spaces(
                self.streamed.map_or_else(Vec::new, Edits::finish),
            ),
            target::Traversal::Template => {
                FormattedEdits::Text(edits(self.source, self.config, self.descriptor))
            }
        }
    }
}
