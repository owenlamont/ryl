use crate::config::YamlLintConfig;
use crate::rules::flow_collection::{self, FlowCollectionDescriptor};

pub use crate::rules::flow_collection::{Forbid, Violation};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config(flow_collection::Config);

pub const ID: &str = "braces";

const DESCRIPTOR: FlowCollectionDescriptor = FlowCollectionDescriptor {
    open: '{',
    close: '}',
    forbid_message: "forbidden flow mapping",
    min_message: "too few spaces inside braces",
    max_message: "too many spaces inside braces",
    min_empty_message: "too few spaces inside empty braces",
    max_empty_message: "too many spaces inside empty braces",
};

#[must_use]
pub fn check(buffer: &str, cfg: &Config) -> Vec<Violation> {
    flow_collection::check(buffer, cfg.inner(), &DESCRIPTOR)
}

impl Config {
    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        Self(flow_collection::Config::resolve_for(cfg, ID))
    }

    #[must_use]
    pub const fn new_for_tests(
        forbid: Forbid,
        min_spaces_inside: i64,
        max_spaces_inside: i64,
        min_spaces_inside_empty: i64,
        max_spaces_inside_empty: i64,
    ) -> Self {
        Self(flow_collection::Config::new_for_tests(
            forbid,
            min_spaces_inside,
            max_spaces_inside,
            min_spaces_inside_empty,
            max_spaces_inside_empty,
        ))
    }

    #[must_use]
    pub const fn effective_min_empty(&self) -> i64 {
        self.0.effective_min_empty()
    }

    #[must_use]
    pub const fn effective_max_empty(&self) -> i64 {
        self.0.effective_max_empty()
    }

    #[must_use]
    pub const fn forbid(&self) -> Forbid {
        self.0.forbid()
    }

    #[must_use]
    pub const fn min_spaces_inside(&self) -> i64 {
        self.0.min_spaces_inside()
    }

    #[must_use]
    pub const fn max_spaces_inside(&self) -> i64 {
        self.0.max_spaces_inside()
    }

    const fn inner(&self) -> &flow_collection::Config {
        &self.0
    }
}
