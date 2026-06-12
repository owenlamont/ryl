use ryl::rules::hyphens::{self, Config, MESSAGE, MESSAGE_DASH_ON_OWN_LINE, Violation};

fn too_many_spaces(line: usize, column: usize) -> Violation {
    Violation {
        line,
        column,
        message: MESSAGE.to_string(),
    }
}

fn dash_on_own_line(line: usize, column: usize) -> Violation {
    Violation {
        line,
        column,
        message: MESSAGE_DASH_ON_OWN_LINE.to_string(),
    }
}

#[test]
fn allows_single_space_after_hyphen() {
    let cfg = Config::new_for_tests(1);
    let diagnostics = hyphens::check("- item\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn reports_too_many_spaces_in_root_sequence() {
    let cfg = Config::new_for_tests(1);
    let diagnostics = hyphens::check("-  item\n", &cfg);
    assert_eq!(diagnostics, vec![too_many_spaces(1, 3)]);
}

#[test]
fn reports_too_many_spaces_with_indentation() {
    let cfg = Config::new_for_tests(1);
    let diagnostics = hyphens::check("  -  item\n", &cfg);
    assert_eq!(diagnostics, vec![too_many_spaces(1, 5)]);
}

#[test]
fn respects_configured_max_spaces() {
    let cfg = Config::new_for_tests(3);
    let diagnostics = hyphens::check("-    item\n", &cfg);
    assert_eq!(diagnostics, vec![too_many_spaces(1, 5)]);

    let ok = hyphens::check("-   item\n", &cfg);
    assert!(ok.is_empty(), "unexpected diagnostics: {ok:?}");
}

#[test]
fn ignores_entries_with_comments_only() {
    let cfg = Config::new_for_tests(1);
    let diagnostics = hyphens::check("-  # comment\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn ignores_blank_lines() {
    let cfg = Config::new_for_tests(1);
    let diagnostics = hyphens::check("\n- item\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn ignores_entries_without_inline_values() {
    let cfg = Config::new_for_tests(1);
    let diagnostics = hyphens::check("-\n  key: value\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

// `dash-on-own-line` is off by default, so the spec-style requirement never fires
// unless explicitly enabled.
#[test]
fn dash_on_own_line_off_by_default() {
    let cfg = Config::new_for_tests(1);
    let diagnostics = hyphens::check("items:\n  - name: web\n    port: 80\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "default config must not require dash-on-own-line: {diagnostics:?}"
    );
}

#[test]
fn dash_on_own_line_flags_mapping_on_dash_line() {
    let cfg = Config::new_for_tests(1).with_dash_on_own_line(true);
    let diagnostics = hyphens::check("items:\n  - name: web\n    port: 80\n", &cfg);
    // Reported at the first key (`name`), the token that must move to the next line.
    assert_eq!(diagnostics, vec![dash_on_own_line(2, 5)]);
}

#[test]
fn dash_on_own_line_accepts_body_below_dash() {
    let cfg = Config::new_for_tests(1).with_dash_on_own_line(true);
    let diagnostics =
        hyphens::check("items:\n  -\n    name: web\n    port: 80\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "dash alone with body below must be accepted: {diagnostics:?}"
    );
}

// A dash line carrying only node properties keeps the mapping keys below it, so the
// spec-style layout is satisfied and nothing is flagged.
#[test]
fn dash_on_own_line_accepts_anchor_or_tag_before_body() {
    let cfg = Config::new_for_tests(1).with_dash_on_own_line(true);
    let diagnostics = hyphens::check("items:\n  - &a !x\n    name: web\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "anchor/tag on the dash line with keys below must be accepted: {diagnostics:?}"
    );
}

#[test]
fn dash_on_own_line_accepts_comment_after_dash() {
    let cfg = Config::new_for_tests(1).with_dash_on_own_line(true);
    let diagnostics = hyphens::check("items:\n  - # c\n    name: web\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "a comment after the dash with keys below must be accepted: {diagnostics:?}"
    );
}

// Only block mappings trigger the option: scalars, aliases, nested block sequences,
// and flow collections on the dash line are all left alone.
#[test]
fn dash_on_own_line_ignores_non_mapping_entries() {
    let cfg = Config::new_for_tests(1).with_dash_on_own_line(true);
    for input in [
        "items:\n  - scalar\n",       // scalar entry
        "items:\n  - *anchor\n",      // alias entry
        "items:\n  - [1, 2]\n",       // flow sequence value
        "items:\n  - {x: 1}\n",       // flow mapping value
        "items:\n  - - x\n    - y\n", // nested block sequence
    ] {
        let diagnostics = hyphens::check(input, &cfg);
        assert!(
            diagnostics.is_empty(),
            "non-mapping entry must not be flagged for {input:?}: {diagnostics:?}"
        );
    }
}

// A block mapping that is a *mapping value* (no preceding `-`) is never a sequence
// entry, so the option must not flag it.
#[test]
fn dash_on_own_line_ignores_mapping_value() {
    let cfg = Config::new_for_tests(1).with_dash_on_own_line(true);
    let diagnostics = hyphens::check("foo:\n  bar: 1\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "a block mapping value must not be flagged: {diagnostics:?}"
    );
}

#[test]
fn dash_on_own_line_flags_each_top_level_entry() {
    let cfg = Config::new_for_tests(1).with_dash_on_own_line(true);
    let diagnostics = hyphens::check("- name: web\n  port: 80\n- name: db\n", &cfg);
    assert_eq!(
        diagnostics,
        vec![dash_on_own_line(1, 3), dash_on_own_line(3, 3)]
    );
}

// The inner sequence's mapping opens on the inner dash line, so the nested entry is
// flagged while the outer (sequence) entry is not.
#[test]
fn dash_on_own_line_flags_nested_sequence_of_mappings() {
    let cfg = Config::new_for_tests(1).with_dash_on_own_line(true);
    let diagnostics = hyphens::check("- - a: 1\n", &cfg);
    assert_eq!(diagnostics, vec![dash_on_own_line(1, 5)]);
}

// Both passes contribute on one line; the combined result stays in document order.
#[test]
fn dash_on_own_line_and_max_spaces_sort_in_document_order() {
    let cfg = Config::new_for_tests(1).with_dash_on_own_line(true);
    let diagnostics = hyphens::check("x:\n  -   name: web\n", &cfg);
    assert_eq!(
        diagnostics,
        vec![too_many_spaces(2, 6), dash_on_own_line(2, 7)]
    );
}

// Char-aligned columns on a multibyte dash line (issue #232): the key column counts
// characters, not bytes.
#[test]
fn dash_on_own_line_reports_char_columns_on_multibyte_line() {
    let cfg = Config::new_for_tests(1).with_dash_on_own_line(true);
    let diagnostics = hyphens::check("café:\n  - café: web\n", &cfg);
    assert_eq!(diagnostics, vec![dash_on_own_line(2, 5)]);
}
