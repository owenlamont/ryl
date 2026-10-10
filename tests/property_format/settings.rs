use proptest::prelude::*;
use ryl::config::YamlLintConfig;
use ryl::config_schema::{LineEndingTarget, MarkerTarget, QuoteStyleTarget};

pub fn agreeing_lint(config: &str) -> String {
    let cfg = YamlLintConfig::from_toml_str(config).expect(config);
    let table = cfg.format().targets();
    let enabled = |target| if target { "enable" } else { "disable" };
    let quote = match table.quote_style {
        QuoteStyleTarget::Single => "single",
        QuoteStyleTarget::Double => "double",
        QuoteStyleTarget::Preserve => "any",
    };
    let ending = match table.line_ending {
        LineEndingTarget::Lf => "unix",
        LineEndingTarget::CrLf => "dos",
        LineEndingTarget::Native => "platform",
        LineEndingTarget::Auto => "unix",
    };
    let padding = u8::from(table.brace_spacing);
    let mut lint = format!(
        "{config}\n[lint.rules]\n\
         comments-indentation = 'enable'\ncommas = 'enable'\n\
         new-line-at-end-of-file = 'enable'\ntrailing-spaces = 'enable'\n\
         document-start = '{}'\ndocument-end = '{}'\n\
         [lint.rules.braces]\nmin-spaces-inside = {padding}\n\
         max-spaces-inside = {padding}\nmin-spaces-inside-empty = 0\n\
         max-spaces-inside-empty = 0\n\
         [lint.rules.brackets]\nmin-spaces-inside = 0\nmax-spaces-inside = 0\n\
         [lint.rules.colons]\nmax-spaces-before = 0\nmax-spaces-after = 1\n\
         [lint.rules.hyphens]\nmax-spaces-after = 1\ndash-on-own-line = {}\n\
         [lint.rules.indentation]\nindent-sequences = {}\n\
         [lint.rules.comments]\nmin-spaces-from-content = {}\n\
         max-spaces-from-content = {}\nrequire-starting-space = {}\n\
         [lint.rules.empty-lines]\nmax = {}\nmax-start = 0\nmax-end = 0\n\
         [lint.rules.line-length]\nmax = {}\n\
         [lint.rules.quoted-strings]\nquote-type = '{quote}'\n\
         required = 'only-when-needed'\nallow-double-quotes-for-escaping = true\n\
         allow-quoted-quotes = true\n\
         [lint.rules.new-lines]\ntype = '{ending}'\n",
        enabled(table.document_start == MarkerTarget::Add),
        enabled(table.document_end == MarkerTarget::Add),
        table.dash_on_own_line,
        table.indent_sequences,
        table.comment_spacing,
        table.comment_spacing,
        table.comment_starting_space == MarkerTarget::Add,
        table.max_blank_lines,
        ryl::format::line_length(&cfg),
    );
    if table.quote_style == QuoteStyleTarget::Preserve {
        let start = lint.find("[lint.rules.quoted-strings]").unwrap();
        let end = lint.find("[lint.rules.new-lines]").unwrap();
        lint.replace_range(start..end, "");
        lint = lint.replace("[lint.rules]", "[lint.rules]\nquoted-strings = 'disable'");
    }
    if table.line_ending == LineEndingTarget::Auto {
        lint.truncate(lint.find("[lint.rules.new-lines]").unwrap());
        lint = lint.replace("[lint.rules]", "[lint.rules]\nnew-lines = 'disable'");
    }
    lint
}

pub fn arb_format_config() -> impl Strategy<Value = String> {
    (
        (
            prop::option::of(1u8..=u8::MAX),
            1u16..=u16::MAX,
            prop::sample::select(vec!["single", "double", "preserve"]),
            prop::sample::select(vec!["lf", "cr-lf", "native", "auto"]),
            any::<bool>(),
            any::<bool>(),
            any::<bool>(),
        ),
        (
            any::<bool>(),
            1u8..=u8::MAX,
            any::<bool>(),
            any::<u8>(),
            prop::sample::select(vec!["preserve", "block", "flow"]),
            prop::sample::select(vec!["preserve", "block", "flow"]),
            any::<bool>(),
            any::<bool>(),
            any::<bool>(),
        ),
    )
        .prop_map(
            |(
                (indent, length, quote, ending, start, end, fold),
                (
                    brace,
                    gap,
                    comment,
                    blanks,
                    sequence,
                    mapping,
                    sequences,
                    dash,
                    preview,
                ),
            )| {
                let indent = indent.map_or_else(String::new, |width| {
                    format!("indent-width = {width}\n")
                });
                let marker = |add| if add { "add" } else { "preserve" };
                format!(
                    "{indent}line-length = {length}\n[format]\n\
                     quote-style = '{quote}'\nline-ending = '{ending}'\n\
                     document-start = '{}'\ndocument-end = '{}'\n\
                     fold-long-lines = {fold}\nbrace-spacing = {brace}\n\
                     preview = {preview}\ncomment-spacing = {gap}\n\
                     comment-starting-space = '{}'\nmax-blank-lines = {blanks}\n\
                     sequence-style = '{sequence}'\nmapping-style = '{mapping}'\n\
                     indent-sequences = {sequences}\ndash-on-own-line = {dash}\n",
                    marker(start),
                    marker(end),
                    marker(comment),
                )
            },
        )
}
