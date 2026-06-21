//! `%YAML` version-directive handling shared by the rules and the lint engine: scan
//! directives, resolve the version in effect per document, and recognise the plain
//! scalars YAML 1.1 resolves to a non-string so quote removal stays value-preserving
//! under an explicit `%YAML 1.1`.

use std::sync::LazyLock;

use granit_parser::{Scanner, StrInput, Token, TokenType};
use regex::Regex;

use crate::rules::support::span_utils::{BytePos, marker_byte_offset};

pub type Version = (u32, u32);

#[derive(Debug, Clone, Copy)]
pub struct Directive {
    pub offset: BytePos,
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
    while let Ok(Some(Token(span, token_type))) = scanner.next_token() {
        if let TokenType::VersionDirective(major, minor) = token_type {
            directives.push(Directive {
                offset: marker_byte_offset(span.start),
                line: span.start.line(),
                column: span.start.col() + 1,
                version: (major, minor),
            });
        }
    }
    directives
}

/// The directive version in effect for each document, consumed in document order.
#[derive(Debug)]
pub struct DocumentVersions {
    directives: Vec<Directive>,
    index: usize,
}

impl DocumentVersions {
    #[must_use]
    pub fn parse(buffer: &str) -> Self {
        Self {
            directives: collect_directives(buffer),
            index: 0,
        }
    }

    pub const fn reset(&mut self) {
        self.index = 0;
    }

    pub fn next_document(&mut self, doc_start: BytePos) -> Option<Version> {
        let mut version = None;
        while self.index < self.directives.len()
            && self.directives[self.index].offset.get() < doc_start.get()
        {
            version = Some(self.directives[self.index].version);
            self.index += 1;
        }
        version
    }
}

/// Whether a scalar's quotes must be kept under the document's resolved version: only
/// for an explicit pre-1.2 directive, and only for a value YAML 1.1 reads as a
/// non-string (so dropping the quotes would change its value).
#[must_use]
pub fn keeps_quotes_under_yaml_1_1(version: Option<Version>, value: &str) -> bool {
    resolves_as_yaml_1_1(version) && resolves_to_nonstring_in_yaml_1_1(value)
}

/// A document resolves under YAML 1.1 when it explicitly declares a pre-1.2 version;
/// an absent directive and `%YAML 1.2`+ resolve under the 1.2 core schema.
const fn resolves_as_yaml_1_1(version: Option<Version>) -> bool {
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

fn resolves_to_nonstring_in_yaml_1_1(value: &str) -> bool {
    YAML_1_1_NONSTRING.is_match(value)
}

static YAML_1_1_NONSTRING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(YAML_1_1_NONSTRING_PATTERN)
        .expect("YAML 1.1 implicit-type regex is valid")
});

// Implicit `bool`/`int`/`float`/`timestamp`/`merge`/`value` resolvers ported verbatim
// from ruamel.yaml's version-1.1 set, so quote-stripping under `%YAML 1.1` matches a
// real 1.1 loader. `y`/`n` are 1.1 booleans here though `truthy` omits them (it mirrors
// yamllint); the wider set only makes quoted-strings keep more quotes, never strip
// unsafely. `null`/`yaml` resolvers are omitted: their values never reach this check
// (the empty/`~`/`null` forms are not 1.2 strings; `!`/`&`/`*` already force quoting).
const YAML_1_1_NONSTRING_PATTERN: &str = concat!(
    r"\A(?:",
    r"y|Y|yes|Yes|YES|n|N|no|No|NO|true|True|TRUE|false|False|FALSE|on|On|ON|off|Off|OFF",
    r"|[-+]?0b[0-1_]+|[-+]?0?[0-7_]+|[-+]?(?:0|[1-9][0-9_]*)|[-+]?0x[0-9a-fA-F_]+",
    r"|[-+]?[1-9][0-9_]*(?::[0-5]?[0-9])+",
    r"|[-+]?(?:[0-9][0-9_]*)\.[0-9_]*(?:[eE][-+]?[0-9]+)?",
    r"|[-+]?(?:[0-9][0-9_]*)(?:[eE][-+]?[0-9]+)|\.[0-9_]+(?:[eE][-+][0-9]+)?",
    r"|[-+]?[0-9][0-9_]*(?::[0-5]?[0-9])+\.[0-9_]*|[-+]?\.(?:inf|Inf|INF)|\.(?:nan|NaN|NAN)",
    r"|[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]",
    r"|[0-9][0-9][0-9][0-9]-[0-9][0-9]?-[0-9][0-9]?(?:[Tt]|[ \t]+)[0-9][0-9]?:[0-9][0-9]:[0-9][0-9](?:\.[0-9]*)?(?:[ \t]*(?:Z|[-+][0-9][0-9]?(?::[0-9][0-9])?))?",
    r"|<<|=",
    r")\z",
);
