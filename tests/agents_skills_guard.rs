//! Guards that the `AGENTS.md` "Dev Skills" pointer list stays in lockstep with
//! the on-disk `.agents/skills/` tree: every referenced `SKILL.md` exists with a
//! matching `name:` frontmatter, and every on-disk skill is referenced (no
//! orphan a contributor would never be pointed at). A moved or renamed skill
//! then fails the build instead of silently rotting the pointer.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// Collect the `<name>` of every `.agents/skills/<name>/SKILL.md` mentioned in
/// `AGENTS.md`. Non-`SKILL.md` references under the same prefix (e.g. the
/// `coverage-missing.py` helper or a bare skill directory) are skipped.
fn referenced_skill_names(agents_md: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for (idx, marker) in agents_md.match_indices(".agents/skills/") {
        let after = &agents_md[idx + marker.len()..];
        // The name is the slug run right after the prefix; only count it when it
        // is immediately followed by `/SKILL.md` (so bare-directory references
        // like `.agents/skills/coverage/coverage-missing.py` are skipped).
        let name: String = after
            .chars()
            .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
            .collect();
        if after[name.len()..].starts_with("/SKILL.md") {
            names.insert(name);
        }
    }
    names
}

/// Extract the frontmatter `name:` value from a `SKILL.md` body.
fn frontmatter_name(skill_md: &str) -> String {
    skill_md
        .lines()
        .find_map(|line| line.strip_prefix("name:"))
        .expect("SKILL.md frontmatter must declare a `name:`")
        .trim()
        .to_string()
}

/// Collect the directory name of every skill under `.agents/skills/`, asserting
/// each carries a `SKILL.md` whose `name:` matches its directory.
fn on_disk_skill_names(skills_dir: &Path) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for entry in fs::read_dir(skills_dir).expect("`.agents/skills/` should exist") {
        let entry = entry.expect("a readable directory entry");
        let dir_name = entry
            .file_name()
            .into_string()
            .expect("skill directory names should be valid UTF-8");
        let body = fs::read_to_string(entry.path().join("SKILL.md"))
            .expect("each skill directory should contain a SKILL.md");
        assert_eq!(
            frontmatter_name(&body),
            dir_name,
            "a SKILL.md `name:` must match its directory name",
        );
        names.insert(dir_name);
    }
    names
}

#[test]
fn dev_skill_pointers_match_on_disk() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let agents_md =
        fs::read_to_string(root.join("AGENTS.md")).expect("AGENTS.md should exist");
    let skills_dir = root.join(".agents").join("skills");

    assert_eq!(
        referenced_skill_names(&agents_md),
        on_disk_skill_names(&skills_dir),
        "AGENTS.md Dev Skills pointers must match `.agents/skills/` exactly \
         (every skill referenced, no orphans)",
    );
}
