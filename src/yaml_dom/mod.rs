//! YAML DOM vendored from saphyr 0.0.6, retargeted at granit-parser and
//! trimmed to the subset ryl actually uses. See module headers for upstream
//! provenance.

mod loader;
mod scalar;
mod yaml_owned;

pub use scalar::{Scalar, ScalarOwned};
pub use yaml_owned::{MappingOwned, SequenceOwned, YamlOwned};
