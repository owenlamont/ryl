//! YAML DOM vendored from saphyr 0.0.6, retargeted at granit-parser and trimmed to
//! the subset ryl uses. Per-module headers carry the upstream provenance.

mod loader;
mod scalar;
mod tag;
mod yaml_owned;

pub use scalar::{Scalar, ScalarOwned};
pub use tag::{core_schema_suffix, is_core_schema};
pub use yaml_owned::{MappingOwned, SequenceOwned, YamlOwned};
