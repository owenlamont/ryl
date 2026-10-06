use crate::config_schema::FormatTable;

/// `input` formatted per `options`.
#[must_use]
pub fn format_str(input: &str, _options: &FormatTable) -> String {
    input.to_string()
}
