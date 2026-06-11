use crate::yaml_dom::{MappingOwned, ScalarOwned, YamlOwned};
use serde::Serialize;

use super::{
    FixTable, MarkdownTable, NormalizedConfig, NormalizedFixConfig, NormalizedMarkdown,
    NormalizedPerLineIgnore, PerLineIgnore, RuleName, RulesTable, StringOrVec,
    TomlConfig, YamlConfig,
};

pub(crate) fn string_or_vec_items(value: &StringOrVec) -> Vec<String> {
    match value {
        StringOrVec::One(item) => vec![item.clone()],
        StringOrVec::Many(items) => items.clone(),
    }
}

fn ignore_patterns_from_string_or_vec(value: &StringOrVec) -> Vec<String> {
    match value {
        StringOrVec::One(item) => super::patterns_from_scalar(item),
        StringOrVec::Many(items) => items
            .iter()
            .flat_map(|item| super::patterns_from_scalar(item))
            .collect(),
    }
}

fn normalize_fix_table(fix: &FixTable) -> NormalizedFixConfig {
    NormalizedFixConfig {
        fixable: fix
            .fixable
            .clone()
            .unwrap_or_else(|| vec![super::FixableRuleSelector::All]),
        unfixable: fix.unfixable.clone().unwrap_or_default(),
    }
}

fn normalize_per_file_ignores(
    ignores: &std::collections::BTreeMap<String, Vec<RuleName>>,
) -> std::collections::BTreeMap<String, Vec<String>> {
    ignores
        .iter()
        .map(|(pattern, rules)| {
            (
                pattern.clone(),
                rules.iter().map(|rule| rule.as_str().to_string()).collect(),
            )
        })
        .collect()
}

fn normalize_per_line_ignores(
    ignores: &[PerLineIgnore],
) -> Vec<NormalizedPerLineIgnore> {
    ignores
        .iter()
        .map(|entry| NormalizedPerLineIgnore {
            regex: entry.regex.clone(),
            path: entry.path.clone(),
            rules: entry
                .rules
                .iter()
                .map(|rule| rule.as_str().to_string())
                .collect(),
        })
        .collect()
}

#[must_use]
pub(crate) fn toml_value_to_yaml_owned(value: &toml::Value) -> YamlOwned {
    match value {
        toml::Value::String(text) => {
            YamlOwned::Value(ScalarOwned::String(text.clone()))
        }
        toml::Value::Integer(num) => YamlOwned::Value(ScalarOwned::Integer(*num)),
        toml::Value::Float(num) => {
            let rendered = num.to_string();
            YamlOwned::load_from_str(&rendered)
                .ok()
                .and_then(|docs| docs.into_iter().next())
                .unwrap_or(YamlOwned::Value(ScalarOwned::String(rendered)))
        }
        toml::Value::Boolean(flag) => YamlOwned::Value(ScalarOwned::Boolean(*flag)),
        toml::Value::Datetime(dt) => {
            YamlOwned::Value(ScalarOwned::String(dt.to_string()))
        }
        toml::Value::Array(items) => {
            YamlOwned::Sequence(items.iter().map(toml_value_to_yaml_owned).collect())
        }
        toml::Value::Table(table) => {
            let mut map = MappingOwned::new();
            for (key, val) in table {
                map.insert(
                    YamlOwned::Value(ScalarOwned::String(key.clone())),
                    toml_value_to_yaml_owned(val),
                );
            }
            YamlOwned::Mapping(map)
        }
    }
}

pub(crate) fn yaml_owned_to_toml_value(
    value: &YamlOwned,
) -> Result<toml::Value, String> {
    if let Some(text) = value.as_str() {
        return Ok(toml::Value::String(text.to_string()));
    }
    if let Some(flag) = value.as_bool() {
        return Ok(toml::Value::Boolean(flag));
    }
    if let Some(num) = value.as_integer() {
        return Ok(toml::Value::Integer(num));
    }
    if value.is_null() {
        return Err(
            "cannot convert null values to TOML (TOML has no null type)".to_string()
        );
    }
    if let Some(items) = value.as_sequence() {
        let out: Result<Vec<_>, _> =
            items.iter().map(yaml_owned_to_toml_value).collect();
        return out.map(toml::Value::Array);
    }
    if let Some(map) = value.as_mapping() {
        let mut out = toml::map::Map::new();
        for (key, val) in map {
            let Some(key_text) = key.as_str() else {
                return Err(format!("cannot convert non-string TOML key: {key:?}"));
            };
            out.insert(key_text.to_string(), yaml_owned_to_toml_value(val)?);
        }
        return Ok(toml::Value::Table(out));
    }
    Err("cannot convert this YAML node to TOML".to_string())
}

