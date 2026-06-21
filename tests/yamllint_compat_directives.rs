//! Differential parity for inline `# yamllint …` directives: ryl must drop exactly the
//! diagnostics real yamllint drops across the block / disable-line / `rule:` forms, for
//! every output format. Uses the `# yamllint` spelling (the compat alias) since that is
//! what yamllint reads; the preferred `# ryl` spelling is covered in `directives.rs`.

use std::fs;

use tempfile::tempdir;

#[path = "common/compat.rs"]
mod compat;

use compat::{
    SCENARIOS, build_ryl_command, build_yamllint_command, capture_with_env,
    ensure_yamllint_installed,
};

const CONFIG: &str = "rules:\n  \
    document-start: disable\n  \
    new-line-at-end-of-file: disable\n  \
    colons: enable\n  \
    commas: enable\n  \
    truthy: enable\n  \
    braces: enable\n  \
    key-duplicates: enable\n  \
    comments: enable\n";

/// `(name, bytes)`, written verbatim so CRLF survives.
const CASES: &[(&str, &str)] = &[
    (
        "inline.yaml",
        "a:  yes  # yamllint disable-line rule:colons\n",
    ),
    (
        "inline-self.yaml",
        "a:  yes  # yamllint disable-line rule:truthy\n",
    ),
    (
        "own-line.yaml",
        "# yamllint disable-line rule:colons\na:  1\nb:  2\n",
    ),
    (
        "block.yaml",
        "# yamllint disable rule:truthy\na: yes\nb: on\n# yamllint enable rule:truthy\nc: yes\n",
    ),
    (
        "bare-block.yaml",
        "# yamllint disable\na:  yes\n# yamllint enable\nb:  on\n",
    ),
    (
        "disable-then-enable.yaml",
        "# yamllint disable\n# yamllint enable rule:colons\na:  yes\n",
    ),
    (
        "multiple.yaml",
        "a:  yes  # yamllint disable-line rule:colons rule:truthy\n",
    ),
    (
        "rejected-extra-spaces.yaml",
        "a:  yes  #   yamllint disable-line rule:colons\n",
    ),
    (
        "rejected-no-prefix.yaml",
        "a:  yes  # yamllint disable-line colons\n",
    ),
    (
        "crlf.yaml",
        "# yamllint disable-line rule:colons\r\na:  1\r\nb:  2\r\n",
    ),
    (
        "disable-file.yaml",
        "# yamllint disable-file\na:  yes\nb: [1\n",
    ),
    (
        "disable-file-lenient.yaml",
        "#yamllint disable-file\na:  yes\n",
    ),
    (
        "disable-file-not-first.yaml",
        "a:  yes\n# yamllint disable-file\n",
    ),
];

#[test]
fn directives_match_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();
    let config = dir.path().join("config.yml");
    fs::write(&config, CONFIG).unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");

    for scenario in SCENARIOS {
        for (name, body) in CASES {
            let file = dir.path().join(name);
            fs::write(&file, body).unwrap();

            let mut ryl = build_ryl_command(exe, scenario.ryl_format);
            ryl.arg("-c").arg(&config).arg(&file);
            let (ryl_code, ryl_out) = capture_with_env(ryl, scenario.envs);

            let mut yam = build_yamllint_command(scenario.yam_format);
            yam.arg("-c").arg(&config).arg(&file);
            let (yam_code, yam_out) = capture_with_env(yam, scenario.envs);

            assert_eq!(
                ryl_code, yam_code,
                "exit mismatch for {name} ({})",
                scenario.label
            );
            assert_eq!(
                ryl_out, yam_out,
                "diagnostic mismatch for {name} ({})",
                scenario.label
            );
        }
    }
}
