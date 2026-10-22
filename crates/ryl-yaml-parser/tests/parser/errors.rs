use std::string::String;
use std::string::ToString;

use ryl_yaml_parser::Marker;
use ryl_yaml_parser::{ErrorKind, InputIoError, ScanError};

#[test]
fn input_error_kinds_construct_info() {
    assert_eq!(
        ErrorKind::InputIo {
            error: InputIoError::from_message("connection reset")
        }
        .to_string(),
        "input I/O error: connection reset"
    );
    assert_eq!(
        ErrorKind::InputDecoding {
            message: String::from("invalid utf-8")
        }
        .to_string(),
        "input decoding error: invalid utf-8"
    );
    assert_eq!(
        ErrorKind::InputByteLimitExceeded { limit: 4096 }.to_string(),
        "input exceeds the configured limit of 4096 bytes"
    );
}

#[test]
fn message_only_input_io_error_has_no_source() {
    use core::error::Error as _;

    let error = InputIoError::from_message("portable failure");

    assert_eq!(error.message(), "portable failure");
    assert!(error.source().is_none());
}

#[test]
fn unique_std_input_io_error_can_be_recovered() {
    use std::io;

    let details = InputIoError::from(io::Error::from_raw_os_error(12_345));
    let error = details
        .try_into_io_error()
        .expect("a uniquely owned io::Error should be recoverable");

    assert_eq!(error.raw_os_error(), Some(12_345));
}

#[test]
fn shared_std_input_io_error_can_be_recovered_after_other_clone_is_dropped() {
    use std::io;

    let details = InputIoError::from(io::Error::from_raw_os_error(12_345));
    let other = details.clone();
    let details = details
        .try_into_io_error()
        .expect_err("a shared io::Error cannot be moved out");

    drop(other);

    let error = details
        .try_into_io_error()
        .expect("the last owner should recover the io::Error");
    assert_eq!(error.raw_os_error(), Some(12_345));
}

#[test]
fn public_constructor_copies_custom_message() {
    let marker = Marker::new(3, 2, 1);
    let mut message = String::from("adapter failed");
    let error = ScanError::new(marker, &message);
    message.clear();

    assert_eq!(
        error.kind(),
        &ErrorKind::Custom(String::from("adapter failed"))
    );
    assert_eq!(error.info(), "adapter failed");
}
