use crate::fix::NewlinePolicy;
use crate::format::layout::Permission;
use crate::rules::support::span_utils::BytePos;

pub(super) struct Initial<'a> {
    buffer: &'a str,
    policy: Option<&'a NewlinePolicy>,
    permission: Permission<'a>,
}

impl<'a> Initial<'a> {
    pub(super) const fn new(
        buffer: &'a str,
        policy: Option<&'a NewlinePolicy>,
        permission: Permission<'a>,
    ) -> Self {
        Self {
            buffer,
            policy,
            permission,
        }
    }
    pub(super) fn enabled(&self) -> bool {
        self.policy.is_some()
            && self.permission.allows(
                BytePos::new(self.buffer.len()),
                BytePos::new(self.buffer.len()),
            )
    }
    pub(super) fn ending(&self) -> &'static str {
        match self
            .policy
            .expect("a final ending is selected only when enabled")
            .newline(self.buffer)
        {
            "\r\n" => "\r\n",
            "\r" => "\r",
            _ => "\n",
        }
    }
}
