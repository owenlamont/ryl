use std::collections::HashSet;
use std::fs;
use std::path::Path;

use ryl_yaml_parser::{Event, ScalarStyle};
use serde_json::{Map, Number, Value};

use crate::config::YamlLintConfig;
use crate::rules::support::span_utils::{BytePos, marker_byte_offset};
use crate::rules::{key_duplicates, support::yaml_version};
use crate::text_version::{Analysis, TextVersion, edits::render};
use crate::yaml_dom::{ScalarOwned, YamlOwned};

use super::bounded_json5;

#[derive(Clone, Copy)]
pub(super) enum Syntax {
    Yaml,
    Json,
    Json5,
    Jsonc,
    Toml,
}

pub(super) fn prettier(path: &Path) -> Result<Value, String> {
    let syntax = match path.extension().and_then(|extension| extension.to_str()) {
        Some("json") => Syntax::Json,
        Some("json5") => Syntax::Json5,
        Some("toml") => Syntax::Toml,
        _ => Syntax::Yaml,
    };
    value(path, syntax)
}

pub(super) fn value(path: &Path, syntax: Syntax) -> Result<Value, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let parsed = match syntax {
        Syntax::Json => serde_json::from_str(text).map_err(|error| error.to_string()),
        Syntax::Json5 => bounded_json5::parse(text).map_err(|error| error.to_string()),
        Syntax::Jsonc => {
            bounded_json5::parse_jsonc(text).map_err(|error| error.to_string())
        }
        Syntax::Toml => toml::from_str::<toml::Value>(text)
            .map_err(|error| error.to_string())
            .and_then(|value| {
                finite_toml(&value)?;
                Ok(serde_json::to_value(value).expect(
                    "finite TOML values have JSON-compatible scalars and string keys",
                ))
            }),
        Syntax::Yaml => yaml_document(text).and_then(yaml_value),
    };
    parsed.map_err(|error| format!("invalid config {}: {error}", path.display()))
}

pub(super) enum YamlfmtRead {
    Value(Value),
    Unsupported(&'static str),
}

pub(super) fn yamlfmt(path: &Path) -> Result<YamlfmtRead, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let version = TextVersion::new(text);
    let analysis = version.analysis();
    let document = checked_document(text, analysis)
        .map_err(|error| format!("invalid config {}: {error}", path.display()))?;
    if has_yamlfmt_merge(analysis) {
        return Ok(YamlfmtRead::Unsupported(
            "YAML merge keys are not imported; expand merged settings explicitly before rerunning migration",
        ));
    }
    let normalized = match yamlfmt_numbers(analysis) {
        Ok(normalized) => normalized,
        Err(reason) => return Ok(YamlfmtRead::Unsupported(reason)),
    };
    let document = match normalized {
        Some(text) => yaml_document(&text)
            .map_err(|error| format!("invalid config {}: {error}", path.display()))?,
        None => document,
    };
    yaml_value(document)
        .map(YamlfmtRead::Value)
        .map_err(|error| format!("invalid config {}: {error}", path.display()))
}

fn has_yamlfmt_merge(analysis: Analysis<'_>) -> bool {
    let positions: HashSet<_> = crate::rules::merge_keys::check_with_analysis(analysis)
        .into_iter()
        .map(|violation| (violation.line, violation.column))
        .collect();
    analysis.event_iter().any(|(event, span)| {
        matches!(event, Event::Scalar(value, ..) if value.as_ref() == "<<" && positions.contains(&(span.start.line(), span.start.col() + 1)))
    })
}

fn yaml_document(text: &str) -> Result<YamlOwned, String> {
    checked_document(text, TextVersion::new(text).analysis())
}

fn checked_document(text: &str, analysis: Analysis<'_>) -> Result<YamlOwned, String> {
    let mut documents = YamlOwned::load_from_str(text)
        .map_err(|error| format!("invalid YAML configuration: {error}"))?;
    if documents.len() > 1 {
        return Err("expected one YAML document".to_owned());
    }
    let config = key_duplicates::Config::resolve(&YamlLintConfig::default())
        .forbid_duplicate_merge_keys();
    if let Some(duplicate) =
        key_duplicates::check_with_analysis(analysis, config).first()
    {
        return Err(format!(
            "duplicate YAML configuration key at {}:{}",
            duplicate.line, duplicate.column
        ));
    }
    Ok(documents
        .pop()
        .unwrap_or(YamlOwned::Value(ScalarOwned::Null)))
}

