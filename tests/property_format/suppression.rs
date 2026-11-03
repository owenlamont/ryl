use proptest::prelude::*;

#[derive(Debug, Clone)]
pub struct Document {
    pub text: String,
    pub protected: String,
    pub independent_conversion: bool,
}

pub fn arb_document() -> impl Strategy<Value = Document> {
    (
        "[a-z]{1,20}",
        0usize..7,
        any::<bool>(),
        any::<bool>(),
        2usize..7,
        0usize..7,
        any::<bool>(),
        0usize..3,
        prop::sample::select(vec!["", " &previous", " !!str", " &previous !!str", " !!str &previous"]),
    )
        .prop_map(|(word, shape, region, crlf, indent, nested, separated, extra_shape, properties)| {
            let body = match shape {
                0 => format!("held:    ['{word}', 1,2]\n"),
                1 => format!(
                    "held:\n{0:1$}child:    '{word}'\n{0:1$}nested:    [1,2]\n",
                    "", indent
                ),
                2 => format!("held: >-\n  café {word}\n  more words\n"),
                3 => format!("? held\n:    '{word}'\n"),
                4 => format!("?    \n    held\n:    '{word}'\n"),
                5 => format!("held: &a\n  !!map\n    child:    '{word}'\n"),
                _ => format!("held: [\n  ? foo\n   bar : '{word}'\n  ]\n"),
            };
            let (open, close) = if region {
                ("# ryl format-disable\n", "# ryl format-enable\n")
            } else {
                ("# ryl format-ignore\n", "")
            };
            let open = format!("{open}{}", if separated { "\n" } else { "" });
            let (content, body) = if nested == 6 {
                let (before, after) = match shape { 0 | 2 | 4 | 6 => ("&anchor", "!!map"), _ => ("!!map", "&anchor") };
                let body = format!("    {after}   \n        held:    '{word}'\n");
                let indent = |text: &str| text.lines().map(|line| format!("    {line}\n")).collect::<String>();
                let content = format!("outer: {before}   \n{}{body}{}", indent(&open), indent(close));
                let protected = if region { body } else { content.clone() };
                (content, protected)
            } else if nested >= 4 {
                let (head, prefix) = if nested == 4 {
                    (format!("outer:\n    -\n    -\n    -\n        -{properties}\n"), "        ")
                } else {
                    (format!("outer:\n- first\n-    -{properties}\n"), "     ")
                };
                let body = format!("{prefix}-    '{word}'\n");
                let indent = |text: &str| {
                    text.lines()
                        .map(|line| format!("{prefix}{line}\n"))
                        .collect::<String>()
                };
                (
                    format!("{head}{}{body}{}{prefix}- after\n", indent(&open), indent(close)),
                    body,
                )
            } else if nested > 0 {
                let (head, column) = match nested {
                    1 => (format!("outer:\n    sibling:{properties}\n"), 4),
                    2 => (format!("outer:\n    - sibling:{properties}\n"), 6),
                    _ => (format!("outer:\n    -\n        sibling:{properties}\n"), 8),
                };
                let indent = |text: &str| {
                    text.lines()
                        .map(|line| format!("{0:1$}{line}\n", "", column))
                        .collect::<String>()
                };
                let body = indent(&body);
                let words = format!("long {word} words long words long words");
                let prose = match shape {
                    0 => words,
                    1 => format!("'{words}'"),
                    2 => format!("\"{words}\""),
                    _ => format!(">-\n{0:1$}{words}", "", column + 2),
                };
                (
                    format!(
                        "{head}{}{body}{}{prefix}sibling_after: {prose}\n",
                        indent(&open),
                        indent(close), prefix = " ".repeat(column)
                    ),
                    body,
                )
            } else {
                (format!("previous:{properties}\n{open}{body}{close}"), body)
            };
            let extra = match extra_shape {
                0 => "extra:\n  - k:\n    tail: 1\n",
                1 => "extra:\n  -    k:\n       tail: 1\n",
                _ => "extra:\n  -\n      k:\n      tail: 1\n",
            };
            let conversion = if nested > 0 { "another:\n  - [5,6]\n" } else { "" };
            let text =
                format!("before:    0\nconvert: [3,4]\n{content}{extra}{conversion}after:    1\n");
            Document {
                text: text.replace('\n', if crlf { "\r\n" } else { "\n" }),
                protected: body.replace('\n', if crlf { "\r\n" } else { "\n" }),
                independent_conversion: nested == 0,
            }
        })
}
