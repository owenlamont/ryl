use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

#[test]
fn every_suite_is_registered() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for (directory, standalone) in [
        ("tests", ""),
        ("crates/ryl-yaml-parser/tests", "yaml-test-suite"),
    ] {
        let directory = root.join(directory);
        // The packaged ryl crate omits the parser's test sources.
        if !directory.is_dir() {
            continue;
        }
        let suites: BTreeSet<_> = fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
            .map(|path| path.file_stem().unwrap().to_str().unwrap().to_owned())
            .filter(|name| name != "integration" && name != standalone)
            .collect();
        let registered: BTreeSet<_> =
            fs::read_to_string(directory.join("integration.rs"))
                .unwrap()
                .lines()
                .filter_map(|line| line.strip_prefix("mod ")?.strip_suffix(';'))
                .filter(|name| directory.join(format!("{name}.rs")).is_file())
                .map(str::to_owned)
                .collect();
        assert_eq!(suites, registered, "{}", directory.display());
    }
}
