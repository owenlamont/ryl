use serde_json::{Map, Value};

use crate::config_schema::{
    CollectionStyleTarget, ExpandTarget, FormatTable, LineEndingTarget, MarkerTarget,
    PropertyOrderTarget, ProseWrap, QuoteStyleTarget,
};

use super::options::{Converted, Options, strings};

pub(super) fn convert(mut values: Map<String, Value>) -> Result<Converted, String> {
    let formatter = match values.remove("formatter") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(value)) => value,
        Some(_) => {
            return Err("invalid yamlfmt formatter: expected a mapping".to_owned());
        }
    };
    let mut root = Options::new("yamlfmt", values).with_null_defaults();
    let mut options = Options::new("yamlfmt basic", formatter)
        .with_null_defaults()
        .with_integer_truncation();
    options.text("type", "basic")?;
    let requested_indent = options.integer("indent", 2)?;
    if requested_indent < 0 {
        return Err(
            "invalid yamlfmt basic option indent: must not be negative".to_owned()
        );
    }
    let indent_value = match requested_indent {
        0 => 4,
        2..=9 => requested_indent,
        _ => 2,
    };
    let requested_width = options.integer("max_line_length", 0)?;
    let width_value = if requested_width > 0 && requested_width <= indent_value * 2 {
        80
    } else {
        requested_width
    };
    let indent = options.dimension("indent", indent_value);
    let width = if width_value == 0 {
        None
    } else {
        options.dimension("max_line_length", width_value)
    };
    let line_ending = match root.typed("line_ending") {
        None => LineEndingTarget::Native,
        Some(Value::String(value)) if value.is_empty() => LineEndingTarget::Native,
        Some(Value::String(value)) if value == "lf" => LineEndingTarget::Lf,
        Some(Value::String(value)) if value == "crlf" => LineEndingTarget::CrLf,
        _ => return Err("invalid yamlfmt line_ending: expected lf or crlf".to_owned()),
    };
    options.text("line_ending", "")?;
    let include_start = options.boolean("include_document_start", false)?;
    if !include_start {
        options.warn(
            "include_document_start",
            &Value::Bool(false),
            "document-start removal is not available; existing markers are preserved",
        );
    }
    let max_blank_lines = blank_limit(&mut options)?;
    let (sequence_style, indent_sequences) =
        collection_policy(&mut options, requested_indent, indent_value)?;
    let quote_style = quote_policy(&mut options)?;
    let padding_value = options.integer("pad_line_comments", 1)?;
    let comment_spacing = options.spacing("pad_line_comments", padding_value, 1);
    let scan_folded = options.boolean("scan_folded_as_literal", false)?;
    let prose_wrap = if scan_folded {
        options.warn("scan_folded_as_literal", &Value::Bool(true), "per-style prose policies are not available; physical prose breaks are preserved");
        ProseWrap::Preserve
    } else if width_value > 0 {
        ProseWrap::Always
    } else {
        ProseWrap::Never
    };
    if width_value > 0 {
        options.note("flow-collection wrapping differs; ryl retains its own collection layout rules");
    }
    let excludes = source_excludes(&mut root)?;
    warn_unmapped(&mut options, &mut root)?;
    let targets = FormatTable {
        quote_style,
        keep_quotes: true,
        line_ending,
        document_start: if include_start {
            MarkerTarget::Add
        } else {
            MarkerTarget::Preserve
        },
        prose_wrap: Some(prose_wrap),
        max_blank_lines,
        indent_sequences,
        sequence_style,
        sequence_expand: ExpandTarget::Never,
        mapping_expand: ExpandTarget::Never,
        comment_spacing,
        comment_starting_space: MarkerTarget::Preserve,
        property_order: PropertyOrderTarget::AnchorFirst,
        ..FormatTable::default()
    };
    let mut converted = options.finish(targets, indent, width);
    converted.excludes = excludes;
    converted.cleanup_safe &= !root.has_unknown_options();
    converted.warnings.extend(root.into_warnings());
    Ok(converted)
}

