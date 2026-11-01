use ryl::rules::commas::{
    Config, check, coverage_compute_spaces_before, coverage_overlapping_replacements,
};

#[test]
fn leading_comma_reports_zero_spaces() {
    assert_eq!(coverage_compute_spaces_before(",", 0), Some(0));
}

#[test]
fn leading_space_before_comma_counts_correctly() {
    assert_eq!(coverage_compute_spaces_before(" ,", 1), Some(1));
}

#[test]
fn newline_before_comma_is_ignored() {
    assert_eq!(coverage_compute_spaces_before("\n,", 1), None);
}

#[test]
fn overlapping_replacements_preserve_right_to_left_order() {
    assert_eq!(coverage_overlapping_replacements(), "aX");
}

#[test]
fn stray_comma_outside_flow_is_ignored() {
    let cfg = Config::new(0, 1, 1);
    assert!(check(",", &cfg).is_empty());
}

#[test]
fn commas_after_multibyte_scalars_are_checked() {
    let cfg = Config::new(0, 1, 1);
    let violations = check("[\"å\",\"b\"]", &cfg);
    assert!(
        violations
            .iter()
            .any(|v| v.message == "too few spaces after comma")
    );
}