/// Normalize a typed TOML config into a shared post-parse representation.
///
/// # Panics
/// Panics if serializing already-validated typed TOML rules unexpectedly stops
/// producing a TOML table.
pub fn normalize_toml_config(config: &TomlConfig) -> NormalizedConfig {
    NormalizedConfig {
        ignore_patterns: config
            .ignore
            .as_ref()
            .map(ignore_patterns_from_string_or_vec),
        ignore_from_files: config.ignore_from_file.as_ref().map(string_or_vec_items),
        per_file_ignores: config
            .per_file_ignores
            .as_ref()
            .map_or_else(std::collections::BTreeMap::new, normalize_per_file_ignores),
        per_line_ignores: config
            .per_line_ignores
            .as_deref()
            .map_or_else(Vec::new, normalize_per_line_ignores),
        yaml_file_patterns: config.files.as_ref().and_then(|files| files.yaml.clone()),
        markdown_file_patterns: config
            .files
            .as_ref()
            .and_then(|files| files.markdown.clone()),
        markdown: config.markdown.as_ref().map(normalize_markdown_table),
        locale: config.locale.clone(),
        fix: config.fix.as_ref().map(normalize_fix_table),
        rules: config
            .rules
            .as_ref()
            .map_or_else(std::collections::BTreeMap::new, normalized_rules_from_table),
    }
}

fn normalize_markdown_table(table: &MarkdownTable) -> NormalizedMarkdown {
    NormalizedMarkdown {
        front_matter: table.front_matter,
        fenced_blocks: table.fenced_blocks,
    }
}

pub fn normalize_yaml_config(config: &YamlConfig) -> NormalizedConfig {
    NormalizedConfig {
        ignore_patterns: config.ignore.as_ref().map(string_or_vec_items),
        ignore_from_files: config.ignore_from_file.as_ref().map(string_or_vec_items),
        per_file_ignores: std::collections::BTreeMap::new(),
        per_line_ignores: Vec::new(),
        yaml_file_patterns: config.yaml_files.clone(),
        markdown_file_patterns: None,
        markdown: None,
        locale: config.locale.clone(),
        fix: None,
        rules: config
            .rules
            .as_ref()
            .map_or_else(std::collections::BTreeMap::new, normalized_rules_from_table),
    }
}

fn normalized_rules_from_table<Q: Serialize, K: Serialize, A: Serialize>(
    rules: &RulesTable<Q, K, A>,
) -> std::collections::BTreeMap<String, YamlOwned> {
    rules_table_to_value(rules)
        .as_table()
        .expect("serializing typed rules should yield a table")
        .clone()
        .into_iter()
        .map(|(name, value)| (name, toml_value_to_yaml_owned(&value)))
        .collect()
}

fn insert_serialized<T: Serialize>(
    table: &mut toml::map::Map<String, toml::Value>,
    key: &str,
    value: Option<&T>,
) {
    if let Some(value) = value {
        table.insert(
            key.to_string(),
            toml::Value::try_from(value)
                .expect("serializing typed TOML value should succeed"),
        );
    }
}