fn yamlfmt_numbers(analysis: Analysis<'_>) -> Result<Option<String>, &'static str> {
    let mut edits = Vec::new();
    for (event, span) in analysis.event_iter() {
        let Event::Scalar(text, style, _, tag) = event else {
            continue;
        };
        let typed = tag.as_deref().and_then(crate::yaml_dom::core_schema_suffix);
        if !(style == ScalarStyle::Plain && tag.is_none()
            || matches!(typed.as_deref(), Some("int" | "float")))
            || !text.as_bytes().first().is_some_and(|byte| {
                byte.is_ascii_digit() || matches!(byte, b'-' | b'+')
            })
        {
            continue;
        }
        let core = yaml_version::value(&text, true, None, false);
        let source = yamlfmt_number(&text).or_else(|| core.clone());
        if source.is_none()
            && (yaml_version::resolves_to_nonstring_in_yaml_1_1(&text)
                || yaml_version::has_source_timestamp_shape(&text))
        {
            return Err(
                "source YAML scalar resolution is not imported; quote string values or use plain decimal numbers before rerunning migration",
            );
        }
        if text.contains(':') {
            continue;
        }
        if source == core && (typed.is_none() || style != ScalarStyle::Plain) {
            continue;
        }
        let spelling = match source {
            Some(yaml_version::Value::Int(value)) if i64::try_from(&value).is_ok() => {
                value.to_string()
            }
            Some(yaml_version::Value::Float(value)) if value.is_finite() => {
                format!("{:?}", value.into_inner())
            }
            Some(yaml_version::Value::Float(_)) => continue,
            _ => {
                return Err(
                    "YAML numeric spelling is not imported; use plain decimal numbers or configure ryl manually before rerunning migration",
                );
            }
        };
        if style != ScalarStyle::Plain {
            return Err(
                "quoted or block numeric spellings are not imported; use plain decimal numbers before rerunning migration",
            );
        }
        let start = marker_byte_offset(span.start);
        edits.push((start, BytePos::new(start.get() + text.len()), spelling));
    }
    Ok(render(analysis.text(), edits))
}

fn yamlfmt_number(text: &str) -> Option<yaml_version::Value> {
    let lower = text.to_ascii_lowercase();
    yaml_version::value(&lower, true, None, true).or_else(|| {
        let cleaned = lower.replace('_', "");
        let unsigned = cleaned.strip_prefix(['+', '-']).unwrap_or(&cleaned);
        if unsigned.starts_with("0o") {
            let value = crate::yaml_dom::core_schema_int(unsigned)?;
            return Some(yaml_version::Value::Int(if cleaned.starts_with('-') {
                -value
            } else {
                value
            }));
        }
        yaml_version::resolves_to_nonstring_in_yaml_1_1(text)
            .then(|| {
                cleaned
                    .parse::<f64>()
                    .ok()
                    .map(|value| yaml_version::Value::Float(value.into()))
            })
            .flatten()
    })
}

pub(super) fn object(value: Value, path: &Path) -> Result<Map<String, Value>, String> {
    match value {
        Value::Object(values) => Ok(values),
        _ => Err(format!(
            "invalid config {}: expected an object",
            path.display()
        )),
    }
}

fn yaml_value(value: YamlOwned) -> Result<Value, String> {
    match value {
        YamlOwned::Value(value) => match value {
            ScalarOwned::Null => Ok(Value::Null),
            ScalarOwned::Boolean(value) => Ok(value.into()),
            ScalarOwned::Integer(value) => Ok(value.into()),
            ScalarOwned::String(value) => Ok(value.into()),
            ScalarOwned::FloatingPoint(value) => Number::from_f64(value.into_inner())
                .map(Value::Number)
                .ok_or_else(|| {
                    "non-finite YAML configuration numbers are not supported".to_owned()
                }),
        },
        YamlOwned::Sequence(values) => values
            .into_iter()
            .map(yaml_value)
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        YamlOwned::Mapping(values) => values
            .into_iter()
            .map(|(key, value)| {
                let Some(key) = key.as_str() else {
                    return Err("configuration keys must be strings".to_owned());
                };
                Ok((key.to_owned(), yaml_value(value)?))
            })
            .collect::<Result<Map<_, _>, _>>()
            .map(Value::Object),
        YamlOwned::Tagged(_, _) => {
            Err("custom YAML configuration tags are not supported".to_owned())
        }
        YamlOwned::BadValue => Err("invalid YAML configuration value".to_owned()),
    }
}

fn finite_toml(value: &toml::Value) -> Result<(), String> {
    match value {
        toml::Value::Float(value) if !value.is_finite() => {
            Err("non-finite TOML configuration numbers are not supported".to_owned())
        }
        toml::Value::Array(values) => values.iter().try_for_each(finite_toml),
        toml::Value::Table(values) => values.values().try_for_each(finite_toml),
        _ => Ok(()),
    }
}
