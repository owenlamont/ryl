use crate::rules::support::span_utils::{BytePos, apply_replacements};

pub(crate) type Edit = (BytePos, BytePos, String);

pub(crate) fn render(buffer: &str, edits: Vec<Edit>) -> Option<String> {
    (!edits.is_empty()).then(|| apply_replacements(buffer, edits))
}

#[derive(Default)]
pub(crate) struct Batch {
    edits: Vec<Edit>,
    rules: Vec<&'static str>,
}

impl Batch {
    pub(crate) fn push(&mut self, rule: &'static str, edits: Vec<Edit>) {
        if !edits.is_empty() {
            self.rules.push(rule);
            self.edits.extend(edits);
        }
    }

    pub(crate) fn changed_rules(&self) -> &[&'static str] {
        &self.rules
    }

    pub(crate) fn finish(self, input: super::Analysis<'_>) -> Option<super::EditPlan> {
        input.plan_byte_edits(
            self.edits
                .into_iter()
                .map(|(start, end, text)| (start.get()..end.get(), text))
                .collect(),
        )
    }
}
