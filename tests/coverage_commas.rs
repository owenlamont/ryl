use ryl::rules::commas::coverage_compute_spaces_before;

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
