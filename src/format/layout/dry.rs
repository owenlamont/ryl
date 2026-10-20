use crate::rules::line_length::fold::{FoldableScalar, Foldables};
use crate::rules::support::{comments_scan::CommentInfo, token_spacing};
use crate::rules::{block_scalar_chomping, comments_indentation, indentation};
use crate::text_version::{Analysis, lexical};

pub(crate) struct Facts {
    pub(crate) headers: Vec<block_scalar_chomping::Header>,
    pub(crate) comment_lines: Vec<comments_indentation::LineInfo>,
    pub(crate) comments: Vec<CommentInfo>,
    pub(crate) sites: Vec<token_spacing::Site>,
    pub(crate) folds: Vec<FoldableScalar>,
    pub(crate) layout: indentation::DryLayout,
}

pub(crate) fn prepare(
    input: Analysis<'_>,
    config: &indentation::Config,
    folding: bool,
) {
    if input.text().contains('\u{feff}') {
        return;
    }
    let mut headers = block_scalar_chomping::Headers::new(input);
    let mut comment_lines = comments_indentation::line_facts(input);
    let mut comments = Vec::new();
    let mut sites = token_spacing::Sites::new(input);
    let mut folds = Foldables::default();
    super::stats::count(super::stats::Counter::Scan);
    super::stats::count(super::stats::Counter::DryScan);
    let tokens = lexical::stream(input.text()).inspect(|(span, kind)| {
        headers.push(*span, kind);
        comments_indentation::classify_token(&mut comment_lines, *span, kind);
        sites.push(*span, kind);
        if folding {
            folds.push(*span, kind);
        }
        if let lexical::Kind::Comment { text, placement } = kind {
            comments.push(CommentInfo {
                span: *span,
                text: text.to_string(),
                placement: *placement,
            });
        }
    });
    let layout = indentation::stream_layout(input, config, tokens, folding);
    input.seed_layout_facts(Facts {
        headers: headers.finish(),
        comment_lines,
        comments,
        sites: sites.finish(),
        folds: folds.finish(),
        layout,
    });
}
