// Vendored from saphyr v0.0.6 (`saphyr/src/yaml_owned.rs` + the `YamlOwned` halves of
// `saphyr/src/macros.rs`), trimmed to the inspector methods ryl uses. Saphyr is MIT
// OR Apache-2.0; ryl ships under MIT.

use granit_parser::Tag;
use hashlink::LinkedHashMap;

use crate::yaml_dom::loader::load_owned_documents;
use crate::yaml_dom::scalar::ScalarOwned;

pub type SequenceOwned = Vec<YamlOwned>;
// `LinkedHashMap` keeps saphyr's insertion-order-sensitive `PartialEq`/`Hash`, so the
// derived `Hash` stays consistent with equality when a mapping is a complex key.
pub type MappingOwned = LinkedHashMap<YamlOwned, YamlOwned>;

#[derive(Clone, PartialEq, Debug, Eq, Hash)]
pub enum YamlOwned {
    Value(ScalarOwned),
    Sequence(SequenceOwned),
    Mapping(MappingOwned),
    Tagged(Tag, Box<YamlOwned>),
    BadValue,
}

impl YamlOwned {
    /// # Errors
    /// Returns [`granit_parser::ScanError`] when the parser rejects the input.
    pub fn load_from_str(source: &str) -> Result<Vec<Self>, granit_parser::ScanError> {
        load_owned_documents(source)
    }

    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Value(ScalarOwned::Boolean(b)) => Some(*b),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Self::Value(ScalarOwned::Integer(i)) => Some(*i),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_floating_point(&self) -> Option<f64> {
        match self {
            Self::Value(ScalarOwned::FloatingPoint(f)) => Some(f.into_inner()),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Value(ScalarOwned::String(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    #[must_use]
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Value(ScalarOwned::Null))
    }

    #[must_use]
    pub fn as_mapping(&self) -> Option<&MappingOwned> {
        match self {
            Self::Mapping(m) => Some(m),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_mapping_mut(&mut self) -> Option<&mut MappingOwned> {
        match self {
            Self::Mapping(m) => Some(m),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_sequence(&self) -> Option<&SequenceOwned> {
        match self {
            Self::Sequence(s) => Some(s),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_mapping_get(&self, key: &str) -> Option<&YamlOwned> {
        self.as_mapping().and_then(|map| map.get(&string_key(key)))
    }

    #[must_use]
    pub fn as_mapping_get_mut(&mut self, key: &str) -> Option<&mut YamlOwned> {
        self.as_mapping_mut()
            .and_then(|map| map.get_mut(&string_key(key)))
    }
}

fn string_key(key: &str) -> YamlOwned {
    YamlOwned::Value(ScalarOwned::String(key.to_owned()))
}
