use std::fs;

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use tempfile::tempdir;

use crate::common::cli::{run, ryl};
use crate::property::check_strategy::arb_document;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct("tests/proptest-regressions/property_directive_deprecation.txt"))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn aliases_preserve_diagnostics_status_and_cache_behavior(
        document in arb_document(),
        directive in prop::sample::select(vec!["enable", "disable", "disable-line rule:colons"]),
        markdown in any::<bool>(),
        cached in any::<bool>(),
        quiet in any::<bool>(),
    ) {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".ryl.toml"), "[lint.rules]\ncolons = \"enable\"\ntruthy = \"enable\"\n").unwrap();
        let source = format!("# yamllint {directive}\n{}", document.render().replace("\r\n", "\n").replace('\r', "\n"));
        let native = ryl::directives::migrate_legacy_comments(&source).unwrap();
        let wrap = |content: &str| if markdown {
            format!("heading\n\n```yaml\n{content}\n```\n")
        } else { content.to_owned() };
        let path = dir.path().join("a.yaml");
        let invoke = || {
            let mut cmd = ryl(dir.path());
            cmd.current_dir(dir.path())
                .args(["check", "--strict", "--output-format", "gitlab"]);
            if markdown { cmd.arg("--markdown"); }
            if !cached { cmd.arg("--no-cache"); }
            if quiet { cmd.arg("--no-warnings"); }
            run(cmd.arg(&path))
        };
        fs::write(&path, wrap(&native)).unwrap();
        let expected = invoke();
        prop_assert!(!expected.2.contains("yamllint directive aliases are deprecated"));
        fs::write(&path, wrap(&source)).unwrap();
        for _ in 0..2 {
            let actual = invoke();
            prop_assert_eq!((actual.0, actual.1), (expected.0, expected.1.clone()), "source: {:?}", source);
            prop_assert_eq!(actual.2.matches("yamllint directive aliases are deprecated").count(), usize::from(!quiet), "source: {:?}, stderr: {}", source, actual.2);
        }
    }
}
