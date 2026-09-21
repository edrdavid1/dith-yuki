//! Reader that counts actually-read bytes against a budget.

use crate::io::IoError;
use std::io::{self, Read};

/// Wraps a [`Read`] and fails once `limit` bytes have been delivered.
pub struct LimitedReader<R> {
    inner: R,
    remaining: u64,
    read_total: u64,
}

impl<R> LimitedReader<R> {
    pub fn new(inner: R, limit: u64) -> Self {
        Self {
            inner,
            remaining: limit,
            read_total: 0,
        }
    }

    pub fn bytes_read(&self) -> u64 {
        self.read_total
    }
}

impl<R: Read> Read for LimitedReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "entry byte budget exhausted",
            ));
        }
        let max = (self.remaining as usize).min(buf.len());
        let n = self.inner.read(&mut buf[..max])?;
        self.remaining = self.remaining.saturating_sub(n as u64);
        self.read_total = self.read_total.saturating_add(n as u64);
        Ok(n)
    }
}

/// Map [`io::Error`] from budget exhaustion into [`IoError`].
pub fn map_limit_err(err: io::Error) -> IoError {
    if err.kind() == io::ErrorKind::InvalidData {
        IoError::Io(err)
    } else {
        IoError::Io(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn stops_at_budget() {
        let data = vec![1u8; 100];
        let mut r = LimitedReader::new(Cursor::new(data), 10);
        let mut buf = [0u8; 64];
        let n = r.read(&mut buf).unwrap();
        assert_eq!(n, 10);
        assert!(r.read(&mut buf).is_err());
    }
}
