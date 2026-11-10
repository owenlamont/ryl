use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use ryl_yaml_parser::Span;

use crate::config::{SourceKind, YamlLintConfig};
use crate::text_version::{Analysis, TextVersion, lexical::Kind};

const SPACE: &str = r"\x09-\x0d\x20\x{a0}\x{1680}\x{2000}-\x{200a}\x{2028}\x{2029}\x{202f}\x{205f}\x{3000}\x{feff}";
static PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^[{SPACE}]+(?:yaml-language-server[{SPACE}]*:|\$schema:)"
    ))
    .expect("modeline prefix regex")
});
static VALUE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"\$schema(?:=|:[{SPACE}&&[^\r\n]]*)([^{SPACE}]+)"))
        .expect("modeline value regex")
});

#[derive(Clone, Debug)]
pub(crate) struct Declaration {
    pub(crate) value: Option<String>,
    pub(crate) line: usize,
    pub(crate) column: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct Request {
    #[cfg(feature = "lsp")]
    pub(crate) owner: PathBuf,
    #[cfg(feature = "lsp")]
    pub(crate) revision: Option<Revision>,
    pub(crate) path: Option<PathBuf>,
    pub(crate) declarations: Vec<Declaration>,
}

#[cfg(feature = "lsp")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Revision {
    pub(crate) version: i64,
    text: blake3::Hash,
}

#[cfg(feature = "lsp")]
impl Revision {
    pub(crate) fn new(version: i64, text: &str) -> Self {
        Self {
            version,
            text: blake3::hash(
                text.strip_prefix('\u{feff}').unwrap_or(text).as_bytes(),
            ),
        }
    }
}

impl Request {
    #[cfg(feature = "lsp")]
    pub(crate) fn declaration_for(&self, value: &str) -> Option<&Declaration> {
        self.declarations
            .iter()
            .find(|declaration| declaration.value.as_deref() == Some(value))
    }

    #[cfg(feature = "lsp")]
    pub(crate) fn error(&self, message: String) -> super::PreparationError {
        let declaration = self.declarations.iter().find(|declaration| {
            declaration
                .value
                .as_ref()
                .is_some_and(|value| !value.eq_ignore_ascii_case("none"))
        });
        super::PreparationError {
            line: declaration.map_or(1, |d| d.line),
            column: declaration.map_or(1, |d| d.column),
            message,
        }
    }
}

pub(crate) fn headers(input: Analysis<'_>) -> Vec<Option<Declaration>> {
    #[cfg(debug_assertions)]
    super::record_directive_scan();
    let mut result = vec![None];
    let mut header = true;
    let mut explicit = false;
    let mut end_line = None;
    for (span, kind) in input.tokens().iter(input.text()) {
        match kind {
            Kind::StreamStart | Kind::StreamEnd | Kind::Other => {}
            Kind::DocumentEnd => {
                header = true;
                explicit = false;
                end_line = Some(span.start.line());
                result.push(None);
            }
            Kind::DocumentStart => {
                if !header || explicit {
                    result.push(None);
                }
                header = true;
                explicit = true;
                end_line = None;
            }
            Kind::Comment { text, .. } => {
                if header
                    && end_line != Some(span.start.line())
                    && result.last().expect("header slot").is_none()
                    && PREFIX.is_match(text)
                {
                    *result.last_mut().expect("header slot") =
                        Some(declaration(text, span));
                }
            }
            _ => header = false,
        }
    }
    result.truncate(
        input
            .event_iter()
            .filter(|(event, _)| {
                matches!(event, ryl_yaml_parser::Event::DocumentStart(..))
            })
            .count(),
    );
    result
}

fn declaration(text: &str, span: Span) -> Declaration {
    Declaration {
        value: VALUE.captures(text).map(|value| value[1].to_owned()),
        line: span.start.line(),
        column: span.start.col() + 1,
    }
}

pub(crate) fn near_modeline(text: &str) -> bool {
    let text = text.trim_start_matches('#').trim_start();
    text.starts_with("yaml-language-server") || text.starts_with("$schema:")
}

pub(crate) fn modeline_lines(input: Analysis<'_>) -> Vec<usize> {
    input
        .tokens()
        .iter(input.text())
        .filter_map(|(span, kind)| match kind {
            Kind::Comment { text, .. } if PREFIX.is_match(text) => {
                Some(span.start.line() - 1)
            }
            _ => None,
        })
        .collect()
}

pub(crate) fn request(
    text: &str,
    path: Option<&Path>,
    cfg: &YamlLintConfig,
    kind: SourceKind,
) -> Request {
    let declarations = match kind {
        SourceKind::Yaml => collect(text),
        SourceKind::Markdown
            if crate::markdown_embed::markdown_has_unsupported_cr(text) =>
        {
            Vec::new()
        }
        SourceKind::Markdown => crate::markdown_embed::extract_regions(
            text,
            crate::markdown_embed::MarkdownSources {
                front_matter: cfg.markdown_front_matter(),
                fenced_blocks: cfg.markdown_fenced_blocks(),
            },
        )
        .into_iter()
        .flat_map(|region| {
            let offsets = crate::markdown_embed::stripped_indents(text, &region);
            collect(&region.content)
                .into_iter()
                .map(move |mut declaration| {
                    declaration.column += offsets
                        .get(declaration.line - 1)
                        .copied()
                        .unwrap_or(region.col_offset);
                    declaration.line += region.line_offset;
                    declaration
                })
        })
        .collect(),
    };
    Request {
        #[cfg(feature = "lsp")]
        owner: path.unwrap_or_else(|| Path::new("")).to_owned(),
        #[cfg(feature = "lsp")]
        revision: None,
        path: path.map(Path::to_owned),
        declarations,
    }
}

fn collect(text: &str) -> Vec<Declaration> {
    if crate::directives::disables_file(text) {
        return Vec::new();
    }
    headers(TextVersion::new(text).analysis())
        .into_iter()
        .flatten()
        .collect()
}
