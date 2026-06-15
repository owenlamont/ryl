//! Builds the code actions `ryl server` offers for a document: the whole-file
//! `source.fixAll.ryl`, a per-rule `source.fixAll.ryl.<rule>` for each safe-fixable
//! rule with a diagnostic, and `quickfix` actions that insert a `# ryl disable-line`
//! (per rule/line) or a first-line `# ryl disable-file`. All edits are pure functions of
//! the document text and the request context; the actual fixing reuses
//! [`analysis::fix_all_edit`] / [`analysis::fix_rule_edit`], and the disable inserts
//! mirror the directive grammar in [`crate::directives`].

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;

use lsp_types::{
    CodeAction, CodeActionContext, CodeActionKind, CodeActionOrCommand,
    CodeActionResponse, Diagnostic, DocumentChanges, NumberOrString, OneOf,
    OptionalVersionedTextDocumentIdentifier, Position, Range, TextDocumentEdit,
    TextEdit, Uri, WorkspaceEdit,
};

use crate::config::{SourceKind, YamlLintConfig};
use crate::fix::SAFE_FIX_RULE_IDS;
use crate::lsp::analysis::{fix_all_edit, fix_rule_edit};
use crate::lsp::encoding::PositionEncoding;
use crate::rules::ALL_RULE_IDS;
use crate::rules::support::line_syntax::{
    buffer_newline, line_contents, protected_scalar_lines,
};

/// The whole-file safe-fix code-action kind, also usable for `editor.codeActionsOnSave`.
const FIX_ALL_KIND: &str = "source.fixAll.ryl";

/// Everything `build` needs about the document under action, so each call site stays
/// short. Borrowed from the server's open-document state and resolved config.
pub struct Input<'a> {
    pub uri: &'a Uri,
    pub text: &'a str,
    pub version: i32,
    pub path: &'a Path,
    pub cfg: &'a YamlLintConfig,
    pub base_dir: &'a Path,
    pub kind: SourceKind,
    pub enc: PositionEncoding,
    pub supports_document_changes: bool,
}

/// Build every code action that applies to `input`, filtered by the request's
/// `context.only`. Returns `None` when nothing applies (no fix and no disableable
/// diagnostic), so the server replies with a null result rather than an empty list.
#[must_use]
pub fn build(input: &Input, context: &CodeActionContext) -> Option<CodeActionResponse> {
    let mut actions = Vec::new();

    if admits(context.only.as_deref(), FIX_ALL_KIND)
        && let Some(edit) = fix_all_edit(
            input.text,
            input.path,
            input.cfg,
            input.base_dir,
            input.kind,
            input.enc,
        )
    {
        actions.push(entry(
            "Fix all ryl problems".to_string(),
            FIX_ALL_KIND,
            input,
            edit,
        ));
    }

    for rule in fixable_rules_present(&context.diagnostics) {
        let kind = format!("{FIX_ALL_KIND}.{rule}");
        if admits(context.only.as_deref(), &kind)
            && let Some(edit) = fix_rule_edit(
                input.text,
                input.path,
                input.cfg,
                input.base_dir,
                input.kind,
                input.enc,
                rule,
            )
        {
            actions.push(entry(
                format!("Fix all {rule} problems"),
                &kind,
                input,
                edit,
            ));
        }
    }

    // Disable actions insert `#` directives by document line. That is only sound for a
    // plain YAML document: in Markdown the diagnostic's line is a host-file line whose
    // embedded YAML carries a prefix (fence indent, `> `, …), so a raw insert would land
    // in the wrong place or as fenced content. Markdown documents get fix-all only.
    if matches!(input.kind, SourceKind::Yaml)
        && admits(context.only.as_deref(), CodeActionKind::QUICKFIX.as_str())
    {
        // A line spanned by a *multi-line* scalar (block `|`/`>` or a quoted/plain scalar
        // continued across lines) is scalar content, not comment context: a disable-line
        // insert there would land inside the still-open scalar and corrupt the value
        // instead of acting as a directive. Skip those lines (the set is 1-based granit
        // line numbers). When the document does NOT parse (e.g. an undefined alias) we
        // cannot tell which lines are scalar content, so no disable-line is offered at all;
        // disable-file (a line-0 prepend) is always safe.
        let scalar_lines = protected_scalar_lines(input.text, |_, span| {
            span.start.line() != span.end.line()
        });
        if let Some(scalar_lines) = scalar_lines {
            for (rule, line) in disable_targets(&context.diagnostics) {
                if let Some(action) =
                    disable_line_action(input, &rule, line, &scalar_lines)
                {
                    actions.push(action);
                }
            }
        }
        // A whole-file disable is only useful when ryl itself flagged the file.
        if has_ryl_diagnostic(&context.diagnostics) {
            actions.push(disable_file_action(input));
        }
    }

    (!actions.is_empty()).then_some(actions)
}

/// Whether the client's `context.only` filter admits an action of `kind`: no filter
/// means yes, otherwise a requested kind must equal `kind` or be an ancestor of it (so a
/// `source` / `source.fixAll` request matches `source.fixAll.ryl`, the way
/// `editor.codeActionsOnSave` issues them).
fn admits(only: Option<&[CodeActionKind]>, kind: &str) -> bool {
    match only {
        None => true,
        Some(only) => only.iter().any(|requested| {
            let requested = requested.as_str();
            kind == requested || kind.starts_with(&format!("{requested}."))
        }),
    }
}

