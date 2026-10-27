//! `ryl format`'s `[format] sequence-style` and `mapping-style`: flow collections
//! rewritten in block style and leaf block ones in flow style, each entry's text copied
//! as written.

mod context;
mod input;
mod spelling;
mod target;
pub(crate) use context::RetainedContext;
pub(crate) use spelling::{Form, FragmentPart, Selected, Structure};

use ryl_yaml_parser::{Event, StructureStyle};

use crate::config_schema::CollectionStyleTarget;
use crate::directives::disables_file;
use crate::lint::{LintProblem, Severity};
use crate::rules::support::span_utils::BytePos;
use crate::rules::{braces, brackets, indentation};
use crate::text_version::Analysis;
use crate::text_version::model::NodeKind as Kind;

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

#[must_use]
pub fn findings(
    input: Analysis<'_>,
    cfg: Config,
    indent: Option<&indentation::Config>,
) -> Vec<Finding> {
    settled(input, cfg, indent).findings
}

#[derive(Default)]
pub(crate) struct Forms(Vec<(usize, Form)>);

impl Forms {
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub(crate) fn new(mut forms: Vec<(usize, Form)>) -> Self {
        forms.sort_unstable_by_key(|&(node, _)| node);
        Self(forms)
    }
    pub(crate) fn at(&self, node: usize) -> Option<Form> {
        self.0
            .binary_search_by_key(&node, |&(index, _)| index)
            .ok()
            .map(|index| self.0[index].1)
    }

    pub(crate) fn events<'a>(
        &'a self,
        input: Analysis<'a>,
    ) -> impl Iterator<Item = (Event<'a>, ryl_yaml_parser::Span)> + 'a {
        let mut node = 0;
        let mut selected = self.0.as_slice();
        input.event_iter().map(move |(event, span)| {
            let mut style = |retained| {
                selected
                    .first()
                    .copied()
                    .filter(|&(index, _)| index == node)
                    .map_or(retained, |(_, form)| {
                        selected = &selected[1..];
                        match form {
                            Form::Block => StructureStyle::Block,
                            Form::Flow => StructureStyle::Flow,
                        }
                    })
            };
            let event = match event {
                Event::SequenceStart(form, anchor, tag) => {
                    Event::SequenceStart(style(form), anchor, tag)
                }
                Event::MappingStart(form, anchor, tag) => {
                    Event::MappingStart(style(form), anchor, tag)
                }
                other => other,
            };
            node += usize::from(matches!(
                event,
                Event::Scalar(..)
                    | Event::Alias(..)
                    | Event::SequenceStart(..)
                    | Event::MappingStart(..)
            ));
            (event, span)
        })
    }
}

#[derive(Default)]
pub(crate) struct Plan {
    recipes: Vec<(Selected, usize)>,
    findings: Vec<Finding>,
    sites: Vec<input::Site>,
    nodes: Vec<crate::text_version::model::Node>,
    pending: Vec<usize>,
    comments: crate::rules::comments::Attachments,
}

impl Plan {
    pub(crate) fn comment_spacing(
        &mut self,
        cfg: Option<&crate::rules::comments::Config>,
        permission: crate::format::layout::Permission<'_>,
    ) -> bool {
        self.recipes.iter_mut().fold(false, |changed, (recipe, _)| {
            recipe.comment_spacing(cfg, permission) || changed
        })
    }

    pub(crate) fn flow_spacing(
        &mut self,
        mappings: &crate::rules::support::flow_collection::ClosingPolicy<'_>,
        sequences: &crate::rules::support::flow_collection::ClosingPolicy<'_>,
    ) -> bool {
        let mut changed = false;
        for (recipe, _) in &mut self.recipes {
            changed |= recipe.flow_spacing(|node, kind, spaces| {
                let at = BytePos::new(self.sites[node].start);
                match kind {
                    crate::text_version::model::Container::Mapping => mappings,
                    crate::text_version::model::Container::Sequence => sequences,
                }
                .select((at, at, spaces))
                .unwrap_or(spaces)
            });
        }
        changed
    }

    pub(crate) fn forms(&self) -> Forms {
        Forms::new(
            self.selections()
                .flat_map(Selected::fragments)
                .filter_map(|part| match part {
                    FragmentPart::Structure(Structure::Start {
                        node, form, ..
                    }) => Some((node, form)),
                    _ => None,
                })
                .collect(),
        )
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.recipes.is_empty()
    }
    pub(crate) fn rules(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.findings
            .iter()
            .filter(|finding| finding.refused.is_none())
            .map(|finding| finding.rule)
    }

    pub(crate) fn has_candidates(&self) -> bool {
        !self.recipes.is_empty() || !self.pending.is_empty()
    }

    fn covers(&self, index: usize) -> bool {
        let site = &self.sites[index];
        let covering = self
            .recipes
            .partition_point(|(recipe, _)| recipe.range().end.get() < site.start);
        self.recipes.get(covering).is_some_and(|(recipe, _)| {
            let range = recipe.range();
            range.start.get() <= site.start && site.end <= range.end.get()
        })
    }

