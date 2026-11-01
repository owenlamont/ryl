//! YAML DOM vendored from saphyr 0.0.6, retargeted at ryl-yaml-parser and trimmed to
//! the subset ryl uses. Per-module headers carry the upstream provenance.

mod loader;
mod scalar;
mod tag;
mod yaml_owned;

pub use scalar::{Scalar, ScalarOwned, is_core_schema_int_spelling};
pub(crate) use scalar::{
    canonical_core_schema_int, core_schema_int, is_core_schema_null,
    parse_core_schema_bool, parse_core_schema_fp,
};
pub use tag::{core_schema_suffix, is_core_schema};
pub use yaml_owned::{MappingOwned, SequenceOwned, YamlOwned};
