use std::cell::RefCell;

thread_local! {
    static FILE_STDERR: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
}

struct StderrScope {
    previous: Option<Vec<u8>>,
}

impl Drop for StderrScope {
    fn drop(&mut self) {
        FILE_STDERR.with_borrow_mut(|buffer| *buffer = self.previous.take());
    }
}

pub(crate) fn capture_file_stderr<T>(f: impl FnOnce() -> T) -> (T, Vec<u8>) {
    let _scope = StderrScope {
        previous: FILE_STDERR.with_borrow_mut(|buffer| buffer.replace(Vec::new())),
    };
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
