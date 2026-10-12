use super::*;
use std::cell::Cell;
use std::io::{self, Cursor};
use std::rc::Rc;

#[test]
fn declared_oversize_is_rejected_without_a_read() {
    struct NeverRead;
    impl Read for NeverRead {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            panic!("declared size must be checked before reading")
        }
    }
    assert!(read_manifest(NeverRead, super::super::MAX_MANIFEST_BYTES as u64 + 1).is_err());
    assert!(read_manifest(NeverRead, u64::MAX).is_err());
}

#[test]
fn exact_manifest_limit_is_inclusive() {
    let limit = super::super::MAX_MANIFEST_BYTES;
    let data = read_manifest(io::repeat(0).take(limit as u64), limit as u64).unwrap();
    assert_eq!(data.len(), limit);
    assert!(data.capacity() <= limit);
}

#[test]
fn understated_size_cannot_read_beyond_one_sentinel_byte() {
    struct CountReads(Rc<Cell<usize>>);
    impl Read for CountReads {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            buf.fill(0);
            self.0.set(self.0.get() + buf.len());
            Ok(buf.len())
        }
    }
    let actual = Rc::new(Cell::new(0));
    let error = read_manifest(CountReads(actual.clone()), 0).unwrap_err();
    assert!(matches!(error, CoreError::Apk(_)));
    assert_eq!(actual.get(), super::super::MAX_MANIFEST_BYTES + 1);
}

#[test]
fn interrupted_reads_retry_like_read_to_end() {
    struct InterruptedOnce {
        interrupted: bool,
        content: Cursor<&'static [u8]>,
    }
    impl Read for InterruptedOnce {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if !self.interrupted {
                self.interrupted = true;
                return Err(io::Error::from(io::ErrorKind::Interrupted));
            }
            self.content.read(buf)
        }
    }
    let data = read_manifest(
        InterruptedOnce {
            interrupted: false,
            content: Cursor::new(b"abc"),
        },
        3,
    )
    .unwrap();
    assert_eq!(data, b"abc");
}

#[test]
fn a_real_read_error_is_not_returned_as_partial_success() {
    struct Failed;
    impl Read for Failed {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::from(io::ErrorKind::InvalidData))
        }
    }
    assert!(matches!(read_manifest(Failed, 3), Err(CoreError::Apk(_))));
}
