//! Guards the published `skills/ryl/SKILL.md` against CLI drift: every long flag
//! (`--name`) the skill teaches must still exist in `ryl --help`, so a renamed or
//! removed flag fails the build instead of silently rotting the agent skill.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

/// Collect every `--long-flag` token in the text.
///
/// `--` is only treated as a flag start at a word boundary, so a `foo--bar` inside a
/// URL or word is ignored. Applied to both the skill and `ryl --help`, so the guard
/// compares whole flags (set membership), not substrings.
fn long_flags(text: &str) -> BTreeSet<String> {
    let bytes = text.as_bytes();
    let mut flags = BTreeSet::new();
    for (idx, marker) in text.match_indices("--") {
        if idx > 0 && bytes[idx - 1].is_ascii_alphanumeric() {
            continue;
        }
        let name: String = text[idx + marker.len()..]
            .chars()
            .take_while(|c| c.is_ascii_lowercase() || *c == '-')
            .collect();
        if name.len() >= 2 && !name.starts_with('-') && !name.ends_with('-') {
            flags.insert(format!("--{name}"));
        }
    }
    flags
}

#[test]
fn skill_flags_still_exist_in_help() {
    let skill = Path::new(env!("CARGO_MANIFEST_DIR")).join("skills/ryl/SKILL.md");
    // skills/ is not packaged in the published crate (Cargo.toml `include`), so skip
    // when the skill is absent (e.g. `cargo test` against a packaged crate).
    if !skill.is_file() {
        return;
    }

    let text = std::fs::read_to_string(&skill).expect("SKILL.md should be readable");
    let help = Command::new(env!("CARGO_BIN_EXE_ryl"))
        .arg("--help")
        .output()
        .expect("`ryl --help` should run");
    let help = String::from_utf8(help.stdout).expect("--help output should be UTF-8");

    let skill_flags = long_flags(&text);
    let help_flags = long_flags(&help);
    let missing: Vec<&String> = skill_flags.difference(&help_flags).collect();
    assert!(
        missing.is_empty(),
        "SKILL.md names long flags absent from `ryl --help` (renamed/removed?): {missing:?}",
    );
}
