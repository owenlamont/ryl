use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag};

use crate::markdown_embed::{MarkdownSources, RegionKind, extract_regions};
use crate::text_version::TextVersion;

use super::{FormatterComments, Migration, formatter};

#[must_use]
pub fn migrate_markdown_comments(
    buffer: &str,
    source: FormatterComments<'_>,
) -> Migration {
    migrate(buffer, source, true)
}

pub(super) fn migrate(
    buffer: &str,
    source: FormatterComments<'_>,
    permit: bool,
) -> Migration {
    if crate::markdown_embed::markdown_has_unsupported_cr(buffer) {
        return Migration {
            converted: 0,
            output: None,
            retained_lines: Vec::new(),
        };
    }
    let mut eligible = Vec::new();
    let mut ignored = Vec::new();
    for (event, range) in Parser::new_ext(buffer, Options::empty()).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info)))
                if info.as_ref() == "yaml" =>
            {
                eligible.push(range);
            }
            Event::Html(text) | Event::InlineHtml(text)
                if text.contains("prettier-ignore") =>
            {
                ignored.push(range.start);
            }
            _ => {}
        }
    }
    let mut retained_lines = if matches!(source, FormatterComments::Prettier) {
        ignored
            .iter()
            .map(|at| buffer[..*at].matches('\n').count() + 1)
            .collect()
    } else {
        Vec::new()
    };
    let sources = MarkdownSources {
        front_matter: true,
        fenced_blocks: true,
    };
    let mut output = buffer.to_owned();
    let mut converted = 0;
    let mut regions = extract_regions(buffer, sources);
    regions.sort_by_key(|region| std::cmp::Reverse(region.raw_span.start));
    for region in regions {
        let permit = permit
            && matches!(source, FormatterComments::Prettier)
            && ignored.is_empty()
            && (region.kind == RegionKind::FrontMatter
                || eligible
                    .iter()
                    .any(|fence| fence.contains(&region.raw_span.start)));
        let migration = formatter::migrate(
            TextVersion::new(region.content.as_str()).analysis(),
            source,
            permit,
        );
        converted += migration.converted;
        retained_lines.extend(
            migration
                .retained_lines
                .iter()
                .map(|line| line + region.line_offset),
        );
        let Some(replacement) = migration.output else {
            continue;
        };
        if let Some((prefix, newline)) = crate::fix::region_prefix(buffer, &region) {
            output.replace_range(
                region.raw_span,
                &crate::fix::reindent(&replacement, &prefix, newline),
            );
        } else {
            retained_lines.push(region.line_offset + 1);
        }
    }
    retained_lines.sort_unstable();
    retained_lines.dedup();
    Migration {
        converted,
        output: (output != buffer).then_some(output),
        retained_lines,
    }
}
