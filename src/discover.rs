use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

pub fn is_yaml_path(path: &Path) -> bool {
    matches!(
        path.extension().and_then(OsStr::to_str).map(str::to_ascii_lowercase),
        Some(ref ext) if ext == "yml" || ext == "yaml"
    )
}

#[must_use]
pub fn gather_yaml_from_dir(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let walker = WalkBuilder::new(dir)
        .hidden(false)
        .ignore(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .follow_links(false)
        .build();

    for entry in walker.flatten() {
        let p = entry.path();
        if p.is_file() && is_yaml_path(p) {
            files.push(p.to_path_buf());
        }
    }
    files
}
