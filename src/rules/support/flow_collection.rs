use crate::config::YamlLintConfig;
use crate::rules::support::punctuation::{
    build_byte_line_starts, byte_line_and_column, collect_scalar_ranges, skip_comment,
    target_spacing, template_double_curly_end,
};
use crate::rules::support::span_utils::{
    BytePos, apply_replacements, containing_byte_scalar_range,
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
            $crate::rules::support::flow_collection::check(
                buffer,
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
            $crate::rules::support::flow_collection::fix(
                input,
                cfg.inner(),
                &DESCRIPTOR,
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

#[derive(Debug, PartialEq, Eq)]
enum AfterResult {
    SameLine { spaces: usize, next_idx: usize },
    Ignored,
}

#[derive(Clone, Copy)]
struct SpacingMessages<'a> {
    min: &'a str,
    max: &'a str,
}

#[must_use]
pub fn check(
    buffer: &str,
    cfg: &Config,
    desc: &FlowCollectionDescriptor,
) -> Vec<Violation> {
    if buffer.is_empty() {
        return Vec::new();
    }

    let scalar_ranges = collect_scalar_ranges(buffer);

    let bytes = buffer.as_bytes();
    let line_starts = build_byte_line_starts(buffer);

    let mut range_idx = 0usize;
    let mut idx = 0usize;
    let mut stack: Vec<CollectionState> = Vec::new();
    let mut violations = Vec::new();

    while idx < bytes.len() {
        if let Some(range) =
            containing_byte_scalar_range(&scalar_ranges, &mut range_idx, idx)
        {
            if idx == range.start.get()
                && let Some(state) = stack.last_mut()
            {
                state.is_empty = false;
            }
            idx = range.end.get();
            continue;
        }

        let ch = bytes[idx];
        if desc.open == b'{'
            && let Some(next_idx) = template_double_curly_end(bytes, idx)
        {
            idx = next_idx;
            continue;
        }
        if ch == desc.open {
            if let Some(state) = stack.last_mut() {
                state.is_empty = false;
            }
            handle_open(
                cfg,
                desc,
                bytes,
                idx,
                &line_starts,
                &mut stack,
                &mut violations,
            );
        } else if ch == desc.close {
            handle_close(
                cfg,
                desc,
                bytes,
                idx,
                &line_starts,
                &mut stack,
                &mut violations,
            );
        } else {
            match ch {
                b'#' => {
                    idx = skip_comment(bytes, idx);
                    continue;
                }
                b',' | b' ' | b'\t' | b'\n' => {}
                b'\r' => {
                    if idx + 1 < bytes.len() && bytes[idx + 1] == b'\n' {
                        idx += 1;
                    }
                }
                _ => {
                    if let Some(state) = stack.last_mut() {
                        state.is_empty = false;
                    }
                }
            }
        }

        idx += 1;
    }

    violations
}

#[must_use]
pub fn fix(
    input: crate::text_version::Analysis<'_>,
    cfg: &Config,
    desc: &FlowCollectionDescriptor,
) -> Option<String> {
    let buffer = input.text();
    if buffer.is_empty() || cfg.forbid() != Forbid::None {
        return None;
    }

    let scalar_ranges = super::punctuation::byte_scalar_ranges(input);

    let bytes = input.text().as_bytes();

    let mut range_idx = 0usize;
    let mut idx = 0usize;
    let mut stack: Vec<CollectionState> = Vec::new();
    let mut replacements: Vec<(BytePos, BytePos, String)> = Vec::new();

    while idx < bytes.len() {
        if let Some(range) =
            containing_byte_scalar_range(&scalar_ranges, &mut range_idx, idx)
        {
            if idx == range.start.get()
                && let Some(state) = stack.last_mut()
            {
                state.is_empty = false;
            }
            idx = range.end.get();
            continue;
        }

        let ch = bytes[idx];
        if desc.open == b'{'
            && let Some(next_idx) = template_double_curly_end(bytes, idx)
        {
            idx = next_idx;
            continue;
        }

        if ch == desc.open {
            if let Some(state) = stack.last_mut() {
                state.is_empty = false;
            }
            let state = fix_open(cfg, desc, bytes, idx, &mut replacements);
            stack.push(state);
        } else if ch == desc.close {
            fix_close(cfg, bytes, idx, &mut stack, &mut replacements);
        } else {
            match ch {
                b'#' => {
                    idx = skip_comment(bytes, idx);
                    continue;
                }
                b',' | b' ' | b'\t' | b'\n' | b'\r' => {}
                _ => {
                    if let Some(state) = stack.last_mut() {
                        state.is_empty = false;
                    }
                }
            }
        }

        idx += 1;
    }

    if replacements.is_empty() {
        return None;
    }
    Some(apply_replacements(buffer, replacements))
}

fn handle_open(
    cfg: &Config,
    desc: &FlowCollectionDescriptor,
    bytes: &[u8],
    idx: usize,
    line_starts: &[usize],
    stack: &mut Vec<CollectionState>,
    violations: &mut Vec<Violation>,
) {
    let (line, column) = byte_line_and_column(bytes, line_starts, idx);
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
            compute_spaces_after_open(bytes, idx)
    {
        let (line, next_column) = byte_line_and_column(bytes, line_starts, next_idx);
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

fn fix_open(
    cfg: &Config,
    desc: &FlowCollectionDescriptor,
    bytes: &[u8],
    idx: usize,
    replacements: &mut Vec<(BytePos, BytePos, String)>,
) -> CollectionState {
    let next_significant = next_significant_index(bytes, idx);
    let mut state = CollectionState {
        is_empty: matches!(next_significant.map(|j| bytes[j]), Some(close) if close == desc.close),
    };

    if let AfterResult::SameLine { spaces, next_idx } =
        compute_spaces_after_open(bytes, idx)
    {
        let target = if state.is_empty && bytes[next_idx] == desc.close {
            target_spacing(spaces, cfg.effective_min_empty(), cfg.effective_max_empty())
        } else {
            state.is_empty = false;
            target_spacing(spaces, cfg.min_spaces_inside(), cfg.max_spaces_inside())
        };

        if target != spaces {
            replacements.push((
                BytePos::new(idx + 1),
                BytePos::new(next_idx),
                " ".repeat(target),
            ));
        }
    }

    state
}

fn handle_close(
    cfg: &Config,
    desc: &FlowCollectionDescriptor,
    bytes: &[u8],
    idx: usize,
    line_starts: &[usize],
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
        let (line, close_column) = byte_line_and_column(bytes, line_starts, idx);
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

fn fix_close(
    cfg: &Config,
    bytes: &[u8],
    idx: usize,
    stack: &mut Vec<CollectionState>,
    replacements: &mut Vec<(BytePos, BytePos, String)>,
) {
    let Some(state) = stack.pop() else {
        return;
    };

    if state.is_empty {
        return;
    }

    let Some((spaces, start_idx)) = compute_spaces_before_close(bytes, idx) else {
        return;
    };
    let target =
        target_spacing(spaces, cfg.min_spaces_inside(), cfg.max_spaces_inside());
    if target == spaces {
        return;
    }
    replacements.push((
        BytePos::new(start_idx),
        BytePos::new(idx),
        " ".repeat(target),
    ));
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

fn compute_spaces_after_open(bytes: &[u8], open_idx: usize) -> AfterResult {
    let mut spaces = 0usize;
    let mut idx = open_idx + 1;
    while idx < bytes.len() {
        match bytes[idx] {
            b' ' | b'\t' => {
                spaces += 1;
                idx += 1;
            }
            b'\n' | b'\r' | b'#' => return AfterResult::Ignored,
            _ => {
                return AfterResult::SameLine {
                    spaces,
                    next_idx: idx,
                };
            }
        }
    }
    AfterResult::Ignored
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
            b' ' | b'\t' | b'\n' => idx += 1,
            b'\r' => {
                if idx + 1 < bytes.len() && bytes[idx + 1] == b'\n' {
                    idx += 2;
                } else {
                    idx += 1;
                }
            }
            b'#' => {
                idx = skip_comment(bytes, idx);
                if idx >= bytes.len() {
                    continue;
                }
                idx += 1;
            }
            _ => return Some(idx),
        }
    }
    None
}
