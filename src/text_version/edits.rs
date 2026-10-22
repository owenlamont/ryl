use crate::rules::support::span_utils::{BytePos, apply_replacements};

pub(crate) type Edit = (BytePos, BytePos, String);

pub(crate) fn render(buffer: &str, edits: Vec<Edit>) -> Option<String> {
    (!edits.is_empty()).then(|| apply_replacements(buffer, edits))
}