fn source_excludes(root: &mut Options) -> Result<Vec<String>, String> {
    if !root.values_match("match_type", "doublestar") {
        return Ok(Vec::new());
    }
    root.take("match_type");
    Ok(root
        .strings("exclude")?
        .into_iter()
        .map(|pattern| format!("/{pattern}"))
        .collect())
}

fn quote_policy(options: &mut Options) -> Result<QuoteStyleTarget, String> {
    let quote = options.choice("force_quote_style", "", &["", "single", "double"])?;
    let quote_style = match quote.as_str() {
        "single" => QuoteStyleTarget::Single,
        "double" => QuoteStyleTarget::Double,
        _ => QuoteStyleTarget::Preserve,
    };
    Ok(quote_style)
}

fn collection_policy(
    options: &mut Options,
    requested_indent: i64,
    indent_value: i64,
) -> Result<(CollectionStyleTarget, bool), String> {
    let requested_array = options.integer("array_indent", 0)?;
    let array_indent = if requested_array > 0 {
        requested_array
    } else if requested_indent == 0 {
        4
    } else {
        requested_indent
    };
    if array_indent != indent_value {
        options.warn(
            "array_indent",
            &Value::from(array_indent),
            "independent sequence indentation is not available; using indent",
        );
    }
    if options.boolean("indent_root_array", false)? {
        options.warn(
            "indent_root_array",
            &Value::Bool(true),
            "root sequence indentation is not available",
        );
    }
    let indent_sequences = !options.boolean("indentless_arrays", false)?;
    let array_style =
        options.choice("force_array_style", "", &["", "flow", "block"])?;
    let sequence_style = match array_style.as_str() {
        "flow" => {
            options.warn(
                "force_array_style",
                &Value::from("flow"),
                "ryl may write overflowing flow collections in block style",
            );
            CollectionStyleTarget::Flow
        }
        "block" => CollectionStyleTarget::Block,
        _ => CollectionStyleTarget::Preserve,
    };
    Ok((sequence_style, indent_sequences))
}

fn blank_limit(options: &mut Options) -> Result<u8, String> {
    let retain = options.boolean("retain_line_breaks", false)?;
    let retain_single = options.boolean("retain_line_breaks_single", false)?;
    let max_blank_lines = if retain_single {
        1
    } else if retain {
        options.warn(
            "retain_line_breaks",
            &Value::Bool(true),
            "unlimited blank-line retention is not available; using ryl's cap of 255",
        );
        255
    } else {
        0
    };
    Ok(max_blank_lines)
}

fn warn_unmapped(options: &mut Options, root: &mut Options) -> Result<(), String> {
    for key in [
        "disallow_anchors",
        "drop_merge_tag",
        "trim_trailing_whitespace",
        "eof_newline",
        "strip_directives",
        "disable_alias_key_correction",
    ] {
        options.unmapped(key, "source-specific formatter policy is not available; ryl preserves YAML values and properties", "a boolean or null", |value| value.is_null() || value.is_boolean())?;
    }
    for key in ["include", "exclude", "regex_exclude", "extensions"] {
        root.unmapped(
            key,
            "source file selection and command behavior are not imported",
            "an array of strings or null",
            |value| value.is_null() || strings(value),
        )?;
    }
    for key in ["match_type", "gitignore_path"] {
        root.unmapped(
            key,
            "source command behavior is not imported",
            "a string or null",
            |value| value.is_null() || value.is_string(),
        )?;
    }
    root.unmapped(
        "output_format",
        "source command output formatting is not imported",
        "default, line, an empty string or null",
        |value| {
            value.is_null() || matches!(value.as_str(), Some("" | "default" | "line"))
        },
    )?;
    for key in ["doublestar", "gitignore_excludes", "continue_on_error"] {
        root.unmapped(
            key,
            "source command behavior is not imported",
            "a boolean or null",
            |value| value.is_null() || value.is_boolean(),
        )?;
    }
    Ok(())
}