    pub(crate) fn reconsider(
        &mut self,
        source: Analysis<'_>,
        cfg: Config,
        edits: &[crate::format::layout::PlanEdit],
        styles: &crate::rules::quoted_strings::Styles,
        (layout, targets): (&indentation::Layout<'_>, &indentation::Candidates),
        trailing: &[crate::format::layout::PlanEdit],
    ) -> bool {
        let pending: Vec<_> = self
            .pending
            .iter()
            .copied()
            .filter(|&index| !self.covers(index))
            .collect();
        if pending.is_empty() {
            return false;
        }
        let presentation = crate::format::layout::projection::Projection::selected(
            source, edits, trailing,
        );
        // Indentation targets apply their gaps after this source projection.
        let coordinates = crate::format::layout::projection::Projection::new(
            source,
            if layout.is_converted() { &[] } else { edits },
        );
        let mut sites = self.sites.clone();
        for index in self.coordinate_sites(&pending) {
            let site = &mut sites[index];
            let position =
                layout.collection_position(site.start, targets, &coordinates);
            site.line = position.line + 1;
            site.column = position.column;
        }
        let positions = pending
            .iter()
            .map(|&index| {
                let before = source.text()[..self.sites[index].start]
                    .trim_end_matches(is_space)
                    .len();
                (
                    before,
                    layout
                        .collection_position(before, targets, &coordinates)
                        .column,
                )
            })
            .collect();
        let geometry = indentation::CollectionGeometry::selected(source, positions);
        let directives = source.directives();
        let enabled = |target, rule| {
            (!directives.disables_any(rule) && !disables_file(source.text()))
                .then_some(target)
        };
        let planner = target::Planner::new(
            &self.nodes,
            input::Initial::new(source, &sites, &self.comments).selected(
                edits,
                styles,
                &presentation,
            ),
            &geometry,
            enabled(cfg.sequences, brackets::ID),
            enabled(cfg.mappings, braces::ID),
            usize::from(cfg.indent),
            usize::from(cfg.width),
        );
        let mut more = Vec::new();
        for index in pending {
            let site = &self.sites[index];
            let flowed = planner.flowed(index);
            let to_flow = flowed.is_some();
            let document = source
                .parsed()
                .document_starts
                .partition_point(|&line| line < site.line)
                .saturating_sub(1);
            let refused = targets.refused.get(document).is_some_and(Option::is_some);
            let recipe = flowed.or_else(|| {
                (!refused && planner.outermost_candidate(index))
                    .then(|| planner.outermost(index).ok())
                    .flatten()
            });
            if let Some(recipe) = recipe {
                let finding = self.findings.len();
                self.findings.push(Finding {
                    line: site.line,
                    column: site.column + 1,
                    rule: match self.nodes[index].kind() {
                        Kind::Mapping => braces::ID,
                        _ => brackets::ID,
                    },
                    refused: None,
                    to_flow,
                });
                more.push((recipe, finding));
            }
        }
        let changed = !more.is_empty();
        self.recipes.extend(more);
        self.recipes.sort_by_key(|(recipe, _)| recipe.range().start);
        changed
    }

    fn coordinate_sites(&self, pending: &[usize]) -> std::collections::BTreeSet<usize> {
        let mut needed = std::collections::BTreeSet::new();
        for &index in pending {
            needed.insert(index);
            if let Some(parent) = self.nodes[index].parent() {
                needed.insert(parent);
                if self.nodes[parent].kind() == Kind::Mapping {
                    needed.insert(
                        self.nodes[parent].children()[self.nodes[index].position() - 1],
                    );
                }
            }
        }
        needed
    }

    pub(crate) fn close_implicit(
        &mut self,
        source: Analysis<'_>,
        cfg: Config,
        edits: &[crate::format::layout::PlanEdit],
        styles: &crate::rules::quoted_strings::Styles,
        layout: &indentation::Layout<'_>,
        targets: &indentation::Candidates,
    ) -> bool {
        if cfg.mappings != CollectionStyleTarget::Flow
            || source.directives().disables_any(braces::ID)
        {
            return false;
        }
        let geometry = indentation::CollectionGeometry::new(source, None);
        let projection =
            crate::format::layout::projection::Projection::new(source, edits);
        let planner = target::Planner::new(
            &self.nodes,
            input::Initial::new(source, &self.sites, &self.comments).selected(
                edits,
                styles,
                &projection,
            ),
            &geometry,
            Some(cfg.sequences),
            Some(cfg.mappings),
            usize::from(cfg.indent),
            usize::from(cfg.width),
        );
        let mut replacements = std::collections::HashMap::new();
        for part in self.selections().flat_map(Selected::fragments) {
            let FragmentPart::Structure(Structure::Start {
                node,
                kind: crate::text_version::model::Container::Mapping,
                form: Form::Block,
            }) = part
            else {
                continue;
            };
            let site = &self.sites[node];
            if site.flow
                && !site.braced
                && layout
                    .collection_form(node)
                    .is_some_and(|form| matches!(form, Form::Block))
                && let Some(body) = planner.implicit_body(
                    node,
                    layout
                        .collection_position(site.start, targets, &projection)
                        .column,
                )
            {
                replacements.insert(node, body);
            }
        }
        if replacements.is_empty() {
            return false;
        }
        for &node in replacements.keys() {
            let site = &self.sites[node];
            self.findings.push(Finding {
                line: site.line,
                column: site.column + 1,
                rule: braces::ID,
                refused: None,
                to_flow: true,
            });
        }
        for (recipe, _) in &mut self.recipes {
            recipe.replace_bodies(&mut replacements);
        }
        true
    }

    pub(crate) fn selections(&self) -> impl Iterator<Item = &Selected> {
        self.recipes.iter().map(|(recipe, _)| recipe)
    }

    pub(crate) fn span(&self, node: usize) -> std::ops::Range<BytePos> {
        let site = &self.sites[node];
        BytePos::new(site.start)..BytePos::new(site.end)
    }

    pub(crate) fn scalar_style(
        &self,
        node: usize,
    ) -> Option<ryl_yaml_parser::ScalarStyle> {
        self.sites[node].scalar
    }
}

pub(crate) fn settled(
    input: Analysis<'_>,
    cfg: Config,
    indent: Option<&indentation::Config>,
) -> Plan {
    settled_with_view(input, cfg, indent).0
}

pub(crate) fn settled_with_view<'a>(
    input: Analysis<'a>,
    cfg: Config,
    indent: Option<&indentation::Config>,
) -> (Plan, Option<crate::format::layout::frame::FrameRows<'a>>) {
    let mut plan = select(input, cfg, indent);
    let view = skip_reindent_dependent(
        input,
        &mut plan,
        indent,
        cfg.require_consistent_indent,
    );
    (plan, view)
}

