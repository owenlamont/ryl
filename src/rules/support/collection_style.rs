//! `ryl format`'s `[format] sequence-style` and `mapping-style`: flow collections
//! rewritten in block style and leaf block ones in flow style, each entry's text copied
//! as written.

mod context;
mod input;
mod spelling;
mod target;
pub(crate) use context::RetainedContext;

use ryl_yaml_parser::{Event, StructureStyle};

use crate::config_schema::CollectionStyleTarget;
use crate::directives::disables_file;
use crate::lint::{LintProblem, Severity};
use crate::rules::support::line_syntax::buffer_newline;
use crate::rules::support::span_utils::{BytePos, apply_replacements};
use crate::rules::{braces, brackets, comments_indentation, indentation};
use crate::text_version::model::NodeKind as Kind;
use crate::text_version::{Analysis, TextVersion};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    pub sequences: CollectionStyleTarget,
    pub mappings: CollectionStyleTarget,
    pub indent: u8,
    pub width: u16,
    pub require_consistent_indent: bool,
}

/// A collection the pass rewrites, or leaves alone for `refused`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub line: usize,
    pub column: usize,
    pub rule: &'static str,
    pub refused: Option<&'static str>,
    pub to_flow: bool,
}

impl Finding {
    #[must_use]
    pub fn into_problem(self) -> LintProblem {
        let kind = if self.rule == braces::ID {
            "mapping"
        } else {
            "sequence"
        };
        let (from, to) = if self.to_flow {
            ("block", "flow")
        } else {
            ("flow", "block")
        };
        LintProblem {
            line: self.line,
            column: self.column,
            level: Severity::Error,
            message: self.refused.map_or_else(
                || format!("{from} {kind} would become {to}"),
                |reason| format!("cannot convert to {to} safely: {reason}"),
            ),
            rule: Some(self.rule),
        }
    }
}

pub(crate) fn prepare(
    input: Analysis<'_>,
    cfg: Config,
    indent: Option<&indentation::Config>,
    comments: Option<comments_indentation::Config>,
) -> Result<
    Option<(TextVersion<'static>, Vec<&'static str>)>,
    crate::format::layout::Fallback,
> {
    let (mut edits, findings) = plan(input, cfg, indent);
    if edits.is_empty() {
        return Ok(None);
    }
    let mut rules: Vec<_> = findings
        .into_iter()
        .filter(|finding| finding.refused.is_none())
        .map(|finding| finding.rule)
        .collect();
    if let Some(cfg) = comments
        && input
            .event_iter()
            .any(|(event, _)| matches!(event, Event::Comment(..)))
    {
        let comments = comments_indentation::edits(input, cfg);
        if !comments.is_empty() {
            rules.push(comments_indentation::ID);
            edits.extend(comments);
        }
    }
    let mut output = TextVersion::new(apply_replacements(input.text(), edits));
    if !input.verify_collection_events(output.analysis()) {
        return Err(crate::format::layout::Fallback::Verification);
    }
    output.mark_strict_valid();
    Ok(Some((output, rules)))
}

#[must_use]
pub fn findings(
    input: Analysis<'_>,
    cfg: Config,
    indent: Option<&indentation::Config>,
) -> Vec<Finding> {
    plan(input, cfg, indent).1
}

type Edit = (BytePos, BytePos, String);

fn plan(
    input: Analysis<'_>,
    cfg: Config,
    indent: Option<&indentation::Config>,
) -> (Vec<Edit>, Vec<Finding>) {
    let buffer = input.text();
    if cfg.sequences == CollectionStyleTarget::Preserve
        && cfg.mappings == CollectionStyleTarget::Preserve
    {
        return (Vec::new(), Vec::new());
    }
    if !input.event_iter().any(|(event, _)| match event {
        Event::SequenceStart(style, ..) => changes_style(cfg.sequences, style),
        Event::MappingStart(style, ..) => changes_style(cfg.mappings, style),
        _ => false,
    }) {
        return (Vec::new(), Vec::new());
    }
    let directives = input.directives();
    let style = |target, rule| {
        (!directives.disables_any(rule) && !disables_file(buffer)).then_some(target)
    };
    let input::Tree {
        nodes,
        sites,
        comments,
    } = input::observe(input);
    let geometry = indentation::CollectionGeometry::new(input, indent.copied());
    let planner = target::Planner::new(
        &nodes,
        input::Initial::new(input, &sites, &comments),
        &geometry,
        style(cfg.sequences, brackets::ID),
        style(cfg.mappings, braces::ID),
        usize::from(cfg.indent),
        usize::from(cfg.width),
    );
    let newline = buffer_newline(buffer);
    let mut edits = Vec::new();
    let mut findings = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        let site = &sites[index];
        let finding = |refused, to_flow| Finding {
            line: site.line,
            column: site.column + 1,
            rule: if node.kind() == Kind::Mapping {
                braces::ID
            } else {
                brackets::ID
            },
            refused,
            to_flow,
        };
        if let Some(edit) = planner.flowed(index) {
            findings.push(finding(None, true));
            edits.push((edit.materialize(buffer, newline), findings.len() - 1));
        }
        if !planner.outermost_candidate(index) {
            continue;
        }
        let outcome = planner.outermost(index);
        findings.push(finding(outcome.as_ref().err().copied(), false));
        if let Ok(edit) = outcome {
            edits.push((edit.materialize(buffer, newline), findings.len() - 1));
        }
    }
    let edits = skip_reindent_dependent(
        input,
        edits,
        &mut findings,
        indent,
        cfg.require_consistent_indent,
    );
    (edits, findings)
}

fn changes_style(target: CollectionStyleTarget, style: StructureStyle) -> bool {
    matches!(
        (target, style),
        (CollectionStyleTarget::Block, StructureStyle::Flow)
            | (CollectionStyleTarget::Flow, StructureStyle::Block)
    )
}

fn skip_reindent_dependent(
    input: Analysis<'_>,
    edits: Vec<(Edit, usize)>,
    findings: &mut [Finding],
    indent: Option<&indentation::Config>,
    require_consistent_indent: bool,
) -> Vec<Edit> {
    let Some(indent) =
        indent.filter(|_| edits.iter().any(|(_, finding)| !findings[*finding].to_flow))
    else {
        return edits.into_iter().map(|(edit, _)| edit).collect();
    };
    let candidate = apply_replacements(
        input.text(),
        edits.iter().map(|(edit, _)| edit.clone()).collect(),
    );
    let candidate = TextVersion::new(candidate);
    let feasibility = indentation::CollectionFeasibility::observe(
        candidate.analysis(),
        indent,
        require_consistent_indent,
    );
    let source_starts = document_start_lines(input);
    let source_lines = input.byte_line_starts();
    edits
        .into_iter()
        .filter_map(|(edit, finding)| {
            let line = source_lines.partition_point(|start| *start <= edit.0.get());
            let document = source_starts.partition_point(|start| *start <= line) - 1;
            if let Some(reason) = feasibility
                .document(document)
                .filter(|_| !findings[finding].to_flow)
            {
                findings[finding].refused = Some(target::refusal(reason));
                None
            } else {
                Some(edit)
            }
        })
        .collect()
}

fn document_start_lines(input: Analysis<'_>) -> Vec<usize> {
    let mut starts: Vec<usize> = input
        .parsed()
        .document_starts
        .iter()
        .map(|start| start + 1)
        .collect();
    *starts
        .first_mut()
        .expect("a restyled collection has a document") = 1;
    starts
}

fn is_space(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\n' | '\r')
}