fn rules_table_to_value<Q: Serialize, K: Serialize, A: Serialize>(
    rules: &RulesTable<Q, K, A>,
) -> toml::Value {
    let mut table = toml::map::Map::new();
    insert_serialized(&mut table, "anchors", rules.anchors.as_ref());
    insert_serialized(
        &mut table,
        "block-scalar-chomping",
        rules.block_scalar_chomping.as_ref(),
    );
    insert_serialized(&mut table, "braces", rules.braces.as_ref());
    insert_serialized(&mut table, "brackets", rules.brackets.as_ref());
    insert_serialized(&mut table, "colons", rules.colons.as_ref());
    insert_serialized(&mut table, "commas", rules.commas.as_ref());
    insert_serialized(&mut table, "comments", rules.comments.as_ref());
    insert_serialized(
        &mut table,
        "comments-indentation",
        rules.comments_indentation.as_ref(),
    );
    insert_serialized(&mut table, "document-end", rules.document_end.as_ref());
    insert_serialized(&mut table, "document-start", rules.document_start.as_ref());
    insert_serialized(&mut table, "empty-lines", rules.empty_lines.as_ref());
    insert_serialized(&mut table, "empty-values", rules.empty_values.as_ref());
    insert_serialized(&mut table, "float-values", rules.float_values.as_ref());
    insert_serialized(&mut table, "hyphens", rules.hyphens.as_ref());
    insert_serialized(&mut table, "indentation", rules.indentation.as_ref());
    insert_serialized(&mut table, "key-duplicates", rules.key_duplicates.as_ref());
    insert_serialized(&mut table, "key-ordering", rules.key_ordering.as_ref());
    insert_serialized(&mut table, "line-length", rules.line_length.as_ref());
    insert_serialized(&mut table, "merge-keys", rules.merge_keys.as_ref());
    insert_serialized(
        &mut table,
        "new-line-at-end-of-file",
        rules.new_line_at_end_of_file.as_ref(),
    );
    insert_serialized(&mut table, "new-lines", rules.new_lines.as_ref());
    insert_serialized(&mut table, "octal-values", rules.octal_values.as_ref());
    insert_serialized(&mut table, "quoted-strings", rules.quoted_strings.as_ref());
    insert_serialized(&mut table, "tags", rules.tags.as_ref());
    insert_serialized(
        &mut table,
        "trailing-spaces",
        rules.trailing_spaces.as_ref(),
    );
    insert_serialized(&mut table, "truthy", rules.truthy.as_ref());
    insert_serialized(
        &mut table,
        "unicode-line-breaks",
        rules.unicode_line_breaks.as_ref(),
    );
    table.extend(rules.extra.clone());
    toml::Value::Table(table)
}

/// Convert a typed TOML config model into a TOML value tree.
///
/// # Panics
/// Panics if serializing the typed config into TOML unexpectedly fails.
#[must_use]
pub fn toml_config_to_value(config: &TomlConfig) -> toml::Value {
    let mut table = toml::map::Map::new();
    insert_serialized(&mut table, "files", config.files.as_ref());
    insert_serialized(&mut table, "markdown", config.markdown.as_ref());
    insert_serialized(&mut table, "ignore", config.ignore.as_ref());
    insert_serialized(
        &mut table,
        "ignore-from-file",
        config.ignore_from_file.as_ref(),
    );
    insert_serialized(&mut table, "locale", config.locale.as_ref());
    insert_serialized(&mut table, "fix", config.fix.as_ref());
    insert_serialized(
        &mut table,
        "per-file-ignores",
        config.per_file_ignores.as_ref(),
    );
    insert_serialized(
        &mut table,
        "per-line-ignores",
        config.per_line_ignores.as_ref(),
    );
    if let Some(rules) = config.rules.as_ref() {
        table.insert("rules".to_string(), rules_table_to_value(rules));
    }
    table.extend(config.extra.clone());
    toml::Value::Table(table)
}

fn insert_string_array(
    table: &mut toml::map::Map<String, toml::Value>,
    key: &str,
    values: &[String],
) {
    table.insert(
        key.to_string(),
        toml::Value::Array(
            values
                .iter()
                .map(|item| toml::Value::String(item.clone()))
                .collect(),
        ),
    );
}

fn normalized_fix_to_toml_value(fix: &NormalizedFixConfig) -> toml::Value {
    let mut table = toml::map::Map::new();
    insert_serialized(&mut table, "fixable", Some(&fix.fixable));
    insert_serialized(&mut table, "unfixable", Some(&fix.unfixable));
    toml::Value::Table(table)
}

