#[path = "../tests/property_safe_fix/ast.rs"]
mod ast;
#[path = "../tests/property_format/properties.rs"]
mod properties;
#[path = "../tests/property_fix_convergence/stack.rs"]
mod stack;
#[path = "../tests/property_safe_fix/strategy.rs"]
mod strategy;

use std::env;
use std::fs;

use proptest::strategy::{Strategy, ValueTree};
use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};

#[test]
#[ignore = "writes benchmark candidates when BENCH_CORPUS_DIR is set"]
fn write_benchmark_candidates() {
    let directory = env::var("BENCH_CORPUS_DIR").expect("BENCH_CORPUS_DIR is required");
    let seed = env::var("BENCH_CORPUS_SEED")
        .expect("BENCH_CORPUS_SEED is required")
        .parse::<u64>()
        .expect("seed must be a u64");
    let rng = TestRng::from_seed(RngAlgorithm::ChaCha, &seed.to_le_bytes().repeat(4));
    let mut runner = TestRunner::new_with_rng(
        Config {
            failure_persistence: None,
            ..Config::default()
        },
        rng,
    );
    fs::create_dir_all(&directory).unwrap();
    let decorated = properties::arb_document_with_properties();
    let plain = strategy::arb_document();
    for index in 0..1024 {
        let sample = if index % 2 == 0 {
            let mut sample = decorated.new_tree(&mut runner).unwrap().current();
            sample.document.version_directive = None;
            sample.start_marker = false;
            sample.end_marker = false;
            sample.follow_ons.clear();
            sample.render()
        } else {
            let mut sample = plain.new_tree(&mut runner).unwrap().current();
            sample.version_directive = None;
            sample.render()
        };
        let path = format!("{directory}/{index:04}.yaml");
        fs::write(path, sample).unwrap();
    }
}
