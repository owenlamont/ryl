pub mod anchors;
pub mod braces;
pub mod brackets;
pub mod colons;
pub mod commas;
pub mod comments;
pub mod comments_indentation;
pub mod document_end;
pub mod document_start;
pub mod empty_lines;
pub mod empty_values;
pub mod float_values;
pub mod hyphens;
pub mod indentation;
pub mod key_duplicates;
pub mod key_ordering;
pub mod line_length;
pub mod new_line_at_end_of_file;
pub mod new_lines;
pub mod octal_values;
pub mod quoted_strings;
pub(crate) mod support;
pub mod trailing_spaces;
pub mod truthy;

/// Every rule id, used by the directive engine to expand a bare `disable`/`enable`
/// (no `rule:` token) to "all rules". Extend this when adding a rule.
pub const ALL_RULE_IDS: [&str; 23] = [
    anchors::ID,
    braces::ID,
    brackets::ID,
    colons::ID,
    commas::ID,
    comments::ID,
    comments_indentation::ID,
    document_end::ID,
    document_start::ID,
    empty_lines::ID,
    empty_values::ID,
    float_values::ID,
    hyphens::ID,
    indentation::ID,
    key_duplicates::ID,
    key_ordering::ID,
    line_length::ID,
    new_line_at_end_of_file::ID,
    new_lines::ID,
    octal_values::ID,
    quoted_strings::ID,
    trailing_spaces::ID,
    truthy::ID,
];
