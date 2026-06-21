//! Extract embeddable YAML regions from markdown so the YAML linter can run on them and
//! map diagnostics back to the original document.
//!
//! Two region kinds are recognised: leading YAML front matter (`---` ... `---`/`...`) and
//! fenced code blocks tagged `yaml`/`yml` (the `{.yaml}` attribute form too). A tag such
//! as `yaml+jinja` is intentionally ignored.

mod lint;

pub use lint::{lint_markdown_str, markdown_parse_skips};

use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use crate::lint::{LintProblem, Severity};

/// Whether the markdown contains a bare `\r` (a CR not part of CRLF) anywhere.
/// `pulldown-cmark` does not honour `CommonMark`'s bare-`\r` line ending, so it can't find
/// fences in a `\r` host, and the `\n`-based host remap can't place a region-content `\r`.
/// The lint/fix/diff paths skip such a file.
#[must_use]
pub(crate) fn markdown_has_unsupported_cr(markdown: &str) -> bool {
    let bytes = markdown.as_bytes();
    bytes
        .iter()
        .enumerate()
        .any(|(idx, &byte)| byte == b'\r' && bytes.get(idx + 1) != Some(&b'\n'))
}

/// The diagnostic for a bare-`\r` markdown file (see [`markdown_has_unsupported_cr`]),
/// reported as a lint error and as the `--fix`/`--diff` skip reason.
#[must_use]
pub(crate) fn unsupported_cr_skip() -> LintProblem {
    LintProblem {
        line: 1,
        column: 1,
        level: Severity::Error,
        message: "a bare carriage return prevents extracting embedded YAML; convert \
                  the markdown file to LF or CRLF line endings"
            .to_string(),
        rule: None,
    }
}

/// Which embedded YAML sources to extract from a markdown document.
#[derive(Debug, Clone, Copy)]
pub struct MarkdownSources {
    pub front_matter: bool,
    pub fenced_blocks: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionKind {
    FrontMatter,
    FencedBlock,
}

/// A slice of YAML lifted out of a markdown document, with the offsets needed to
/// translate region-local diagnostic positions back to the host file.
#[derive(Debug, Clone)]
pub struct EmbeddedRegion {
    pub kind: RegionKind,
    /// Newlines before the region's first content line: region line 1 maps to host line
    /// `line_offset + 1`.
    pub line_offset: usize,
    /// Common leading indent (chars) stripped from the region, added back to every
    /// diagnostic column. Always 0 for front matter.
    pub col_offset: usize,
    pub content: String,
    /// Byte span of the region's raw content in the source markdown. `--fix` write-back
    /// splices over it, and the per-line column remap recovers each line's stripped indent.
    pub raw_span: Range<usize>,
}

#[must_use]
pub fn extract_regions(
    markdown: &str,
    sources: MarkdownSources,
) -> Vec<EmbeddedRegion> {
    // Locate the front matter even when it is not linted, so a fence nested in its scalar
    // is still filtered out below (otherwise a `front-matter = false` source would leak
    // back in via that nested fence).
    let front_matter = front_matter_region(markdown);
    let front_end = front_matter.as_ref().map(|region| region.raw_span.end);

    let mut regions = Vec::new();
    if sources.front_matter
        && let Some(region) = front_matter
    {
        regions.push(region);
    }
    if sources.fenced_blocks {
        collect_fenced_blocks(markdown, &mut regions);
    }
    // A fence opening inside the front-matter scalar is malformed (its content is part of
    // that scalar's value), yet CommonMark still parses it. A real body fence's content
    // always begins strictly after the terminator line, so keep a fence only when its
    // content starts past the front matter end; otherwise its content would be
    // double-linted or, under --fix, spliced over the front matter's span.
    if let Some(front_end) = front_end {
        regions.retain(|region| {
            region.kind == RegionKind::FrontMatter || region.raw_span.start > front_end
        });
    }
    regions
}

fn front_matter_region(markdown: &str) -> Option<EmbeddedRegion> {
    let first_newline = markdown.find('\n')?;
    if !is_front_matter_open(&markdown[..first_newline]) {
        return None;
    }
    let content_start = first_newline + 1;
    let mut cursor = content_start;
    while cursor < markdown.len() {
        let line_end = markdown[cursor..]
            .find('\n')
            .map_or(markdown.len(), |offset| cursor + offset);
        if is_front_matter_close(&markdown[cursor..line_end]) {
            return Some(EmbeddedRegion {
                kind: RegionKind::FrontMatter,
                line_offset: markdown[..content_start].matches('\n').count(),
                col_offset: 0,
                content: markdown[content_start..cursor].to_string(),
                raw_span: content_start..cursor,
            });
        }
        cursor = line_end + 1;
    }
    None
}

fn is_front_matter_open(line: &str) -> bool {
    line.trim_end() == "---"
}

fn is_front_matter_close(line: &str) -> bool {
    matches!(line.trim_end(), "---" | "...")
}

fn collect_fenced_blocks(markdown: &str, regions: &mut Vec<EmbeddedRegion>) {
    // Byte offsets of every newline, ascending, so a block's line offset is a binary
    // search rather than an O(offset) rescan that would be quadratic over many blocks.
    let newlines: Vec<usize> =
        markdown.match_indices('\n').map(|(idx, _)| idx).collect();
    let mut active: Option<FenceAccumulator> = None;
    for (event, range) in Parser::new_ext(markdown, Options::empty()).into_offset_iter()
    {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => {
                active = is_yaml_info(&info).then(FenceAccumulator::default);
            }
            Event::Text(text) => {
                if let Some(accumulator) = active.as_mut() {
                    accumulator.first_byte.get_or_insert(range.start);
                    accumulator.content.push_str(&text);
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(accumulator) = active.take()
                    && let Some(first_byte) = accumulator.first_byte
                {
                    let line_start = line_start_of(markdown, first_byte);
                    regions.push(EmbeddedRegion {
                        kind: RegionKind::FencedBlock,
                        line_offset: newlines.partition_point(|&pos| pos < line_start),
                        col_offset: markdown[line_start..first_byte].chars().count(),
                        raw_span: line_start
                            ..fenced_content_end(
                                markdown,
                                line_start,
                                &accumulator.content,
                            ),
                        content: accumulator.content,
                    });
                }
            }
            _ => {}
        }
    }
}

#[derive(Default)]
struct FenceAccumulator {
    content: String,
    first_byte: Option<usize>,
}

/// Byte offset of the start of the line containing `content_start`. Block content always
/// sits on a line after its opening fence, so a preceding newline is guaranteed.
fn line_start_of(markdown: &str, content_start: usize) -> usize {
    markdown[..content_start]
        .rfind('\n')
        .expect("fenced block content follows its opening fence line")
        + 1
}

/// Byte offset where the fenced block's raw content ends: the start of the closing fence
/// line, or end of input for a block left open at EOF.
fn fenced_content_end(markdown: &str, start: usize, content: &str) -> usize {
    // A missing trailing newline means a block left open at EOF, whose content runs to
    // the end of input.
    if !content.ends_with('\n') {
        return markdown.len();
    }
    let mut pos = start;
    for _ in 0..content.matches('\n').count() {
        let offset = markdown[pos..]
            .find('\n')
            .expect("each content line precedes the closing fence");
        pos += offset + 1;
    }
    pos
}

fn is_yaml_info(info: &str) -> bool {
    let token = info
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_start_matches('{')
        .trim_end_matches('}')
        .trim_start_matches('.');
    token.eq_ignore_ascii_case("yaml") || token.eq_ignore_ascii_case("yml")
}
