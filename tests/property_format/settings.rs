use proptest::prelude::*;

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
                (brace, gap, comment, blanks, sequence, mapping, sequences, dash, preview),
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
                    marker(start), marker(end), marker(comment),
                )
            },
        )
}
