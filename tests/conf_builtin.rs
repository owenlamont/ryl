use ryl::conf::builtin;

#[test]
fn builtin_presets_are_available() {
    assert!(builtin("default").is_some());
    assert!(builtin("relaxed").is_some());
    assert!(builtin("empty").is_some());
    assert!(builtin("nonexistent").is_none());
}
