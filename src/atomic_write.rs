use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempPath(PathBuf);

impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn temp_file(path: &Path) -> io::Result<(TempPath, File)> {
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "write target has no file name")
    })?;
    let mut temp_name = OsString::from(".");
    temp_name.push(name);
    temp_name.push(format!(
        ".ryl-{}-{}.tmp",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let temp = path.with_file_name(temp_name);
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    Ok((TempPath(temp), file))
}

/// Write a regular file through a sibling temporary file, preserving permissions when replacing.
///
/// # Errors
/// Returns an error when the target is a symlink or any file operation fails.
pub fn write(path: &Path, bytes: &[u8], replace: bool) -> io::Result<()> {
    write_with_permissions(path, bytes, replace, None)
}

pub(crate) fn copy_new(source: &Path, target: &Path) -> io::Result<()> {
    fs::read(source).and_then(|original| {
        fs::metadata(source).and_then(|metadata| {
            write_with_permissions(
                target,
                &original,
                false,
                Some(metadata.permissions()),
            )
        })
    })
}

fn write_with_permissions(
    path: &Path,
    bytes: &[u8],
    replace: bool,
    permissions: Option<fs::Permissions>,
) -> io::Result<()> {
    let permissions = if replace {
        fs::symlink_metadata(path).and_then(|metadata| {
            if metadata.file_type().is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "refusing to follow a symlink",
                ));
            }
            OpenOptions::new()
                .write(true)
                .open(path)
                .map(|_| Some(metadata.permissions()))
        })
    } else {
        Ok(permissions)
    };
    permissions.and_then(|permissions| write_temp(path, bytes, replace, permissions))
}

fn write_temp(
    path: &Path,
    bytes: &[u8],
    replace: bool,
    permissions: Option<fs::Permissions>,
) -> io::Result<()> {
    let (temp, mut file) = temp_file(path)?;
    let written = permissions
        .map_or(Ok(()), |permissions| file.set_permissions(permissions))
        .and_then(|()| file.write_all(bytes))
        .and_then(|()| file.sync_all());
    drop(file);
    written.and_then(|()| {
        if replace {
            fs::rename(&temp.0, path)
        } else {
            fs::hard_link(&temp.0, path)
        }
    })
}
