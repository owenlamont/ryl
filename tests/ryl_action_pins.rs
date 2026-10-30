//! Every `owenlamont/ryl-action@vX.Y.Z` example in the README, the docs and the agent
//! skill pins the release being built, since the action's tags mirror ryl releases.

use std::fs;
use std::path::{Path, PathBuf};

const PIN: &str = "owenlamont/ryl-action@v";

fn markdown_and_text(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            markdown_and_text(&path, found);
        } else if path
            .extension()
            .is_some_and(|ext| ext == "md" || ext == "txt")
        {
            found.push(path);
        }
    }
}

#[test]
fn ryl_action_pins_match_the_crate_version() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    // docs/ is not packaged in the published crate.
    if !root.join("docs").is_dir() {
        return;
    }
    let mut files = vec![root.join("README.md"), root.join("skills/ryl/SKILL.md")];
    markdown_and_text(&root.join("docs"), &mut files);
    let mut pins = 0;
    for file in files {
        let text = fs::read_to_string(&file).unwrap();
        for (at, _) in text.match_indices(PIN) {
            let version: String = text[at + PIN.len()..]
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            assert_eq!(
                version.trim_end_matches('.'),
                env!("CARGO_PKG_VERSION"),
                "{} pins ryl-action to v{version}",
                file.display()
            );
            pins += 1;
        }
    }
    assert!(pins > 0, "no ryl-action pin found to check");
}
