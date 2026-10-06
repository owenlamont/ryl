//! `colons` rule: limit spaces around `:` (and explicit `?`) in mappings.
//!
//! An alias, anchor or tag key keeps one space before `:`, which is never reported: `:` is
//! a legal anchor-name and tag char (YAML 1.2.2 §6.9.2), so `*foo:` aliases an anchor named
//! `foo:`. yamllint exempts only the alias (adrienverge/yamllint#226).
//!
//! Safe `--fix` collapses each run to the tolerance, never below that one space or the one
//! after an indicator. A `?` or `:` opening a compact block collection that continues
//! below is left alone: its spacing is the collection's indentation.
use crate::config::YamlLintConfig;
use crate::rules::support::token_spacing::{self, Fix, Indicator, Mode, Site};

pub const ID: &str = "colons";
const TOO_MANY_BEFORE: &str = "too many spaces before colon";
const TOO_MANY_AFTER: &str = "too many spaces after colon";
const TOO_MANY_AFTER_QUESTION: &str = "too many spaces after question mark";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    max_spaces_before: i64,
    max_spaces_after: i64,
    mode: Mode,
}

impl Config {
    const DEFAULT_MAX_BEFORE: i64 = 0;
    const DEFAULT_MAX_AFTER: i64 = 1;

    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        Self::new(
            cfg.rule_option_int(ID, "max-spaces-before", Self::DEFAULT_MAX_BEFORE),
            cfg.rule_option_int(ID, "max-spaces-after", Self::DEFAULT_MAX_AFTER),
        )
    }

    #[must_use]
    pub const fn new(max_spaces_before: i64, max_spaces_after: i64) -> Self {
        Self {
            max_spaces_before,
            max_spaces_after,
            mode: Mode::Lint,
        }
    }

    /// The formatter's target: no space before `:`, exactly one after `:` and `?`.
    #[must_use]
    pub const fn format() -> Self {
        Self {
            mode: Mode::Format,
            ..Self::new(0, 1)
        }
    }

    #[must_use]
    pub const fn max_spaces_before(&self) -> i64 {
        self.max_spaces_before
    }

    #[must_use]
    pub const fn max_spaces_after(&self) -> i64 {
        self.max_spaces_after
    }

    fn tolerance(&self, site: &Site) -> Option<i64> {
        match site.indicator {
            Indicator::Colon if site.before => Some(self.max_spaces_before),
            Indicator::Colon | Indicator::Question => Some(self.max_spaces_after),
            Indicator::Dash => None,
        }
    }

    fn violations(&self, buffer: &str, keep: impl Fn(Fix) -> bool) -> Vec<Violation> {
        token_spacing::sites(buffer)
            .into_iter()
            .filter(|site| {
                keep(site.fix)
                    && self.tolerance(site).is_some_and(|tol| site.exceeds(tol))
            })
            .map(|site| Violation {
                line: site.line,
                column: site.column,
                message: match (site.indicator, site.before) {
                    (Indicator::Question, _) => TOO_MANY_AFTER_QUESTION,
                    (_, true) => TOO_MANY_BEFORE,
                    (_, false) => TOO_MANY_AFTER,
                }
                .to_string(),
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

/// The violations under `cfg`; for the formatter's config, only those its fix rewrites.
#[must_use]
pub fn check(buffer: &str, cfg: &Config) -> Vec<Violation> {
    cfg.violations(buffer, |fix| cfg.mode == Mode::Lint || fix == Fix::Safe)
}

#[must_use]
pub fn fix(buffer: &str, cfg: &Config) -> Option<String> {
    token_spacing::fix(buffer, cfg.mode, |site| cfg.tolerance(site))
}

/// The violations `fix` leaves because respacing them would re-indent a collection.
#[must_use]
pub fn unfixed(buffer: &str, cfg: &Config) -> Vec<Violation> {
    cfg.violations(buffer, |fix| fix == Fix::Reindents)
}
