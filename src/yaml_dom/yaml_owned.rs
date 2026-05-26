// Vendored from saphyr v0.0.6 (`saphyr/src/yaml_owned.rs` + the `YamlOwned`
// halves of `saphyr/src/macros.rs`). Trimmed to the inspector methods ryl
// actually uses and re-targeted at indexmap::IndexMap. The original is
// dual-licensed MIT OR Apache-2.0; ryl ships under MIT.

use std::hash::{Hash, Hasher};

use granit_parser::Tag;
use indexmap::IndexMap;

use crate::yaml_dom::loader::load_owned_documents;
use crate::yaml_dom::scalar::ScalarOwned;

pub type SequenceOwned = Vec<YamlOwned>;
pub type MappingOwned = IndexMap<YamlOwned, YamlOwned>;

#[derive(Clone, PartialEq, Debug, Eq)]
pub enum YamlOwned {
    Value(ScalarOwned),
    Sequence(SequenceOwned),
    Mapping(MappingOwned),
    Tagged(Tag, Box<YamlOwned>),
    BadValue,
}

impl YamlOwned {
    /// Parse `source` into a sequence of documents.
    ///
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

impl Hash for YamlOwned {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Self::Value(scalar) => scalar.hash(state),
            Self::Sequence(seq) => seq.hash(state),
            Self::Mapping(map) => {
                for entry in map {
                    entry.hash(state);
                }
            }
            Self::Tagged(tag, node) => {
                tag.hash(state);
                node.hash(state);
            }
            Self::BadValue => {}
        }
    }
}
