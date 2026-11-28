use std::cell::RefCell;
use std::mem::DropGuard;

thread_local! {
    static FILE_STDERR: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
}

pub(crate) fn capture_file_stderr<T>(f: impl FnOnce() -> T) -> (T, Vec<u8>) {
    let previous = FILE_STDERR.with_borrow_mut(|buffer| buffer.replace(Vec::new()));
    let _scope = DropGuard::new(previous, |previous| {
        FILE_STDERR.with_borrow_mut(|buffer| *buffer = previous);
    });
    let result = f();
    let stderr = FILE_STDERR
        .with_borrow_mut(Option::take)
        .unwrap_or_default();
    (result, stderr)
}

pub(crate) fn write_file_stderr(writer: &mut dyn std::io::Write, bytes: &[u8]) {
    FILE_STDERR.with_borrow_mut(|buffer| {
        if let Some(captured) = buffer {
            captured.extend_from_slice(bytes);
        } else {
            let _ = writer.write_all(bytes);
        }
    });
}
