//! Extract embeddable YAML regions from markdown so the YAML linter can run on
//! them and map diagnostics back to the original document.
//!
//! Two region kinds are recognised: leading YAML front matter (`---` … `---`/`...`)
//! and fenced code blocks tagged `yaml`/`yml`. Front matter is found with a small
//! line scan; fenced blocks are located with `pulldown-cmark`, whose offset
//! iterator yields each block's byte span and language tag. Only the `yaml`/`yml`
//! info string (including the `{.yaml}` attribute form) is matched; tags such as
//! `yaml+jinja` are intentionally ignored.

mod lint;

pub use lint::lint_markdown_str;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

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
    /// Number of newlines before the region's first content line. Region line 1
    /// maps to host line `line_offset + 1`.
    pub line_offset: usize,
    /// Common leading indent (in characters) stripped from the region. Added back
    /// to every diagnostic column. Always 0 for front matter.
    pub col_offset: usize,
    pub content: String,
}

#[must_use]
pub fn extract_regions(
    markdown: &str,
    sources: MarkdownSources,
) -> Vec<EmbeddedRegion> {
    let mut regions = Vec::new();
    if sources.front_matter
        && let Some(region) = front_matter_region(markdown)
    {
        regions.push(region);
    }
    if sources.fenced_blocks {
        collect_fenced_blocks(markdown, &mut regions);
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
                    regions.push(EmbeddedRegion {
                        kind: RegionKind::FencedBlock,
                        line_offset: markdown[..first_byte].matches('\n').count(),
                        col_offset: fence_indent(markdown, first_byte),
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

/// Characters of leading indentation pulldown stripped from the fenced block,
/// i.e. the column of its first content byte. The block's content always sits on
/// a line after its opening fence, so a preceding newline is guaranteed.
fn fence_indent(markdown: &str, content_start: usize) -> usize {
    let line_start = markdown[..content_start]
        .rfind('\n')
        .expect("fenced block content follows its opening fence line")
        + 1;
    markdown[line_start..content_start].chars().count()
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
