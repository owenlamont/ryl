use std::ffi::OsStr;
use std::path::{Path, PathBuf};
// Only the LSP's workspace pull needs this; gate it out of `--no-default-features` so
// it is not flagged dead.
#[cfg(feature = "lsp")]
use std::sync::atomic::{AtomicBool, Ordering};

use ignore::{DirEntry, Walk, WalkBuilder};

pub fn is_yaml_path(path: &Path) -> bool {
    path.extension().and_then(OsStr::to_str).is_some_and(|ext| {
        ext.eq_ignore_ascii_case("yml") || ext.eq_ignore_ascii_case("yaml")
    })
}

const VCS_DIRECTORIES: [&str; 4] = [".git", ".hg", ".svn", ".jj"];

/// The directory walk shared by the CLI and the LSP: git ignore/exclude, hidden files
/// included, no symlink follow, and VCS directories pruned. The walker never filters
/// its root, so a VCS directory named as an input is still walked.
#[must_use]
pub fn walk_dir(dir: &Path) -> Walk {
    WalkBuilder::new(dir)
        .hidden(false)
        .ignore(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .follow_links(false)
        .filter_entry(|entry| {
            !(entry.file_type().is_some_and(|t| t.is_dir())
                && VCS_DIRECTORIES.iter().any(|vcs| entry.file_name() == *vcs))
        })
        .build()
}

/// A regular file, or a symlink to one: `file_type` describes the link itself without
/// a `stat`, so only links pay for following.
#[must_use]
pub fn is_walked_file(entry: &DirEntry) -> bool {
    entry
        .file_type()
        .is_some_and(|t| t.is_file() || (t.is_symlink() && entry.path().is_file()))
}

#[must_use]
pub fn gather_yaml_from_dir(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in walk_dir(dir).flatten() {
        if is_yaml_path(entry.path()) && is_walked_file(&entry) {
            files.push(entry.into_path());
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
    for entry in walk_dir(dir).flatten() {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        if is_yaml_path(entry.path()) && is_walked_file(&entry) {
            files.push(entry.into_path());
        }
    }
    Some(files)
}
