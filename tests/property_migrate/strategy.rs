use proptest::prelude::*;
use ryl::migrate::MigrationSource;
use serde_json::{Value, json};

#[derive(Clone, Debug)]
pub struct Source {
    pub tool: MigrationSource,
    pub filename: &'static str,
    pub body: String,
    pub indent: u8,
    pub width: u16,
}

pub fn arb_source_timestamp_string() -> impl Strategy<Value = String> {
    (
        1970_u16..2101,
        1_u8..13,
        1_u8..29,
        0_u8..24,
        0_u8..60,
        0_u8..60,
        any::<bool>(),
        0_u8..5,
    )
        .prop_map(|(year, month, day, hour, minute, second, padded, form)| {
            let date = if padded {
                format!("{year}-{month:02}-{day:02}")
            } else {
                format!("{year}-{month}-{day}")
            };
            match form {
                0 => date,
                1 => format!("{date}T{hour}:{minute}:{second}Z"),
                2 => format!("{date}t{hour}:{minute}:{second},4Z"),
                3 => format!("{date} {hour}:{minute}:{second}"),
                _ => format!("{date}T{hour:02}:{minute:02}:{second:02}.5+05:30"),
            }
        })
}

pub fn arb_source() -> impl Strategy<Value = Source> {
    (0_u8..4, 1_u8..9, 20_u16..161, any::<bool>(), "[a-zA-Z0-9_ \n\r\t\\\"]{0,30}", 0_u8..4)
        .prop_map(|(tool, indent, width, single, unknown, representation)| {
            let (tool, filename, body) = match tool {
                0 => (MigrationSource::Prettier, ".prettierrc.json5", format!("{{tabWidth:{indent},printWidth:{width},singleQuote:{single},unknown:{}}}", json!(unknown))),
                1 => {
                    let (indent, width) = match representation {
                        1 => (format!("0{indent:o}"), format!("0{width:o}")),
                        2 => (format!("{indent}.5"), format!("{width}.9")),
                        3 => (format!("0X{indent:X}"), format!("0X{width:X}")),
                        _ => (indent.to_string(), width.to_string()),
                    };
                    (MigrationSource::Yamlfmt, ".yamlfmt", format!("formatter:\n  indent: {indent}\n  max_line_length: {width}\n  unknown: {}\n", json!(unknown)))
                },
                2 => (MigrationSource::Dprint, "dprint.jsonc", json!({"yaml":{"indentWidth":indent,"printWidth":width,"unknown":unknown}}).to_string()),
                _ => {
                    let mut values = toml::Table::new();
                    values.insert("indent_mapping".to_owned(), toml::Value::Integer(indent.into()));
                    values.insert("line_length".to_owned(), toml::Value::Integer(width.into()));
                    values.insert("preserve_quotes".to_owned(), toml::Value::Boolean(single));
                    values.insert("unknown".to_owned(), toml::Value::String(unknown));
                    (MigrationSource::Yamlfix, "yamlfix.toml", toml::to_string(&values).unwrap())
                }
            };
            let indent = if tool == MigrationSource::Yamlfmt && indent == 1 { 2 } else { indent };
            Source { tool, filename, body, indent, width }
        })
}

fn arb_value() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(|value| json!(value)),
        proptest::num::f64::NORMAL.prop_map(|value| Value::Number(
            serde_json::Number::from_f64(value).expect("normal floats are finite")
        )),
        "[a-zA-Z0-9_ \n\r\t]{0,30}".prop_map(Value::String),
    ]
    .prop_recursive(3, 24, 6, |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 0..5).prop_map(Value::Array),
            proptest::collection::btree_map("[a-z]{1,8}", inner, 0..5)
                .prop_map(|values| Value::Object(values.into_iter().collect())),
        ]
    })
}