/// The ryl rule id a diagnostic carries in its `code`, if any. Only ryl's own
/// diagnostics are considered (a document may also carry diagnostics from a coexisting
/// server such as yaml-language-server), and only a known rule id is accepted — so a
/// foreign or crafted code (e.g. one containing a newline) can never form a directive.
fn diagnostic_rule(diagnostic: &Diagnostic) -> Option<&'static str> {
    if diagnostic.source.as_deref() != Some("ryl") {
        return None;
    }
    let code = match &diagnostic.code {
        Some(NumberOrString::String(code)) => code.as_str(),
        _ => return None,
    };
    ALL_RULE_IDS.into_iter().find(|id| *id == code)
}

/// Whether the document has at least one ryl-sourced diagnostic (so the whole-file
/// disable is offered for ryl's findings, not a coexisting server's).
fn has_ryl_diagnostic(diagnostics: &[Diagnostic]) -> bool {
    diagnostics
        .iter()
        .any(|diagnostic| diagnostic.source.as_deref() == Some("ryl"))
}

/// Safe-fixable rules with at least one diagnostic, in fix-application order (stable and
/// independent of diagnostic order) so the offered actions are deterministic.
fn fixable_rules_present(diagnostics: &[Diagnostic]) -> Vec<&'static str> {
    let present: HashSet<&str> =
        diagnostics.iter().filter_map(diagnostic_rule).collect();
    SAFE_FIX_RULE_IDS
        .iter()
        .copied()
        .filter(|rule| present.contains(rule))
        .collect()
}

/// Distinct `(rule, 0-based line)` pairs to offer a `disable-line` for, in diagnostic
/// order.
fn disable_targets(diagnostics: &[Diagnostic]) -> Vec<(String, u32)> {
    let mut seen = HashSet::new();
    let mut targets = Vec::new();
    for diagnostic in diagnostics {
        if let Some(rule) = diagnostic_rule(diagnostic) {
            let line = diagnostic.range.start.line;
            if seen.insert((rule.to_string(), line)) {
                targets.push((rule.to_string(), line));
            }
        }
    }
    targets
}

/// A `disable-line` quickfix: insert `# ryl disable-line rule:<rule>` on its own line
/// above `line`, indented like `line` so the comment does not itself trip
/// `comments-indentation`. A standalone `disable-line` applies to the *next* line (see
/// [`crate::directives`]), which is the diagnostic's line after the insertion shifts it
/// down. `None` when `line` is past the document or inside a multi-line scalar (where a
/// `#` insert would be scalar content, not a directive).
fn disable_line_action(
    input: &Input,
    rule: &str,
    line: u32,
    scalar_lines: &HashSet<usize>,
) -> Option<CodeActionOrCommand> {
    let index = usize::try_from(line).unwrap_or(usize::MAX);
    // `protected_scalar_lines` is 1-based; the 0-based LSP `line` maps to `index + 1`.
    if scalar_lines.contains(&index.saturating_add(1)) {
        return None;
    }
    let lines = line_contents(input.text);
    let target = lines.get(index)?;
    let indent: String = target
        .chars()
        .take_while(|ch| *ch == ' ' || *ch == '\t')
        .collect();
    let newline = buffer_newline(input.text);
    let insert = format!("{indent}# ryl disable-line rule:{rule}{newline}");
    let edit = TextEdit::new(at_line_start(line), insert);
    Some(entry(
        format!("Disable {rule} for this line"),
        CodeActionKind::QUICKFIX.as_str(),
        input,
        edit,
    ))
}

/// A `disable-file` quickfix: prepend a first-line `# ryl disable-file`, which skips the
/// whole file (all rules) for linting and `--fix` (see [`crate::directives`]).
fn disable_file_action(input: &Input) -> CodeActionOrCommand {
    let insert = format!("# ryl disable-file{}", buffer_newline(input.text));
    let edit = TextEdit::new(at_line_start(0), insert);
    entry(
        "Disable ryl for this file".to_string(),
        CodeActionKind::QUICKFIX.as_str(),
        input,
        edit,
    )
}

/// A zero-width range at the start of `line`, for an insert-a-line edit.
fn at_line_start(line: u32) -> Range {
    Range {
        start: Position::new(line, 0),
        end: Position::new(line, 0),
    }
}

/// Wrap a single whole-or-partial `TextEdit` as a titled code action of `kind`.
fn entry(
    title: String,
    kind: &str,
    input: &Input,
    edit: TextEdit,
) -> CodeActionOrCommand {
    CodeActionOrCommand::CodeAction(CodeAction {
        title,
        kind: Some(CodeActionKind::from(kind.to_string())),
        edit: Some(workspace_edit(
            input.uri.clone(),
            input.version,
            vec![edit],
            input.supports_document_changes,
        )),
        ..Default::default()
    })
}

/// Build a single-file workspace edit from one or more `edits`. A client advertising
/// `documentChanges` support gets a versioned `TextDocumentEdit` (so it can discard the
/// edit if the buffer moved past `version` before the edit is applied); otherwise it gets
/// the unversioned `changes` map. Shared by the code actions here and by rename. `Uri`
/// has benign interior mutability (a fluent-uri parse cache) that never affects its
/// hash/equality, hence the lint allow.
#[allow(clippy::mutable_key_type)]
pub(crate) fn workspace_edit(
    uri: Uri,
    version: i32,
    edits: Vec<TextEdit>,
    supports_document_changes: bool,
) -> WorkspaceEdit {
    if supports_document_changes {
        WorkspaceEdit {
            document_changes: Some(DocumentChanges::Edits(vec![TextDocumentEdit {
                text_document: OptionalVersionedTextDocumentIdentifier {
                    uri,
                    version: Some(version),
                },
                edits: edits.into_iter().map(OneOf::Left).collect(),
            }])),
            ..Default::default()
        }
    } else {
        WorkspaceEdit {
            changes: Some(HashMap::from([(uri, edits)])),
            ..Default::default()
        }
    }
}