fn normalized_rules_to_toml_value(
    rules: &std::collections::BTreeMap<String, YamlOwned>,
) -> toml::Value {
    let mut table = toml::map::Map::new();
    for (name, value) in rules {
        table.insert(
            name.clone(),
            yaml_owned_to_toml_value(value)
                .expect("normalized config should only contain TOML-compatible values"),
        );
    }
    toml::Value::Table(table)
}

#[must_use]
pub fn normalized_config_to_toml_value(config: &NormalizedConfig) -> toml::Value {
    let mut table = toml::map::Map::new();

    if let Some(files) = normalized_files_to_toml_value(config) {
        table.insert("files".to_string(), files);
    }

    if let Some(markdown) = config.markdown.as_ref() {
        table.insert(
            "markdown".to_string(),
            normalized_markdown_to_toml_value(markdown),
        );
    }

    if let Some(ignore_from_file) = config.ignore_from_files.as_ref() {
        insert_string_array(&mut table, "ignore-from-file", ignore_from_file);
    } else if let Some(ignore) = config.ignore_patterns.as_ref() {
        insert_string_array(&mut table, "ignore", ignore);
    }

    if let Some(locale) = config.locale.as_ref() {
        table.insert("locale".to_string(), toml::Value::String(locale.clone()));
    }

    if let Some(fix) = config.fix.as_ref() {
        table.insert("fix".to_string(), normalized_fix_to_toml_value(fix));
    }

    if !config.per_file_ignores.is_empty() {
        table.insert(
            "per-file-ignores".to_string(),
            normalized_per_file_ignores_to_toml_value(&config.per_file_ignores),
        );
    }

    if !config.per_line_ignores.is_empty() {
        table.insert(
            "per-line-ignores".to_string(),
            normalized_per_line_ignores_to_toml_value(&config.per_line_ignores),
        );
    }

    if !config.rules.is_empty() {
        table.insert(
            "rules".to_string(),
            normalized_rules_to_toml_value(&config.rules),
        );
    }

    toml::Value::Table(table)
}

fn normalized_files_to_toml_value(config: &NormalizedConfig) -> Option<toml::Value> {
    let mut table = toml::map::Map::new();
    if let Some(yaml) = config.yaml_file_patterns.as_ref() {
        insert_string_array(&mut table, "yaml", yaml);
    }
    if let Some(markdown) = config.markdown_file_patterns.as_ref() {
        insert_string_array(&mut table, "markdown", markdown);
    }
    (!table.is_empty()).then_some(toml::Value::Table(table))
}

fn normalized_markdown_to_toml_value(markdown: &NormalizedMarkdown) -> toml::Value {
    let mut table = toml::map::Map::new();
    table.insert(
        "front-matter".to_string(),
        toml::Value::Boolean(markdown.front_matter.unwrap_or(true)),
    );
    table.insert(
        "fenced-blocks".to_string(),
        toml::Value::Boolean(markdown.fenced_blocks.unwrap_or(true)),
    );
    toml::Value::Table(table)
}

fn normalized_per_file_ignores_to_toml_value(
    per_file_ignores: &std::collections::BTreeMap<String, Vec<String>>,
) -> toml::Value {
    toml::Value::Table(
        per_file_ignores
            .iter()
            .map(|(pattern, rules)| {
                (
                    pattern.clone(),
                    toml::Value::Array(
                        rules
                            .iter()
                            .map(|rule| toml::Value::String(rule.clone()))
                            .collect(),
                    ),
                )
            })
            .collect(),
    )
}

fn normalized_per_line_ignores_to_toml_value(
    per_line_ignores: &[NormalizedPerLineIgnore],
) -> toml::Value {
    toml::Value::Array(
        per_line_ignores
            .iter()
            .map(|entry| {
                let mut table = toml::map::Map::new();
                if let Some(regex) = entry.regex.as_ref() {
                    table.insert(
                        "regex".to_string(),
                        toml::Value::String(regex.clone()),
                    );
                }
                if let Some(path) = entry.path.as_ref() {
                    table.insert("path".to_string(), toml::Value::String(path.clone()));
                }
                insert_string_array(&mut table, "rules", &entry.rules);
                toml::Value::Table(table)
            })
            .collect(),
    )
}
