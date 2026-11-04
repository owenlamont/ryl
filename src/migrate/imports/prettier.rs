use std::num::NonZeroU8;

use serde_json::{Map, Value};

use crate::config_schema::{
    ExpandTarget, FormatTable, LineEndingTarget, MarkerTarget, PropertyOrderTarget,
    ProseWrap, QuoteStyleTarget,
};

use super::options::{Converted, Options, string_or_strings, truthy};

pub(super) fn convert(
    values: Map<String, Value>,
    migrate_directives: bool,
) -> Result<Converted, String> {
    let mut options = Options::new("Prettier", values).with_null_defaults();
    let indent_value = options.integer("tabWidth", 2)?;
    let width_value = options.integer("printWidth", 80)?;
    let indent = options.dimension("tabWidth", indent_value);
    let width = options.dimension("printWidth", width_value);
    let quote_style = if options.boolean("singleQuote", false)? {
        QuoteStyleTarget::Single
    } else {
        QuoteStyleTarget::Double
    };
    let brace_spacing = options.boolean("bracketSpacing", true)?;
    let prose_wrap = match options
        .choice("proseWrap", "preserve", &["preserve", "always", "never"])?
        .as_str()
    {
        "always" => ProseWrap::Always,
        "never" => ProseWrap::Never,
        _ => ProseWrap::Preserve,
    };
    let ending = options.choice("endOfLine", "lf", &["lf", "crlf", "auto", "cr"])?;
    let line_ending = match ending.as_str() {
        "crlf" => LineEndingTarget::CrLf,
        "auto" => {
            options.warn("endOfLine", &Value::from("auto"), "Prettier uses the first line ending; ryl uses the most frequent ending");
            LineEndingTarget::Auto
        }
        "cr" => {
            options.warn(
                "endOfLine",
                &Value::from("cr"),
                "bare-CR output is not available; using LF",
            );
            LineEndingTarget::Lf
        }
        _ => LineEndingTarget::Lf,
    };
    let _ = options.boolean("useTabs", false)?;
    options.unmapped("overrides", "per-file formatter configuration is not available; only root options are imported", "an array of overrides or null", valid_overrides)?;
    options.unmapped(
        "plugins",
        "plugin code and plugin-dependent formatting are not imported",
        "an array of plugin names or objects",
        |value| {
            value.as_array().is_some_and(|plugins| {
                plugins
                    .iter()
                    .all(|plugin| plugin.is_string() || plugin.is_object())
            })
        },
    )?;
    options.unmapped(
        "requirePragma",
        "pragma-based file selection is not imported",
        "a boolean or null",
        |value| value.is_null() || value.is_boolean(),
    )?;
    options.unmapped(
        "insertPragma",
        "pragma insertion is not available",
        "a boolean or null",
        |value| value.is_null() || value.is_boolean(),
    )?;
    options.unmapped(
        "checkIgnorePragma",
        "source ignore pragmas require separate opt-in directive conversion",
        "a boolean or null",
        |value| value.is_null() || value.is_boolean(),
    )?;
    if !migrate_directives {
        options.note("Prettier ignore comments are not rewritten by config migration; convert them separately before replacing the formatter");
    }
    let targets = FormatTable {
        quote_style,
        keep_quotes: true,
        brace_spacing,
        prose_wrap: Some(prose_wrap),
        line_ending,
        comment_spacing: NonZeroU8::new(1).expect("1 is non-zero"),
        comment_starting_space: MarkerTarget::Preserve,
        max_blank_lines: 1,
        sequence_expand: ExpandTarget::Auto,
        mapping_expand: ExpandTarget::Auto,
        property_order: PropertyOrderTarget::TagFirst,
        ..FormatTable::default()
    };
    Ok(options.finish(targets, indent, width))
}

fn valid_overrides(value: &Value) -> bool {
    !truthy(value)
        || value.as_array().is_some_and(|overrides| {
            overrides.iter().all(|item| {
                item.as_object().is_some_and(|item| {
                    item.get("files").is_some_and(string_or_strings)
                        && item.get("excludeFiles").is_none_or(|value| {
                            !truthy(value) || string_or_strings(value)
                        })
                })
            })
        })
}
