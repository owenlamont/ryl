mod stderr {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/cli_support/stderr.rs"
    ));
}

use stderr::{capture_file_stderr, write_file_stderr};

#[test]
fn nested_file_capture_restores_the_enclosing_buffer() {
    let mut direct = Vec::new();
    let ((result, inner), outer) = capture_file_stderr(|| {
        write_file_stderr(&mut direct, "before café\n".as_bytes());
        let inner = capture_file_stderr(|| {
            write_file_stderr(&mut direct, b"child\n");
            7
        });
        write_file_stderr(&mut direct, b"after\n");
        inner
    });
    assert_eq!(result, 7);
    assert_eq!(inner.as_slice(), b"child\n");
    assert_eq!(outer, "before café\nafter\n".as_bytes());
    assert!(direct.is_empty());
}

#[test]
fn panicking_nested_capture_restores_the_enclosing_buffer() {
    let mut direct = Vec::new();
    let ((), outer) = capture_file_stderr(|| {
        write_file_stderr(&mut direct, b"before\n");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            capture_file_stderr(|| {
                write_file_stderr(&mut direct, b"child\n");
                panic!("nested capture failed");
            });
        }));
        assert!(result.is_err());
        write_file_stderr(&mut direct, b"after\n");
    });
    assert_eq!(outer.as_slice(), b"before\nafter\n");
    assert!(direct.is_empty());
}

#[test]
fn writing_after_capture_reaches_the_supplied_writer() {
    let mut direct = Vec::new();
    assert!(capture_file_stderr(|| ()).1.is_empty());
    write_file_stderr(&mut direct, b"direct\n");
    assert_eq!(direct.as_slice(), b"direct\n");
}
