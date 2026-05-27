use ryl::yaml_dom::{ScalarOwned, YamlOwned};

fn parse_single(source: &str) -> YamlOwned {
    YamlOwned::load_from_str(source)
        .expect("input should parse")
        .into_iter()
        .next()
        .expect("at least one document")
}

#[test]
fn parses_hex_integer() {
    let doc = parse_single("v: 0xFF\n");
    assert_eq!(
        doc.as_mapping_get("v").and_then(YamlOwned::as_integer),
        Some(255)
    );
}

#[test]
fn parses_octal_integer() {
    let doc = parse_single("v: 0o17\n");
    assert_eq!(
        doc.as_mapping_get("v").and_then(YamlOwned::as_integer),
        Some(15)
    );
}

#[test]
fn parses_explicit_positive_integer() {
    let doc = parse_single("v: +42\n");
    assert_eq!(
        doc.as_mapping_get("v").and_then(YamlOwned::as_integer),
        Some(42)
    );
}

#[test]
fn yaml_1_2_core_schema_null_and_bool_spellings_resolve() {
    for spelling in ["~", "null", "Null", "NULL"] {
        let doc = parse_single(&format!("v: {spelling}\n"));
        assert!(
            doc.as_mapping_get("v").is_some_and(YamlOwned::is_null),
            "{spelling} should resolve to null"
        );
    }
    for spelling in ["true", "True", "TRUE"] {
        let doc = parse_single(&format!("v: {spelling}\n"));
        assert_eq!(
            doc.as_mapping_get("v").and_then(YamlOwned::as_bool),
            Some(true),
            "{spelling} should resolve to true"
        );
    }
    for spelling in ["false", "False", "FALSE"] {
        let doc = parse_single(&format!("v: {spelling}\n"));
        assert_eq!(
            doc.as_mapping_get("v").and_then(YamlOwned::as_bool),
            Some(false),
            "{spelling} should resolve to false"
        );
    }
}

#[test]
fn yaml_1_1_only_booleans_stay_strings() {
    // ryl targets YAML 1.2; `Yes`/`No`/`On`/`Off` are YAML 1.1 booleans and
    // must remain strings so they round-trip rather than being retyped.
    for spelling in ["Yes", "No", "On", "Off", "yes", "no"] {
        let doc = parse_single(&format!("v: {spelling}\n"));
        assert_eq!(
            doc.as_mapping_get("v").and_then(YamlOwned::as_str),
            Some(spelling),
            "{spelling} should stay a string under YAML 1.2"
        );
    }
}

#[test]
fn parses_infinity_floats() {
    let pos = parse_single("v: .inf\n");
    let neg = parse_single("v: -.inf\n");
    let nan = parse_single("v: .nan\n");
    assert!(
        pos.as_mapping_get("v")
            .and_then(YamlOwned::as_floating_point)
            .is_some_and(f64::is_infinite)
    );
    assert!(
        neg.as_mapping_get("v")
            .and_then(YamlOwned::as_floating_point)
            .is_some_and(|f| f.is_infinite() && f.is_sign_negative())
    );
    assert!(
        nan.as_mapping_get("v")
            .and_then(YamlOwned::as_floating_point)
            .is_some_and(f64::is_nan)
    );
}

#[test]
fn resolves_core_schema_int_tag() {
    let doc = parse_single("v: !!int 42\n");
    assert_eq!(
        doc.as_mapping_get("v").and_then(YamlOwned::as_integer),
        Some(42)
    );
}

#[test]
fn resolves_core_schema_bool_tag() {
    let doc = parse_single("v: !!bool true\n");
    assert_eq!(
        doc.as_mapping_get("v").and_then(YamlOwned::as_bool),
        Some(true)
    );
}

#[test]
fn resolves_core_schema_str_tag_forces_string() {
    let doc = parse_single("v: !!str 42\n");
    assert_eq!(
        doc.as_mapping_get("v").and_then(YamlOwned::as_str),
        Some("42")
    );
}

#[test]
fn resolves_core_schema_null_tag() {
    let doc = parse_single("v: !!null ~\n");
    assert!(doc.as_mapping_get("v").is_some_and(YamlOwned::is_null));
}

#[test]
fn resolves_core_schema_float_tag() {
    let doc = parse_single("v: !!float 1.5\n");
    assert_eq!(
        doc.as_mapping_get("v")
            .and_then(YamlOwned::as_floating_point),
        Some(1.5)
    );
}

#[test]
fn unknown_core_schema_tag_is_bad_value() {
    let doc = parse_single("v: !!unknown foo\n");
    let v = doc.as_mapping_get("v").unwrap();
    assert!(matches!(v, YamlOwned::BadValue));
}

#[test]
fn non_core_tagged_scalar_wraps_in_tagged() {
    let doc = parse_single("v: !foo bar\n");
    assert!(matches!(
        doc.as_mapping_get("v"),
        Some(YamlOwned::Tagged(_, _))
    ));
}

#[test]
fn non_core_tagged_sequence_wraps_in_tagged() {
    let doc = parse_single("v: !foo [1, 2]\n");
    assert!(matches!(
        doc.as_mapping_get("v"),
        Some(YamlOwned::Tagged(_, _))
    ));
}

#[test]
fn non_core_tagged_mapping_wraps_in_tagged() {
    let doc = parse_single("v: !foo {a: b}\n");
    assert!(matches!(
        doc.as_mapping_get("v"),
        Some(YamlOwned::Tagged(_, _))
    ));
}

#[test]
fn anchored_collection_resolves_alias() {
    let doc = parse_single("a: &anchor\n  - 1\n  - 2\nb: *anchor\n");
    let seq = doc
        .as_mapping_get("b")
        .and_then(YamlOwned::as_sequence)
        .expect("alias resolves to sequence");
    assert_eq!(seq.len(), 2);
}

#[test]
fn as_sequence_returns_none_for_non_sequence() {
    let scalar = YamlOwned::Value(ScalarOwned::String("foo".to_owned()));
    assert!(scalar.as_sequence().is_none());
}

#[test]
fn scalar_anchor_is_resolved_via_alias() {
    let doc = parse_single("a: &x foo\nb: *x\n");
    assert_eq!(
        doc.as_mapping_get("b").and_then(YamlOwned::as_str),
        Some("foo")
    );
}

#[test]
fn invalid_hex_falls_back_to_string() {
    let doc = parse_single("v: 0xZZ\n");
    assert_eq!(
        doc.as_mapping_get("v").and_then(YamlOwned::as_str),
        Some("0xZZ")
    );
}

#[test]
fn invalid_octal_falls_back_to_string() {
    let doc = parse_single("v: 0oZZ\n");
    assert_eq!(
        doc.as_mapping_get("v").and_then(YamlOwned::as_str),
        Some("0oZZ")
    );
}

#[test]
fn invalid_signed_int_falls_back_to_string() {
    let doc = parse_single("v: +abc\n");
    assert_eq!(
        doc.as_mapping_get("v").and_then(YamlOwned::as_str),
        Some("+abc")
    );
}

#[test]
fn core_schema_null_tag_rejects_non_null_value() {
    let doc = parse_single("v: !!null foo\n");
    assert!(matches!(doc.as_mapping_get("v"), Some(YamlOwned::BadValue)));
}

#[test]
fn as_mapping_mut_returns_none_for_non_mapping() {
    let mut scalar = YamlOwned::Value(ScalarOwned::Integer(1));
    assert!(scalar.as_mapping_mut().is_none());
}