pub(crate) fn select(
    input: Analysis<'_>,
    cfg: Config,
    indent: Option<&indentation::Config>,
) -> Plan {
    let buffer = input.text();
    if cfg.sequences == CollectionStyleTarget::Preserve
        && cfg.mappings == CollectionStyleTarget::Preserve
    {
        return Plan::default();
    }
    if !input.event_iter().any(|(event, _)| match event {
        Event::SequenceStart(style, ..) => changes_style(cfg.sequences, style),
        Event::MappingStart(style, ..) => changes_style(cfg.mappings, style),
        _ => false,
    }) {
        return Plan::default();
    }
    if crate::fix::non_ascii_marker(buffer) {
        return Plan::default();
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
    let mut recipes = Vec::new();
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
            recipes.push((edit, findings.len() - 1));
        }
        if !planner.outermost_candidate(index) {
            continue;
        }
        let outcome = planner.outermost(index);
        findings.push(finding(outcome.as_ref().err().copied(), false));
        if let Ok(edit) = outcome {
            recipes.push((edit, findings.len() - 1));
        }
    }
    let pending = (0..nodes.len())
        .filter(|&index| planner.pending(index))
        .collect();
    recipes.sort_by_key(|(recipe, _)| recipe.range().start);
    Plan {
        recipes,
        findings,
        sites,
        nodes,
        pending,
        comments,
    }
}

fn changes_style(target: CollectionStyleTarget, style: StructureStyle) -> bool {
    matches!(
        (target, style),
        (CollectionStyleTarget::Block, StructureStyle::Flow)
            | (CollectionStyleTarget::Flow, StructureStyle::Block)
    )
}

fn skip_reindent_dependent<'a>(
    input: Analysis<'a>,
    plan: &mut Plan,
    indent: Option<&indentation::Config>,
    require_consistent_indent: bool,
) -> Option<crate::format::layout::frame::FrameRows<'a>> {
    let indent = indent.filter(|_| {
        plan.recipes
            .iter()
            .any(|(_, finding)| !plan.findings[*finding].to_flow)
    })?;
    let (feasibility, view) = indentation::CollectionFeasibility::observe(
        input,
        indent,
        require_consistent_indent,
        plan,
    );
    let source_starts = document_start_lines(input);
    let source_lines = input.byte_line_starts();
    let selected = plan.recipes.len();
    plan.recipes.retain(|(recipe, finding)| {
        let line =
            source_lines.partition_point(|start| *start <= recipe.range().start.get());
        let document = source_starts.partition_point(|start| *start <= line) - 1;
        if !plan.findings[*finding].to_flow && feasibility.refuses(document) {
            plan.findings[*finding].refused = Some(target::REINDENT_REFUSAL);
            false
        } else {
            true
        }
    });
    (plan.recipes.len() == selected).then_some(view)
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
