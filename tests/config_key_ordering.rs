use ryl::config::YamlLintConfig;

#[test]
fn ignored_keys_sequence_accepts_valid_entries() {
    let cfg = YamlLintConfig::from_yaml_str(
        "rules:\n  key-ordering:\n    ignored-keys: [\"name\", \"^b\"]\n",
    )
    .expect("config should parse");
    let resolved =
        ryl::rules::key_ordering::Config::resolve(&cfg, std::path::Path::new("t.yaml"));
    let hits = ryl::rules::key_ordering::check("b: 1\na: 1\n", &resolved);
    assert!(
        hits.is_empty(),
        "ignored keys should skip enforcement: {hits:?}"
    );
}

#[test]
fn ignored_keys_sequence_non_string_errors() {
    let err = YamlLintConfig::from_yaml_str(
        "rules:\n  key-ordering:\n    ignored-keys: [1]\n",
    )
    .expect_err("non-string sequence entries should error");
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.key-ordering"), "{err}");
}

#[test]
fn ignored_keys_sequence_invalid_regex_errors() {
    let err = YamlLintConfig::from_yaml_str(
        "rules:\n  key-ordering:\n    ignored-keys: [\"[\"]\n",
    )
    .expect_err("invalid regex should error");
    assert!(err.contains("invalid regex"), "unexpected message: {err}");
}

#[test]
fn ignored_keys_scalar_invalid_regex_errors() {
    let err = YamlLintConfig::from_yaml_str(
        "rules:\n  key-ordering:\n    ignored-keys: \"[\"\n",
    )
    .expect_err("invalid scalar regex should error");
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.key-ordering"), "{err}");
}

#[test]
fn ignored_keys_invalid_type_errors() {
    let err = YamlLintConfig::from_yaml_str(
        "rules:\n  key-ordering:\n    ignored-keys: {bad: true}\n",
    )
    .expect_err("non sequence/string should error");
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.key-ordering"), "{err}");
}

#[test]
fn key_ordering_unknown_option_errors() {
    let err = YamlLintConfig::from_yaml_str(
        "rules:\n  key-ordering:\n    unexpected: true\n",
    )
    .expect_err("unknown option should error");
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.key-ordering"), "{err}");
}

#[test]
fn key_ordering_non_string_key_errors() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  key-ordering:\n    1: true\n")
        .expect_err("non-string key should error");
    assert!(err.contains("cannot convert non-string TOML key"), "{err}");
}

fn orders_error(entry: &str) -> String {
    YamlLintConfig::from_toml_str(&format!(
        "[rules.key-ordering]\n\n[[rules.key-ordering.orders]]\n{entry}\n"
    ))
    .expect_err("invalid orders entry should error")
}

#[test]
fn orders_entries_are_validated() {
    let valid = "files = [\"*\"]\npath = \"$\"\nkeys = [\"a\"]";
    let cases = [
        (valid.replace("[\"*\"]", "[]"), "empty `files`"),
        (
            valid.replace("[\"*\"]", "[\"!a[\"]"),
            "invalid `files` glob '!a['",
        ),
        (valid.replace("[\"a\"]", "[]"), "empty `keys`"),
        (
            valid.replace("[\"a\"]", "[\"a\", \"a\"]"),
            "lists key 'a' twice",
        ),
        (valid.replace("\"$\"", "\"a\""), "must start with `$`"),
        (valid.replace("\"$\"", "\"$..a\""), "descendant segments"),
        (valid.replace("\"$\"", "\"$[0]\""), "array indices"),
        (valid.replace("\"$\"", "\"$[-1]\""), "array indices"),
        (valid.replace("\"$\"", "\"$.1a\""), "needs a bracketed name"),
        (valid.replace("\"$\"", "\"$.-a\""), "needs a bracketed name"),
        (valid.replace("\"$\"", "\"$['a\""), "malformed name"),
        (
            valid.replace("\"$\"", "\"$['a']x\""),
            "`x` is not a supported",
        ),
        (valid.replace("\"$\"", "\"$['a'\""), "malformed name"),
        (valid.replace("\"$\"", "'$[\"a\\n\"]'"), "malformed name"),
        (
            valid.replace("\"$\"", "\"$[?@.a]\""),
            "not a supported selector",
        ),
    ];
    for (entry, expected) in cases {
        let err = orders_error(&entry);
        assert!(err.contains(expected), "{entry}: {err}");
        assert!(err.contains("entry 1 of option \"orders\""), "{err}");
    }
}

#[test]
fn yaml_orders_and_unknown_unlisted_values_are_rejected() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  key-ordering:\n    orders: []\n")
            .expect_err("orders is TOML-only");
    assert!(err.contains("rules.key-ordering"), "{err}");
    let err = orders_error(
        "files = [\"*\"]\npath = \"$\"\nkeys = [\"a\"]\nunlisted = \"drop\"",
    );
    assert!(err.contains("failed to parse config data"), "{err}");
}

#[test]
fn orders_round_trip_through_toml() {
    let toml = "[rules.key-ordering]\n\n[[rules.key-ordering.orders]]\n\
                files = [\"*\"]\npath = \"$.a['b c'][*]\"\nkeys = [\"z\", \"y\"]\n\
                unlisted = \"keep\"\n";
    let rendered = YamlLintConfig::from_toml_str(toml)
        .expect("valid orders parse")
        .to_toml_string();
    assert!(
        rendered.contains("[[rules.key-ordering.orders]]"),
        "{rendered}"
    );
    let reparsed = YamlLintConfig::from_toml_str(&rendered).expect("round trip parses");
    assert_eq!(reparsed.to_toml_string(), rendered);
}
