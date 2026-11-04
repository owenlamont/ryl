use serde_json::{Map, Value};

use crate::config_schema::{
    CollectionStyleTarget, ExpandTarget, FormatTable, MarkerTarget,
    PropertyOrderTarget, QuoteStyleTarget,
};

use super::options::{Converted, Options};

pub(super) fn convert(values: Map<String, Value>) -> Result<Converted, String> {
    let mut options = Options::new("yamlfix", values);
    let indent_value = options.integer("indent_mapping", 2)?;
    let width_value = options.integer("line_length", 80)?;
    let indent = options.dimension("indent_mapping", indent_value);
    let width = options.dimension("line_length", width_value);
    let sequence_indent = options.integer("indent_sequence", 4)?;
    let offset = options.integer("indent_offset", 2)?;
    let indent_sequences = offset != 0;
    if sequence_indent != indent_value.saturating_add(offset)
        || (offset != 0 && offset != indent_value)
    {
        options.note("independent indent_sequence/indent_offset settings cannot be represented; using indent_mapping and the closest sequence-indent policy");
    }
    let (quote_style, preserve_quotes) = quote_policy(&mut options)?;
    let start = options.boolean("explicit_start", true)?;
    if !start {
        options.warn(
            "explicit_start",
            &Value::Bool(false),
            "document-start removal is not available; existing markers are preserved",
        );
    }
    let spacing_value = options.integer("comments_min_spaces_from_content", 2)?;
    let comment_spacing =
        options.spacing("comments_min_spaces_from_content", spacing_value, 2);
    let starting_space = options.boolean("comments_require_starting_space", true)?;
    let blank_value = options.integer("whitelines", 0)?;
    let max_blank_lines = options.blank_limit("whitelines", blank_value);
    if blank_value != 0 {
        options.warn(
            "whitelines",
            &Value::from(blank_value),
            "ryl caps blank runs rather than inserting an exact number of blank lines",
        );
    }
    let comment_blanks = options.integer("comments_whitelines", 1)?;
    let section_blanks = options.integer("section_whitelines", 0)?;
    if comment_blanks != 0 || section_blanks != 0 {
        options.note("separate comment/section blank-line minima are not available");
    }
    let null = options.text("none_representation", "")?;
    options.warn("none_representation", &Value::from(null), "null spelling normalization is not available; existing spellings are preserved");
    let style = options.choice(
        "sequence_style",
        "flow_style",
        &["flow_style", "block_style", "keep_style"],
    )?;
    let sequence_style = match style.as_str() {
        "flow_style" => {
            options.warn(
                "sequence_style",
                &Value::from("flow_style"),
                "ryl may write overflowing flow collections in block style",
            );
            CollectionStyleTarget::Flow
        }
        "block_style" => CollectionStyleTarget::Block,
        _ => CollectionStyleTarget::Preserve,
    };
    for key in ["quote_basic_values", "quote_keys_and_basic_values"] {
        if options.boolean(key, false)? {
            options.warn(
                key,
                &Value::Bool(true),
                "forcing quotes onto plain strings is not available",
            );
        }
    }
    options.boolean("allow_duplicate_keys", false)?;
    options.unmapped(
        "config_path",
        "environment/CLI discovery overrides are not imported",
        "a string",
        Value::is_string,
    )?;
    options.note("yamlfix joins plain/quoted continuations but preserves folded breaks; retaining ryl's split-only folding because per-style prose policies are not available");
    let targets = FormatTable {
        quote_style,
        keep_quotes: preserve_quotes,
        document_start: if start {
            MarkerTarget::Add
        } else {
            MarkerTarget::Preserve
        },
        fold_long_lines: Some(true),
        comment_spacing,
        comment_starting_space: if starting_space {
            MarkerTarget::Add
        } else {
            MarkerTarget::Preserve
        },
        max_blank_lines,
        sequence_style,
        indent_sequences,
        sequence_expand: ExpandTarget::Preserve,
        mapping_expand: ExpandTarget::Preserve,
        property_order: PropertyOrderTarget::AnchorFirst,
        ..FormatTable::default()
    };
    Ok(options.finish(targets, indent, width))
}

fn quote_policy(options: &mut Options) -> Result<(QuoteStyleTarget, bool), String> {
    let preserve_quotes = options.boolean("preserve_quotes", false)?;
    let representation = options.choice("quote_representation", "'", &["'", "\""])?;
    let quote_style = if preserve_quotes {
        QuoteStyleTarget::Preserve
    } else if representation == "'" {
        QuoteStyleTarget::Single
    } else {
        QuoteStyleTarget::Double
    };
    Ok((quote_style, preserve_quotes))
}
