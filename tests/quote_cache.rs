use ryl::rules::quoted_strings::{self, Config, QuoteStyle};

pub fn assert_cached_quote_matches_fresh(value: &str) {
    let quoted = serde_json::to_string(value).unwrap();
    let single = format!("first: {quoted}\n");
    let repeated = format!("{single}second: {quoted}\n");
    assert!(
        ryl_yaml_parser::Parser::new_from_str(&repeated).all(|event| event.is_ok())
    );

    let config =
        Config::ladder(QuoteStyle::Double, ryl::config::TargetVersion::default());
    let fresh = quoted_strings::check(&single, &config);
    let cached = quoted_strings::check(&repeated, &config);
    assert_eq!(cached.len(), fresh.len() * 2, "{value:?}");
    for (first, second) in fresh.iter().zip(cached.iter().skip(fresh.len())) {
        assert_eq!(first.message, second.message, "{value:?}");
        assert_eq!(first.column + 1, second.column, "{value:?}");
    }

    let fixed_single = quoted_strings::fix(&single, &config).unwrap_or(single);
    let fixed_repeated = quoted_strings::fix(&repeated, &config).unwrap_or(repeated);
    let fixed_value = fixed_single.strip_prefix("first: ").unwrap();
    assert_eq!(
        fixed_repeated,
        format!("{fixed_single}second: {fixed_value}")
    );
}
