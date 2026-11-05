use proptest::prelude::*;

fn expand() -> impl Strategy<Value = &'static str> {
    prop::sample::select(vec!["preserve", "auto", "always", "never"])
}

pub fn arb_format_config() -> impl Strategy<Value = String> {
    (
        (
            prop::option::of(1u8..=u8::MAX),
            prop_oneof![1u16..=100, 1u16..=u16::MAX],
            prop::sample::select(vec!["single", "double", "preserve"]),
            prop::sample::select(vec!["lf", "cr-lf", "native", "auto"]),
            prop::sample::select(vec!["add", "preserve", "remove"]),
            any::<bool>(),
            prop::sample::select(vec!["fold-long-lines = false", "fold-long-lines = true", "prose-wrap = 'preserve'", "prose-wrap = 'always'", "prose-wrap = 'never'"]),
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
        (expand(), expand(), prop::sample::select(vec!["preserve", "anchor-first", "tag-first"]), any::<bool>()),
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
                (sequence_expand, mapping_expand, property_order, keep_quotes),
            )| {
                let indent = indent.map_or_else(String::new, |width| {
                    format!("indent-width = {width}\n")
                });
                let marker = |add| if add { "add" } else { "preserve" };
                format!(
                    "{indent}line-length = {length}\n[format]\n\
                     quote-style = '{quote}'\nkeep-quotes = {keep_quotes}\nline-ending = '{ending}'\n\
                     document-start = '{}'\ndocument-end = '{}'\n\
                     {fold}\nbrace-spacing = {brace}\n\
                     preview = {preview}\ncomment-spacing = {gap}\n\
                     comment-starting-space = '{}'\nmax-blank-lines = {blanks}\n\
                     sequence-style = '{sequence}'\nmapping-style = '{mapping}'\n\
                     indent-sequences = {sequences}\ndash-on-own-line = {dash}\n\
                     sequence-expand = '{sequence_expand}'\nmapping-expand = '{mapping_expand}'\n\
                     property-order = '{property_order}'\n",
                    start,
                    marker(end),
                    marker(comment),
                )
            },
        )
}
