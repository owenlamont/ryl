#[path = "property_safe_fix/ast.rs"]
pub(super) mod ast;
#[path = "property_check/harness.rs"]
pub(super) mod check_harness;
#[path = "property_check/strategy.rs"]
pub(super) mod check_strategy;
#[path = "property_safe_fix/config.rs"]
pub(super) mod config;
#[path = "property_migrate/strategy.rs"]
pub(super) mod migration_strategy;
#[path = "property_format/passes.rs"]
pub(super) mod passes;
#[path = "property_format/properties.rs"]
pub(super) mod properties;
#[path = "property_format/reference.rs"]
pub(super) mod reference;
#[path = "property_format/representation.rs"]
pub(super) mod representation;
#[path = "property_format/settings.rs"]
pub(super) mod settings;
#[path = "property_fix_convergence/stack.rs"]
pub(super) mod stack;
#[path = "property_safe_fix/strategy.rs"]
pub(super) mod strategy;
#[path = "property_format/suppression.rs"]
pub(super) mod suppression;
#[path = "property_markdown_fix/wrap.rs"]
pub(super) mod wrap;

macro_rules! failure_persistence {
    ($suite:literal) => {
        Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/target/proptest-regressions/",
                $suite,
                ".txt"
            )),
        ))
    };
}

pub(super) use failure_persistence;

pub(super) fn underscored(
    digits: String,
    probability: f64,
) -> impl proptest::strategy::Strategy<Value = String> {
    use proptest::strategy::Strategy;

    proptest::collection::vec(proptest::bool::weighted(probability), digits.len())
        .prop_map(move |marks| {
            digits
                .chars()
                .zip(marks)
                .flat_map(|(digit, mark)| [Some(digit), mark.then_some('_')])
                .flatten()
                .collect()
        })
}
