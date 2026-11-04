use std::path::Path;

use ryl::cli_support::{LintFile, sanitize_control};
use ryl::config::{SourceKind, YamlLintConfig};
use ryl::directives::{legacy_directive_lines, starts_with_jinja_header};
use ryl::{MarkdownSources, extract_regions};

pub(crate) fn notice(
    content: &str,
    path: &Path,
    cfg: &YamlLintConfig,
    kind: SourceKind,
    no_warnings: bool,
) -> Option<String> {
    if no_warnings || !content.contains("yamllint ") {
        return None;
    }
    let first = match kind {
        SourceKind::Yaml => first_alias(content),
        SourceKind::Markdown => {
            if content.lines().any(|line| line.contains('\r')) {
                return None;
            }
            extract_regions(
                content,
                MarkdownSources {
                    front_matter: cfg.markdown_front_matter(),
                    fenced_blocks: cfg.markdown_fenced_blocks(),
                },
            )
            .into_iter()
            .filter_map(|region| {
                first_alias(&region.content).map(|line| line + region.line_offset)
            })
            .min()
        }
    };
    first.map(|line| format!(
        "warning: {}:{line}: yamllint directive aliases are deprecated; use ryl directives or --migrate-directives\n",
        sanitize_control(&path.display().to_string())
    ))
}

fn first_alias(content: &str) -> Option<usize> {
    if starts_with_jinja_header(content) {
        return None;
    }
    legacy_directive_lines(content).into_iter().min()
}

pub(crate) fn notice_bytes(
    bytes: &[u8],
    path: &Path,
    cfg: &YamlLintConfig,
    kind: SourceKind,
    no_warnings: bool,
) -> Option<String> {
    if no_warnings {
        return None;
    }
    let encoding = std::env::var("YAMLLINT_FILE_ENCODING").ok();
    ryl::decoder::decode_bytes_with_override(bytes, encoding.as_deref())
        .ok()
        .and_then(|content| notice(&content, path, cfg, kind, false))
}

fn notice_file(
    path: &Path,
    cfg: &YamlLintConfig,
    kind: SourceKind,
    no_warnings: bool,
) -> Option<String> {
    if no_warnings
        || !std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file())
    {
        return None;
    }
    std::fs::read(path)
        .ok()
        .and_then(|bytes| notice_bytes(&bytes, path, cfg, kind, false))
}

pub(crate) fn emit(notice: Option<String>) {
    if let Some(notice) = notice {
        eprint!("{notice}");
    }
}

pub(crate) fn rewrite_notice(
    (path, _, cfg, kind): &LintFile,
    bytes: Option<&[u8]>,
    no_warnings: bool,
) -> Option<String> {
    bytes.map_or_else(
        || notice_file(path, cfg, *kind, no_warnings),
        |bytes| notice_bytes(bytes, path, cfg, *kind, no_warnings),
    )
}
