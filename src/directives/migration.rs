use std::ops::Range;

use crate::rules::support::{
    comments_scan::collect_comments_with_analysis,
    line_syntax::split_lines_preserve_endings,
    span_utils::{BytePos, apply_replacements, marker_byte_offset},
};
use crate::text_version::Analysis;

use super::{DISABLE_FILE, SEGMENT_BREAK, parse_segment};

mod formatter;
mod markdown;

pub use formatter::{FormatterComments, Migration};
pub use markdown::migrate_markdown_comments;

pub(crate) fn migrate_source_comments(
    buffer: &str,
    source: FormatterComments<'_>,
    kind: crate::config::SourceKind,
    permit: bool,
) -> Migration {
    match kind {
        crate::config::SourceKind::Yaml => formatter::migrate(
            crate::text_version::TextVersion::new(buffer).analysis(),
            source,
            permit,
        ),
        crate::config::SourceKind::Markdown => {
            markdown::migrate(buffer, source, permit)
        }
    }
}

#[must_use]
pub fn migrate_comments(buffer: &str, source: FormatterComments<'_>) -> Migration {
    formatter::migrate(
        crate::text_version::TextVersion::new(buffer).analysis(),
        source,
        true,
    )
}

pub(super) struct LegacyUse {
    pub(super) line: usize,
    tool: Range<usize>,
}

impl LegacyUse {
    fn replacement(self) -> crate::text_version::edits::Edit {
        (
            BytePos::new(self.tool.start),
            BytePos::new(self.tool.end),
            "ryl".to_owned(),
        )
    }
}

pub(super) fn legacy_uses(input: Analysis<'_>) -> Vec<LegacyUse> {
    let buffer = input.text();
    if !buffer.contains("yamllint ") {
        return Vec::new();
    }
    let first = split_lines_preserve_endings(buffer)
        .next()
        .map_or("", |(_, content, _)| content);
    let header = first
        .find("yamllint")
        .filter(|_| DISABLE_FILE.is_match(first))
        .map(|start| LegacyUse {
            line: 1,
            tool: start..start + "yamllint".len(),
        });
    header
        .into_iter()
        .chain(
            collect_comments_with_analysis(input)
                .into_iter()
                .filter(|comment| {
                    comment.text.starts_with(" yamllint ")
                        && !SEGMENT_BREAK.is_match(&comment.text)
                        && parse_segment(&comment.text).is_some()
                })
                .map(|comment| {
                    let start = marker_byte_offset(comment.span.start).get() + 2;
                    LegacyUse {
                        line: comment.span.start.line(),
                        tool: start..start + "yamllint".len(),
                    }
                }),
        )
        .collect()
}

pub(super) fn rewrite_legacy(input: Analysis<'_>) -> Option<String> {
    let uses = legacy_uses(input);
    (!uses.is_empty()).then(|| {
        apply_replacements(
            input.text(),
            uses.into_iter().map(LegacyUse::replacement).collect(),
        )
    })
}
