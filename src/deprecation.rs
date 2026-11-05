use std::sync::atomic::{AtomicUsize, Ordering};

use ryl::cli_support::LintFile;
use ryl::config::{SourceKind, YamlLintConfig};
use ryl::directives::{legacy_directive_lines, starts_with_jinja_header};
use ryl::{MarkdownSources, extract_regions};

static FILES: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn record(
    content: &str,
    cfg: &YamlLintConfig,
    kind: SourceKind,
    no_warnings: bool,
) {
    if no_warnings || !content.contains("yamllint ") {
        return;
    }
    let found = match kind {
        SourceKind::Yaml => has_alias(content),
        SourceKind::Markdown => {
            if content.lines().any(|line| line.contains('\r')) {
                return;
            }
            extract_regions(
                content,
                MarkdownSources {
                    front_matter: cfg.markdown_front_matter(),
                    fenced_blocks: cfg.markdown_fenced_blocks(),
                },
            )
            .into_iter()
            .any(|region| has_alias(&region.content))
        }
    };
    if found {
        FILES.fetch_add(1, Ordering::Relaxed);
    }
}

fn has_alias(content: &str) -> bool {
    !starts_with_jinja_header(content) && !legacy_directive_lines(content).is_empty()
}

pub(crate) fn record_bytes(
    bytes: &[u8],
    cfg: &YamlLintConfig,
    kind: SourceKind,
    no_warnings: bool,
) {
    if no_warnings {
        return;
    }
    let encoding = std::env::var("YAMLLINT_FILE_ENCODING").ok();
    if let Ok(content) =
        ryl::decoder::decode_bytes_with_override(bytes, encoding.as_deref())
    {
        record(&content, cfg, kind, false);
    }
}

pub(crate) fn emit_summary() {
    let count = FILES.load(Ordering::Relaxed);
    if count > 0 {
        eprintln!(
            "warning: yamllint directive aliases are deprecated in {count} {}; use `ryl --migrate-directives` to convert",
            if count == 1 { "file" } else { "files" }
        );
    }
}

pub(crate) fn file_notice(
    no_warnings: bool,
) -> impl Fn(&str, &LintFile) -> Option<String> + Sync {
    move |content, (_, _, cfg, kind)| {
        record(content, cfg, *kind, no_warnings);
        None
    }
}
