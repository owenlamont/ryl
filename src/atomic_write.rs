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

pub(crate) fn write(path: &Path, bytes: &[u8], replace: bool) -> io::Result<()> {
    write_with_metadata(path, bytes, replace, None)
}

pub(crate) fn copy_new(source: &Path, target: &Path) -> io::Result<()> {
    fs::read(source).and_then(|original| {
        fs::metadata(source).and_then(|metadata| {
            write_with_metadata(target, &original, false, Some(metadata))
        })
    })
}

fn write_with_metadata(
    path: &Path,
    bytes: &[u8],
    replace: bool,
    metadata: Option<fs::Metadata>,
) -> io::Result<()> {
    let metadata = if replace {
        fs::symlink_metadata(path).and_then(|source_metadata| {
            if source_metadata.file_type().is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "refusing to follow a symlink",
                ));
            }
            OpenOptions::new()
                .write(true)
                .open(path)
                .map(|_| Some(source_metadata))
        })
    } else {
        Ok(metadata)
    };
    metadata.and_then(|metadata| write_temp(path, bytes, replace, metadata))
}

fn write_temp(
    path: &Path,
    bytes: &[u8],
    replace: bool,
    metadata: Option<fs::Metadata>,
) -> io::Result<()> {
    let (temp, mut file) = temp_file(path)?;
    let written = metadata
        .map_or(Ok(()), |metadata| preserve_metadata(&file, &metadata))
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

fn preserve_metadata(file: &File, metadata: &fs::Metadata) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, fchown};

        fchown(file, Some(metadata.uid()), Some(metadata.gid())).map_err(|err| {
            io::Error::new(
                err.kind(),
                format!("cannot preserve config owner and group: {err}"),
            )
        })?;
    }
    file.set_permissions(metadata.permissions())
}
