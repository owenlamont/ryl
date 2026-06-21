use std::ffi::OsStr;
use std::path::{Path, PathBuf};
// Only the LSP's workspace pull needs this; gate it out of `--no-default-features` so
// it is not flagged dead.
#[cfg(feature = "lsp")]
use std::sync::atomic::{AtomicBool, Ordering};

use ignore::{Walk, WalkBuilder};

pub fn is_yaml_path(path: &Path) -> bool {
    path.extension().and_then(OsStr::to_str).is_some_and(|ext| {
        ext.eq_ignore_ascii_case("yml") || ext.eq_ignore_ascii_case("yaml")
    })
}

/// Shared walker (git ignore/exclude, includes hidden files, no symlink follow) so the
/// plain and cancellable gatherers cannot drift apart.
fn yaml_walker(dir: &Path) -> Walk {
    WalkBuilder::new(dir)
        .hidden(false)
        .ignore(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .follow_links(false)
        .build()
}

#[must_use]
pub fn gather_yaml_from_dir(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in yaml_walker(dir).flatten() {
        let p = entry.path();
        if p.is_file() && is_yaml_path(p) {
            files.push(p.to_path_buf());
        }
    }
    files
}

/// Like [`gather_yaml_from_dir`] but returns `None` as soon as `cancel` is set (checked
/// once per entry), so a cancelled `workspace/diagnostic` stops enumerating a huge tree.
#[cfg(feature = "lsp")]
pub(crate) fn gather_yaml_from_dir_cancellable(
    dir: &Path,
    cancel: &AtomicBool,
) -> Option<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in yaml_walker(dir).flatten() {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let p = entry.path();
        if p.is_file() && is_yaml_path(p) {
            files.push(p.to_path_buf());
        }
    }
    Some(files)
}
