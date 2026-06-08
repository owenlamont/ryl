//! `braces`: control spacing inside flow mappings `{ }` and, optionally, forbid
//! flow mappings entirely. Mirrors yamllint's `braces`; built from the shared
//! `support::flow_collection::define_rule!` (its sequence twin is `brackets`).
//! Safe `--fix` normalises the inside-brace spacing.

crate::rules::support::flow_collection::define_rule!(
    "braces",
    '{',
    '}',
    "forbidden flow mapping",
    "too few spaces inside braces",
    "too many spaces inside braces",
    "too few spaces inside empty braces",
    "too many spaces inside empty braces",
);
