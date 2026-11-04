use std::num::NonZeroU8;

use serde_json::{Map, Value};

use crate::config_schema::{
    ExpandTarget, FormatTable, LineEndingTarget, MarkerTarget, PropertyOrderTarget,
    ProseWrap, QuoteStyleTarget,
};

use super::options::{Converted, Options, string_or_strings, strings};

pub(super) fn convert(mut values: Map<String, Value>) -> Result<Converted, String> {
    let yaml = match values.remove("yaml") {
        None => Map::new(),
        Some(Value::Object(values)) => values,
        Some(_) => {
            return Err(
                "invalid dprint yaml configuration: expected an object".to_owned()
            );
        }
    };
    let mut global = Options::new("dprint", values);
    let mut options = Options::new("pretty_yaml", yaml);
    let global_indent = global.strict_integer("indentWidth", 2)?;
    let global_width = global.strict_integer("lineWidth", 80)?;
    let indent_value = options.strict_integer("indentWidth", global_indent)?;
    let width_value = options.strict_integer("printWidth", global_width)?;
    let indent = options.dimension("indentWidth", indent_value);
    let width = options.dimension("printWidth", width_value);
    let global_ending = global.choice("newLineKind", "lf", &["lf", "crlf", "auto"])?;
    let ending = options.choice(
        "lineBreak",
        if global_ending == "crlf" {
            "crlf"
        } else {
            "lf"
        },
        &["lf", "crlf"],
    )?;
    let line_ending = if ending == "crlf" {
        LineEndingTarget::CrLf
    } else {
        LineEndingTarget::Lf
    };
    let quote = options.choice(
        "quotes",
        "preferDouble",
        &["preferDouble", "preferSingle", "forceDouble", "forceSingle"],
    )?;
    let quote_style = if quote.ends_with("Single") {
        QuoteStyleTarget::Single
    } else {
        QuoteStyleTarget::Double
    };
    if quote.starts_with("force") {
        options.warn("quotes", &Value::from(quote), "forcing quotes onto plain strings is not available; existing quoted presentation is retained");
    }
    let format_comments = options.boolean("formatComments", false)?;
    let mut collection_targets = FormatTable::default();
    collection_policy(&mut options, &mut collection_targets)?;
    let prose_wrap = match options
        .choice("proseWrap", "preserve", &["preserve", "always"])?
        .as_str()
    {
        "always" => ProseWrap::Always,
        _ => ProseWrap::Preserve,
    };
    whitespace_policy(&mut options)?;
    let marker = options.text("ignoreCommentDirective", "pretty-yaml-ignore")?;
    options.warn("ignoreCommentDirective", &Value::from(marker), "source comments require separate opt-in directive conversion; no runtime alias is added");
    global.boolean("useTabs", false)?;
    warn_unmapped(&mut global, &mut options)?;
    let targets = FormatTable {
        quote_style,
        keep_quotes: true,
        line_ending,
        prose_wrap: Some(prose_wrap),
        comment_spacing: NonZeroU8::new(1).expect("1 is non-zero"),
        comment_starting_space: if format_comments {
            MarkerTarget::Add
        } else {
            MarkerTarget::Preserve
        },
        max_blank_lines: 1,
        property_order: PropertyOrderTarget::Preserve,
        ..collection_targets
    };
    let mut converted = options.finish(targets, indent, width);
    converted.cleanup_safe &= !global.has_unknown_options();
    converted.warnings.extend(global.into_warnings());
    Ok(converted)
}

fn collection_policy(
    options: &mut Options,
    targets: &mut FormatTable,
) -> Result<(), String> {
    let indent_sequences = options.boolean("indentBlockSequenceInMap", true)?;
    let brace_spacing = options.boolean("braceSpacing", true)?;
    if options.boolean("bracketSpacing", false)? {
        options.warn(
            "bracketSpacing",
            &Value::Bool(true),
            "flow-sequence bracket padding is not available",
        );
    }
    let dash = options.choice("dashSpacing", "oneSpace", &["oneSpace", "indent"])?;
    if dash == "indent" {
        options.warn(
            "dashSpacing",
            &Value::from(dash),
            "indent-width-dependent dash padding is not available",
        );
    }
    let single = options.boolean("preferSingleLine", false)?;
    let sequence_single = options
        .optional_boolean("flowSequence.preferSingleLine")?
        .unwrap_or(single);
    let mapping_single = options
        .optional_boolean("flowMap.preferSingleLine")?
        .unwrap_or(single);
    options.note("flow-collection wrapping and retained physical breaks may differ at the configured width");
    targets.indent_sequences = indent_sequences;
    targets.brace_spacing = brace_spacing;
    targets.sequence_expand = if sequence_single {
        ExpandTarget::Auto
    } else {
        ExpandTarget::Preserve
    };
    targets.mapping_expand = if mapping_single {
        ExpandTarget::Auto
    } else {
        ExpandTarget::Preserve
    };
    Ok(())
}

fn whitespace_policy(options: &mut Options) -> Result<(), String> {
    if !options.boolean("trailingComma", true)? {
        options.warn(
            "trailingComma",
            &Value::Bool(false),
            "expanded flow collections retain ryl's trailing-comma policy",
        );
    }
    if !options.boolean("trimTrailingWhitespaces", true)? {
        options.warn(
            "trimTrailingWhitespaces",
            &Value::Bool(false),
            "ryl removes non-value-bearing trailing whitespace",
        );
    }
    if options.boolean("trimTrailingZero", false)? {
        options.warn(
            "trimTrailingZero",
            &Value::Bool(true),
            "numeric spelling normalization is not available",
        );
    }
    Ok(())
}

fn warn_unmapped(global: &mut Options, options: &mut Options) -> Result<(), String> {
    global.unmapped(
        "plugins",
        "plugin code is not imported",
        "an array of strings",
        strings,
    )?;
    for key in ["includes", "excludes"] {
        global.unmapped(
            key,
            "source file selection is not imported",
            "an array of strings",
            strings,
        )?;
    }
    global.unmapped(
        "extends",
        "shared configurations are not fetched",
        "a string or an array of strings",
        string_or_strings,
    )?;
    global.unmapped(
        "incremental",
        "source command behavior is not imported",
        "a boolean",
        Value::is_boolean,
    )?;
    options.unmapped(
        "locked",
        "plugin configuration locking is not imported",
        "a boolean",
        Value::is_boolean,
    )?;
    options.unmapped(
        "associations",
        "plugin file associations are not imported",
        "a string or an array of strings",
        string_or_strings,
    )?;
    Ok(())
}
