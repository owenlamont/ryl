use granit_parser::{ScalarStyle, Scanner, StrInput, TokenType};

// granit invents a break for unterminated indentation, unlike YAML 1.2.2's
// l-keep-empty and resolving loaders.
pub fn without_indentation(content: &str) -> &str {
    let Some((prefix, tail)) = content.rsplit_once(['\n', '\r']) else {
        return content;
    };
    if tail.is_empty() || !tail.bytes().all(|byte| byte == b' ') {
        return content;
    }
    let mut removable = false;
    let chars: Vec<_> = content.chars().collect();
    let mut previous_end = 0;
    for token in Scanner::new(StrInput::new(content)) {
        let Ok(token) = token else { return content };
        let (span, kind) = token.into_parts();
        if matches!(kind, TokenType::Comment(_)) {
            continue;
        }
        if matches!(
            kind,
            TokenType::Scalar(ScalarStyle::Literal | ScalarStyle::Folded, _)
        ) && span.end.index() == chars.len()
        {
            let mut comment = false;
            let marker = previous_end
                + chars[previous_end..]
                    .iter()
                    .position(|&ch| {
                        comment = (comment || ch == '#') && !matches!(ch, '\n' | '\r');
                        !comment && matches!(ch, '|' | '>')
                    })
                    .expect("block scalar header");
            let indent = span.indent.unwrap_or_else(|| {
                if span.start.index() == marker {
                    usize::MAX
                } else {
                    span.start.col()
                }
            });
            removable = tail.len() <= indent;
        }
        previous_end = span.end.index();
    }
    if removable {
        &content[..prefix.len() + 1]
    } else {
        content
    }
}
