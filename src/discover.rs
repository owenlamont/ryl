use std::ffi::OsStr;
use std::path::{Path, PathBuf};
// Only the LSP's workspace pull needs the cancellable walk; keep it (and its atomics
// import) out of a `--no-default-features` build so it is not flagged as dead code.
#[cfg(feature = "lsp")]
use std::sync::atomic::{AtomicBool, Ordering};

use ignore::{Walk, WalkBuilder};

pub fn is_yaml_path(path: &Path) -> bool {
    path.extension().and_then(OsStr::to_str).is_some_and(|ext| {
        ext.eq_ignore_ascii_case("yml") || ext.eq_ignore_ascii_case("yaml")
    })
}

/// The shared directory walker: honours git ignore/exclude, includes hidden files, and
/// does not follow symlinks. Used by both the plain and cancellable YAML gatherers so the
/// traversal rules cannot drift between them.
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

/// Like [`gather_yaml_from_dir`] but abandons the walk and returns `None` as soon as
/// `cancel` is set (checked once per entry), so a cancelled `workspace/diagnostic` stops
/// enumerating a (possibly huge or slow) tree instead of finishing the whole walk.
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
