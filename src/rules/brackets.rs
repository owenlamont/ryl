//! `brackets`: control spacing inside flow sequences `[ ]` and, optionally, forbid
//! flow sequences entirely. Mirrors yamllint's `brackets`; built from the shared
//! `support::flow_collection::define_rule!` (its mapping twin is `braces`).
//! Safe `--fix` normalises the inside-bracket spacing.

crate::rules::support::flow_collection::define_rule!(
    "brackets",
    '[',
    ']',
    "forbidden flow sequence",
    "too few spaces inside brackets",
    "too many spaces inside brackets",
    "too few spaces inside empty brackets",
    "too many spaces inside empty brackets",
);