pub fn arb_hostile_source()
-> impl Strategy<Value = (MigrationSource, &'static str, String)> {
    (0_u8..7, any::<usize>(), arb_value()).prop_map(|(tool, index, value)| {
        let keys: &[&str] = match tool {
            0 => &[
                "tabWidth",
                "printWidth",
                "singleQuote",
                "bracketSpacing",
                "proseWrap",
                "endOfLine",
                "useTabs",
                "plugins",
                "overrides",
                "requirePragma",
                "insertPragma",
                "checkIgnorePragma",
            ],
            1 | 4 => &[
                "type",
                "indent",
                "max_line_length",
                "force_quote_style",
                "include_document_start",
                "retain_line_breaks",
                "retain_line_breaks_single",
                "array_indent",
                "indent_root_array",
                "indentless_arrays",
                "force_array_style",
                "pad_line_comments",
                "scan_folded_as_literal",
                "disallow_anchors",
                "drop_merge_tag",
                "trim_trailing_whitespace",
                "eof_newline",
                "strip_directives",
                "disable_alias_key_correction",
                "line_ending",
            ],
            2 => &[
                "indentWidth",
                "printWidth",
                "lineBreak",
                "quotes",
                "formatComments",
                "indentBlockSequenceInMap",
                "braceSpacing",
                "bracketSpacing",
                "dashSpacing",
                "preferSingleLine",
                "flowSequence.preferSingleLine",
                "flowMap.preferSingleLine",
                "proseWrap",
                "trailingComma",
                "trimTrailingWhitespaces",
                "trimTrailingZero",
                "ignoreCommentDirective",
                "locked",
                "associations",
            ],
            5 => &[
                "include",
                "exclude",
                "regex_exclude",
                "extensions",
                "match_type",
                "gitignore_path",
                "output_format",
                "doublestar",
                "gitignore_excludes",
                "continue_on_error",
            ],
            6 => &["plugins", "includes", "excludes", "extends", "incremental"],
            _ => &[
                "indent_mapping",
                "line_length",
                "indent_sequence",
                "indent_offset",
                "preserve_quotes",
                "quote_representation",
                "explicit_start",
                "comments_min_spaces_from_content",
                "comments_require_starting_space",
                "whitelines",
                "comments_whitelines",
                "section_whitelines",
                "none_representation",
                "sequence_style",
                "quote_basic_values",
                "quote_keys_and_basic_values",
                "allow_duplicate_keys",
                "config_path",
            ],
        };
        let key = keys[index % keys.len()];
        match tool {
            0 => (
                MigrationSource::Prettier,
                ".prettierrc.json5",
                format!("{{{}:{value},unknown:{value}}}", json!(key)),
            ),
            1 => (
                MigrationSource::Yamlfmt,
                ".yamlfmt",
                format!("{{formatter:{{{}:{value},unknown:{value}}}}}", json!(key)),
            ),
            2 => (
                MigrationSource::Dprint,
                "dprint.jsonc",
                format!("{{yaml:{{{}:{value},unknown:{value}}}}}", json!(key)),
            ),
            4 => (
                MigrationSource::Yamlfmt,
                ".yamlfmt",
                format!("formatter: {{<<: {{{}:{value}}}}}", json!(key)),
            ),
            5 => (
                MigrationSource::Yamlfmt,
                ".yamlfmt",
                format!("{{{}:{value},unknown:{value}}}", json!(key)),
            ),
            6 => (
                MigrationSource::Dprint,
                "dprint.jsonc",
                format!("{{{}:{value},yaml:{{}},unknown:{value}}}", json!(key)),
            ),
            _ => {
                let body = toml::Value::try_from(&value)
                    .and_then(|value| {
                        toml::to_string(&toml::Table::from_iter([
                            (key.to_owned(), value.clone()),
                            ("unknown".to_owned(), value),
                        ]))
                    })
                    .unwrap_or_else(|_| format!("{key}="));
                (MigrationSource::Yamlfix, "yamlfix.toml", body)
            }
        }
    })
}
