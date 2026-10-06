//! `%YAML` version-directive handling shared by the rules and the lint engine: scan
//! directives, resolve the version in effect per document, and recognise the plain
//! scalars YAML 1.1 resolves to a non-string so quote removal stays value-preserving
//! for a 1.1 reader.

use std::sync::LazyLock;

use granit_parser::{Scanner, StrInput, TokenType, YamlVersion};
use regex::Regex;

pub type Version = (u32, u32);

/// The `%YAML` version granit reports on `Event::DocumentStart`, which already resolves
/// the directive to its document, so rules take it from the event rather than rescanning.
#[must_use]
pub const fn event_version(version: YamlVersion) -> Version {
    (version.major, version.minor)
}

#[derive(Debug, Clone, Copy)]
pub struct Directive {
    pub line: usize,
    pub column: usize,
    pub version: Version,
}

fn collect_directives(buffer: &str) -> Vec<Directive> {
    if !buffer.contains("%YAML") {
        return Vec::new();
    }
    let mut directives = Vec::new();
    let mut scanner = Scanner::new(StrInput::new(buffer));
    // The scanner reports a `%YAML` only where it is a real directive (not block-scalar
    // or plain-scalar text that happens to start with `%YAML`); it stops at the first
    // lexical error, after which no further directive can be reached anyway.
    while let Some(Ok(token)) = scanner.next() {
        let (span, token_type) = token.into_parts();
        if let TokenType::VersionDirective(major, minor) = token_type {
            directives.push(Directive {
                line: span.start.line(),
                column: span.start.col() + 1,
                version: (major, minor),
            });
        }
    }
    directives
}

/// A document resolves under YAML 1.1 when it explicitly declares a pre-1.2 version;
/// an absent directive and `%YAML 1.2`+ resolve under the 1.2 core schema.
#[must_use]
pub const fn resolves_as_yaml_1_1(version: Option<Version>) -> bool {
    matches!(version, Some((1, minor)) if minor <= 1)
}

#[must_use]
pub fn first_unsupported_major(buffer: &str) -> Option<Directive> {
    collect_directives(buffer)
        .into_iter()
        .find(|directive| directive.version.0 != 1)
}

#[must_use]
pub fn first_higher_minor(buffer: &str) -> Option<Directive> {
    collect_directives(buffer)
        .into_iter()
        .find(|directive| directive.version.0 == 1 && directive.version.1 > 2)
}

/// Whether YAML 1.1 reads the plain scalar `value` as a non-string, so dropping its
/// quotes would change its value for a 1.1 reader.
#[must_use]
pub fn resolves_to_nonstring_in_yaml_1_1(value: &str) -> bool {
    YAML_1_1_NONSTRING.is_match(value)
}

static YAML_1_1_NONSTRING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(YAML_1_1_NONSTRING_PATTERN)
        .expect("YAML 1.1 implicit-type regex is valid")
});

// PyYAML's 1.1 implicit-type resolvers plus the `0o` octal yamllint's quoted-strings
// adds, so the set matches yamllint (`y`/`n`, `1e5` and `-.5` are strings).
const YAML_1_1_NONSTRING_PATTERN: &str = concat!(
    r"\A(?:",
    r"yes|Yes|YES|no|No|NO|true|True|TRUE|false|False|FALSE|on|On|ON|off|Off|OFF",
    r"|[-+]?[0-9][0-9_]*\.[0-9_]*(?:[eE][-+][0-9]+)?|\.[0-9][0-9_]*(?:[eE][-+][0-9]+)?",
    r"|[-+]?[0-9][0-9_]*(?::[0-5]?[0-9])+\.[0-9_]*|[-+]?\.(?:inf|Inf|INF)|\.(?:nan|NaN|NAN)",
    r"|[-+]?0b[0-1_]+|[-+]?0o?[0-7_]+|[-+]?(?:0|[1-9][0-9_]*)|[-+]?0x[0-9a-fA-F_]+",
    r"|[-+]?[1-9][0-9_]*(?::[0-5]?[0-9])+",
    r"|<<|~|null|Null|NULL|",
    r"|[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]",
    r"|[0-9][0-9][0-9][0-9]-[0-9][0-9]?-[0-9][0-9]?(?:[Tt]|[ \t]+)[0-9][0-9]?:[0-9][0-9]:[0-9][0-9](?:\.[0-9]*)?(?:[ \t]*(?:Z|[-+][0-9][0-9]?(?::[0-9][0-9])?))?",
    r"|=",
    r")\z",
);
