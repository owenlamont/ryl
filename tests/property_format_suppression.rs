use std::path::Path;

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use ryl::config::YamlLintConfig;
use ryl::format::format_str;

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/proptest-regressions/property_format_suppression.txt",
        ))),
        ..ProptestConfig::default()
    })]
    #[test]
    fn ignored_nodes_and_regions_keep_source_bytes(document in crate::property::suppression::arb_document(), indent_sequences in any::<bool>(), own_line in any::<bool>(), width in 1usize..5) {
        let cfg = YamlLintConfig::from_toml_str(&format!("line-length = 20\nindent-width = {width}\n[format]\ndash-on-own-line = {own_line}\nfold-long-lines = true\nquote-style = 'double'\nsequence-style = 'block'\nsequence-expand = 'auto'\nindent-sequences = {indent_sequences}\n")).unwrap();
        let output = format_str(&document.text, &cfg, Path::new("case.yaml"), &[]);
        prop_assert!(output.contains(&document.protected), "protected {:?} in {:?}", document.protected, output);
        prop_assert!(output.contains("before: 0"));
        prop_assert!(output.contains("after: 1"));
        if document.independent_conversion {
            let conversion = if indent_sequences { &format!("convert:\n{0:1$}- 3\n{0:1$}- 4\n", "", width) } else { "convert:\n- 3\n- 4\n" };
            prop_assert!(output.contains(conversion));
        }
        crate::property::reference::check_strict_events(&document.text, &output).map_err(TestCaseError::fail)?;
        prop_assert_eq!(format_str(&output, &cfg, Path::new("case.yaml"), &[]), output);
    }
}
