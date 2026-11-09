use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

pub type Entries = Box<dyn Iterator<Item = io::Result<PathBuf>> + Send>;

pub trait FileSystem: Send + Sync {
    /// # Errors
    /// Returns filesystem metadata errors without following a final symlink.
    fn metadata(&self, path: &Path) -> io::Result<std::fs::Metadata>;
    /// # Errors
    /// Returns cache read/open failures.
    fn open(&self, path: &Path) -> io::Result<Box<dyn Read + Send>>;
    /// # Errors
    /// Returns lock-file open failures.
    fn open_lock(&self, path: &Path) -> io::Result<std::fs::File> {
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
    }
    /// # Errors
    /// Returns directory creation failures.
    fn create_dirs(&self, path: &Path) -> io::Result<()>;
    /// # Errors
    /// Returns directory listing failures, including per-entry errors.
    fn entries(&self, path: &Path) -> io::Result<Entries>;
    /// # Errors
    /// Returns an error rather than overwriting an existing file.
    fn create_new(&self, path: &Path) -> io::Result<Box<dyn Write + Send>>;
    /// # Errors
    /// Returns rename failures.
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    /// # Errors
    /// Returns removal failures.
    fn remove(&self, path: &Path) -> io::Result<()>;
}

pub struct SystemFileSystem;
impl FileSystem for SystemFileSystem {
    fn metadata(&self, path: &Path) -> io::Result<std::fs::Metadata> {
        path.symlink_metadata()
    }
    fn open(&self, path: &Path) -> io::Result<Box<dyn Read + Send>> {
        std::fs::File::open(path).map(|file| Box::new(file) as Box<dyn Read + Send>)
    }
    fn create_dirs(&self, path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }
    fn entries(&self, path: &Path) -> io::Result<Entries> {
        std::fs::read_dir(path).map(|entries| {
            Box::new(entries.map(|entry| entry.map(|entry| entry.path()))) as Entries
        })
    }
    fn create_new(&self, path: &Path) -> io::Result<Box<dyn Write + Send>> {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map(|file| Box::new(file) as Box<dyn Write + Send>)
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        std::fs::rename(from, to)
    }
    fn remove(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }
}
