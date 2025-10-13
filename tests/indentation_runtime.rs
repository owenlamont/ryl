use ryl::rules::indentation::{
    SpacesSetting, test_observe_increase_with_allowed, test_observe_indent_with_allowed,
};

#[test]
fn observe_increase_allowed_updates_consistent_step() {
    let updated = test_observe_increase_with_allowed(Some(4), 0, 6, Some(6));
    assert_eq!(
        updated,
        Some(2),
        "expected gcd reduction when allowed delta differs"
    );
}

#[test]
fn observe_indent_allowed_skips_diagnostics() {
    let hits = test_observe_indent_with_allowed(SpacesSetting::Consistent, Some(2), 4, Some(4));
    assert!(
        hits.is_empty(),
        "allowed indentation should not emit diagnostics: {hits:?}"
    );
}
