//! Test the feature that most operations (like `read_exact`) are not interruptible by a reopen.

use std::io::{Error, ErrorKind, Read};
use std::iter;

use partial_io::{PartialOp, PartialRead};
use reopen::{Handle, Reopen};

/// Request reopen after each operation. That way we can check the reopen doesn't happen in the
/// middle of something.
struct RequestReopen<FD> {
    handle: Handle,
    fd: FD,
}

impl<FD: Read> Read for RequestReopen<FD> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        let result = self.fd.read(buf);
        self.handle.reopen();
        result
    }
}

// Get a reader that has bunch of data in it, but chunks it by a single byte.
fn provide_reader() -> Reopen<RequestReopen<PartialRead<&'static [u8]>>> {
    let handle = Handle::stub();
    Reopen::with_handle(
        handle.clone(),
        Box::new(move || {
            let data = b"hello" as &[u8];
            let partial = PartialRead::new(data, iter::repeat(PartialOp::Limited(1)));
            Ok(RequestReopen {
                fd: partial,
                handle: handle.clone(),
            })
        }),
    )
    .unwrap()
}

/// Check that the convoluted reader chunks by single bytes and resets it after each use.
///
/// This doesn't check reopen as much as the test infrastructure itself.
#[test]
fn read_sanity_check() {
    let mut reader = provide_reader();
    let mut buf = [0; 10];
    assert_eq!(1, reader.read(&mut buf).unwrap());
    assert_eq!(b'h', buf[0]);
    // After reopening, it provides 'h' again.
    assert_eq!(1, reader.read(&mut buf).unwrap());
    assert_eq!(b'h', buf[0]);
}

/// Test explicit locking of multiple operations.
#[test]
fn read_explicit() {
    let mut reader = provide_reader();
    let lock = reader.lock().unwrap();
    let mut buf = [0; 10];
    // Will read one byte at a time, without reopening
    assert_eq!(1, lock.read(&mut buf).unwrap());
    assert_eq!(b'h', buf[0]);
    assert_eq!(1, lock.read(&mut buf).unwrap());
    assert_eq!(b'e', buf[0]);

    // Will reopen now
    assert_eq!(1, reader.read(&mut buf).unwrap());
    assert_eq!(b'h', buf[0]);
}

/// Check that read_exact isn't interrupted.
#[test]
fn read_exact() {
    let mut reader = provide_reader();
    let mut buf = [0; 3];
    // Doesn't get interrupted in the middle of read_exact
    reader.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"hel");
    // But reopens afterwards, as requested from the inside.
    assert_eq!(1, reader.read(&mut buf).unwrap());
    assert_eq!(b'h', buf[0]);
}

/// Test EOF is propagated correctly.
///
/// And that after the last use it can get reopened again.
#[test]
fn read_exact_eof() {
    let mut reader = provide_reader();
    let mut buf = [0; 10];
    // Tries to read 10 bytes, but there isn't enough.
    let err = reader.read_exact(&mut buf).unwrap_err();
    assert_eq!(ErrorKind::UnexpectedEof, err.kind());
    // After reopen, starts from the beginning.
    assert_eq!(1, reader.read(&mut buf).unwrap());
    assert_eq!(b'h', buf[0]);
}

/// Test we can read to the end of the current reader. And then reopen and do it again.
#[test]
fn read_to_end() {
    let mut reader = provide_reader();
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).unwrap();
    assert_eq!(b"hello", &buf[..]);
    // Reopens after the use, we can read another input.
    reader.read_to_end(&mut buf).unwrap();
    assert_eq!(b"hellohello", &buf[..]);
}
