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
