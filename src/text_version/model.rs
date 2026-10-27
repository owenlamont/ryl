use std::cell::OnceCell;

use ryl_yaml_parser::Tag;

use crate::rules::support::yaml_version::{
    Version, resolves_as_yaml_1_1, resolves_to_nonstring_in_yaml_1_1,
};
use crate::yaml_dom::{Scalar as Value, is_core_schema, is_core_schema_int_spelling};

#[derive(Clone, Copy)]
pub(crate) enum Role {
    Key,
    Value,
}

#[derive(Clone, Copy)]
pub(crate) struct Comment<'a> {
    value: &'a str,
    magic: bool,
}

impl<'a> Comment<'a> {
    pub(crate) const fn new(value: &'a str, magic: bool) -> Self {
        Self { value, magic }
    }

    pub(crate) const fn value(self) -> &'a str {
        self.value
    }

    pub(crate) const fn is_magic(self) -> bool {
        self.magic
    }
}

pub(crate) struct Scalar<'a> {
    value: &'a str,
    tag: Option<&'a Tag>,
    role: Role,
    version: Option<Version>,
    plain_string: OnceCell<bool>,
}

impl<'a> Scalar<'a> {
    pub(crate) const fn new(
        value: &'a str,
        tag: Option<&'a Tag>,
        role: Role,
        version: Option<Version>,
    ) -> Self {
        Self {
            value,
            tag,
            role,
            version,
            plain_string: OnceCell::new(),
        }
    }

    pub(crate) const fn value(&self) -> &'a str {
        self.value
    }

    pub(crate) fn plain_is_string(&self) -> bool {
        *self
            .plain_string
            .get_or_init(|| resolves_to_string_for_version(self.version, self.value))
    }

    pub(crate) fn has_core_tag(&self) -> bool {
        self.tag.is_some_and(is_core_schema)
    }

    pub(crate) const fn is_key(&self) -> bool {
        matches!(self.role, Role::Key)
    }
}

// Unquoted values must stay strings for YAML 1.1 readers as well as the active schema.
pub(crate) fn resolves_to_string_for_version(
    version: Option<Version>,
    value: &str,
) -> bool {
    (resolves_as_yaml_1_1(version)
        || (!is_core_schema_int_spelling(value)
            && matches!(Value::resolve_plain_scalar(value.into()), Value::String(_))))
        && !resolves_to_nonstring_in_yaml_1_1(value)
}
