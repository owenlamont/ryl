//! Wraps generated YAML `Document`s into a markdown host: optional front matter
//! plus prose-separated fenced `yaml`/`yml` blocks at varied indent. Reuses the
//! safe-fix YAML AST/strategy so the embedded YAML matches the flat `--fix` suite.

use proptest::prelude::*;

use super::ast::{Document, NewlineStyle};
use super::strategy::arb_document;

#[derive(Debug)]
pub struct FencedSection {
    pub prose: String,
    pub doc: Document,
    pub indent: usize,
    pub fence: char,
    pub info: String,
}

#[derive(Debug)]
pub struct MarkdownDoc {
    pub front: Option<Document>,
    pub sections: Vec<FencedSection>,
    pub newline: NewlineStyle,
}

fn newline_str(newline: NewlineStyle) -> &'static str {
    match newline {
        NewlineStyle::Lf => "\n",
        NewlineStyle::Crlf => "\r\n",
    }
}

/// The YAML body as logical lines (LF-split, no trailing terminator), so the host
/// renderer can re-terminate with its own newline and indent each non-empty line.
fn yaml_lines(doc: &Document) -> Vec<String> {
    let mut normalized = doc.clone();
    normalized.newline = NewlineStyle::Lf;
    normalized.has_final_newline = false;
    normalized.render().split('\n').map(String::from).collect()
}

impl MarkdownDoc {
    pub fn render(&self) -> String {
        let nl = newline_str(self.newline);
        let mut out = String::new();
        if let Some(front) = &self.front {
            out.push_str("---");
            out.push_str(nl);
            push_body(&mut out, &yaml_lines(front), "", nl);
            out.push_str("---");
            out.push_str(nl);
            out.push_str(nl);
        }
        for section in &self.sections {
            out.push_str(&section.prose);
            out.push_str(nl);
            out.push_str(nl);
            let indent = " ".repeat(section.indent);
            let fence: String = std::iter::repeat_n(section.fence, 3).collect();
            out.push_str(&indent);
            out.push_str(&fence);
            out.push_str(&section.info);
            out.push_str(nl);
            push_body(&mut out, &yaml_lines(&section.doc), &indent, nl);
            out.push_str(&indent);
            out.push_str(&fence);
            out.push_str(nl);
            out.push_str(nl);
        }
        out
    }
}

/// Append YAML body lines, indenting non-empty lines (blank lines stay empty, the
/// way the CommonMark parser dedents them) and terminating with `nl`.
fn push_body(out: &mut String, lines: &[String], indent: &str, nl: &str) {
    for line in lines {
        if !line.is_empty() {
            out.push_str(indent);
            out.push_str(line);
        }
        out.push_str(nl);
    }
}

fn arb_prose() -> impl Strategy<Value = String> {
    prop::collection::vec("[a-z]{1,6}", 1..=4).prop_map(|words| words.join(" "))
}

fn arb_section() -> impl Strategy<Value = FencedSection> {
    (
        arb_prose(),
        arb_document(),
        0usize..=3,
        prop_oneof![Just('`'), Just('~')],
        prop_oneof![Just("yaml".to_string()), Just("yml".to_string())],
    )
        .prop_map(|(prose, doc, indent, fence, info)| FencedSection {
            prose,
            doc,
            indent,
            fence,
            info,
        })
}

pub fn arb_markdown_doc() -> impl Strategy<Value = MarkdownDoc> {
    (
        prop::option::of(arb_document()),
        prop::collection::vec(arb_section(), 0..=3),
        prop_oneof![Just(NewlineStyle::Lf), Just(NewlineStyle::Crlf)],
    )
        .prop_map(|(front, sections, newline)| MarkdownDoc {
            front,
            sections,
            newline,
        })
}
