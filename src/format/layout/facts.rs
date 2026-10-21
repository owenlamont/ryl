use crate::rules::line_length::fold::{FoldableScalar, Foldables};
use crate::rules::support::{comments_scan::CommentInfo, token_spacing};
use crate::rules::{block_scalar_chomping, comments_indentation};
use crate::text_version::{Analysis, lexical};

pub(crate) struct PlanFacts {
    pub(crate) headers: Vec<block_scalar_chomping::Header>,
    pub(crate) comment_lines: Vec<comments_indentation::LineInfo>,
    pub(crate) comments: Vec<CommentInfo>,
    pub(crate) sites: Vec<token_spacing::Site>,
    pub(crate) folds: Vec<FoldableScalar>,
}

pub(crate) struct Builder<'a> {
    headers: block_scalar_chomping::Headers<'a>,
    comment_lines: Vec<comments_indentation::LineInfo>,
    comments: Vec<CommentInfo>,
    pub(crate) sites: token_spacing::Sites<'a>,
    folds: Foldables,
    folding: bool,
    complete: bool,
}

impl<'a> Builder<'a> {
    pub(crate) fn new(input: Analysis<'a>, folding: bool) -> Self {
        Self {
            headers: block_scalar_chomping::Headers::new(input),
            comment_lines: comments_indentation::line_facts(input),
            comments: Vec::new(),
            sites: token_spacing::Sites::format(input),
            folds: Foldables::default(),
            folding,
            complete: false,
        }
    }

    pub(crate) fn push(&mut self, span: ryl_yaml_parser::Span, kind: &lexical::Kind) {
        self.complete |= matches!(kind, lexical::Kind::StreamEnd);
        self.headers.push(span, kind);
        comments_indentation::classify_token(&mut self.comment_lines, span, kind);
        self.sites.push(span, kind);
        if self.folding {
            self.folds.push(span, kind);
        }
        if let lexical::Kind::Comment { text, placement } = kind {
            self.comments.push(CommentInfo {
                span,
                text: text.to_string(),
                placement: *placement,
            });
        }
    }

    pub(crate) fn finish(self) -> Result<PlanFacts, super::Fallback> {
        self.complete
            .then(|| PlanFacts {
                headers: self.headers.finish(),
                comment_lines: self.comment_lines,
                comments: self.comments,
                sites: self.sites.finish(),
                folds: self.folds.finish(),
            })
            .ok_or(super::Fallback::Verification)
    }
}

pub(crate) fn prepare_input(
    input: Analysis<'_>,
    folding: bool,
) -> Result<(), super::Fallback> {
    let mut facts = Builder::new(input, folding);
    for (span, kind) in input.tokens().iter() {
        facts.push(span, kind);
    }
    facts.finish().map(|facts| input.seed_layout_facts(facts))
}
