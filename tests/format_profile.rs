//! The formatter page documents every `[format]` default twice, in the full example and
//! the key table, and gives each a rationale in the profile table; all three must match
//! `FormatTable::default()` and the top-level defaults `ryl format` uses.

use std::collections::BTreeMap;

use ryl::config_schema::FormatTable;
use ryl::format::{DEFAULT_INDENT_WIDTH, DEFAULT_LINE_LENGTH};

fn page() -> String {
    std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/docs/formatter.md"))
        .expect("the formatter page exists")
}

/// The lines of the page's `## <heading>` section.
fn section<'a>(page: &'a str, heading: &str) -> Vec<&'a str> {
    page.lines()
        .skip_while(|line| *line != format!("## {heading}"))
        .skip(1)
        .take_while(|line| !line.starts_with("## "))
        .collect()
}

fn toml_value(text: &str) -> toml::Value {
    toml::from_str::<toml::Table>(&format!("v = {text}"))
        .unwrap_or_else(|error| panic!("{text:?} is not a TOML value: {error}"))["v"]
        .clone()
}

/// Each table row's first two cells, with their backticks stripped, keyed by the first.
fn rows(lines: &[&str]) -> BTreeMap<String, String> {
    lines
        .iter()
        .filter_map(|line| {
            let mut cells = line.strip_prefix("| `")?.split(" | ");
            let first = cells.next()?.trim_end_matches('`');
            Some((first.to_owned(), cells.next()?.trim_matches('`').to_owned()))
        })
        .collect()
}

fn expected() -> toml::Table {
    let mut defaults = toml::Table::try_from(FormatTable::default())
        .expect("the [format] table serializes");
    defaults.insert("line-length".into(), i64::from(DEFAULT_LINE_LENGTH).into());
    defaults.insert(
        "indent-width".into(),
        i64::from(DEFAULT_INDENT_WIDTH).into(),
    );
    defaults
}

#[test]
fn the_configuration_example_and_key_table_show_every_default() {
    let page = page();
    let lines = section(&page, "Configuration");
    let example: String = lines
        .iter()
        .skip_while(|line| **line != "```toml")
        .skip(1)
        .take_while(|line| **line != "```")
        .map(|line| format!("{line}\n"))
        .collect();
    let example: toml::Table = toml::from_str(&example).expect("the example parses");
    let mut format = expected();
    format.remove("line-length");
    format.remove("indent-width");
    assert_eq!(example["format"], toml::Value::Table(format.clone()));
    let table: toml::Table = rows(&lines)
        .into_iter()
        .map(|(key, default)| (key, toml_value(&default)))
        .collect();
    assert_eq!(table, format);
}

#[test]
fn the_profile_table_gives_every_default_a_reason() {
    let page = page();
    let profile: toml::Table = rows(&section(&page, "Default profile"))
        .into_keys()
        .map(|assignment| {
            let (key, value) = assignment
                .split_once(" = ")
                .unwrap_or_else(|| panic!("{assignment:?} is not `key = value`"));
            (key.to_owned(), toml_value(value))
        })
        .collect();
    assert_eq!(profile, expected());
}
