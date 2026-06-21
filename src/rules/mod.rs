pub mod anchors;
pub mod block_scalar_chomping;
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
pub mod merge_keys;
pub mod new_line_at_end_of_file;
pub mod new_lines;
pub mod octal_values;
pub mod quoted_strings;
pub(crate) mod support;
pub mod tags;
pub mod trailing_spaces;
pub mod truthy;
pub mod unicode_line_breaks;

/// Every rule id; the directive engine expands a bare `disable`/`enable` to this.
/// Extend when adding a rule.
pub const ALL_RULE_IDS: [&str; 27] = [
    anchors::ID,
    block_scalar_chomping::ID,
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
    merge_keys::ID,
    new_line_at_end_of_file::ID,
    new_lines::ID,
    octal_values::ID,
    quoted_strings::ID,
    tags::ID,
    trailing_spaces::ID,
    truthy::ID,
    unicode_line_breaks::ID,
];

/// Rules with no yamllint equivalent, so configurable only via TOML: the YAML config
/// rejects them and the YAML schema prunes them, reserving the YAML `rules` namespace
/// for yamllint's own definitions. Extend when adding a rule yamllint does not have.
pub const RYL_ONLY_RULE_IDS: [&str; 4] = [
    block_scalar_chomping::ID,
    merge_keys::ID,
    tags::ID,
    unicode_line_breaks::ID,
];
