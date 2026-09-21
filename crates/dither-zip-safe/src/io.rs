//! Random-access IO abstraction for preview extractors (`DtIo.read_at`).

use std::io;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IoError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("short read at offset {offset}: got {got}, wanted {wanted}")]
    ShortRead { offset: u64, got: usize, wanted: usize },
    #[error("offset {offset} beyond size {size}")]
    OutOfRange { offset: u64, size: u64 },
    #[error("read_at operation budget exceeded")]
    TooManyOps,
    #[error("deadline exceeded")]
    Timeout,
}

/// Read `buf.len()` bytes at `offset`, or a short read reported via [`IoError::ShortRead`]
/// when `allow_short` is used by the caller after inspecting `out_read`.
pub trait ReadAt {
    fn size(&self) -> u64;
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize, IoError>;
}

/// In-memory `ReadAt` for tests and whole-file loaders.
pub struct MemoryReadAt<'a> {
    pub data: &'a [u8],
    pub ops: u32,
    pub max_ops: u32,
    pub deadline: Option<std::time::Instant>,
}

impl<'a> MemoryReadAt<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            ops: 0,
            max_ops: u32::MAX,
            deadline: None,
        }
    }

    pub fn with_budget(data: &'a [u8], max_ops: u32, deadline: Option<std::time::Instant>) -> Self {
        Self {
            data,
            ops: 0,
            max_ops,
            deadline,
        }
    }

    fn check_guards(&self) -> Result<(), IoError> {
        if self.ops >= self.max_ops {
            return Err(IoError::TooManyOps);
        }
        if let Some(dl) = self.deadline {
            if std::time::Instant::now() >= dl {
                return Err(IoError::Timeout);
            }
        }
        Ok(())
    }
}

impl ReadAt for MemoryReadAt<'_> {
    fn size(&self) -> u64 {
        self.data.len() as u64
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize, IoError> {
        self.check_guards()?;
        self.ops = self.ops.saturating_add(1);
        let size = self.data.len() as u64;
        if offset > size {
            return Err(IoError::OutOfRange { offset, size });
        }
        let start = offset as usize;
        let available = self.data.len().saturating_sub(start);
        let n = available.min(buf.len());
        buf[..n].copy_from_slice(&self.data[start..start + n]);
        Ok(n)
    }
}

/// Read exactly `buf.len()` bytes or error.
pub fn read_exact_at(io: &mut dyn ReadAt, offset: u64, buf: &mut [u8]) -> Result<(), IoError> {
    let n = io.read_at(offset, buf)?;
    if n != buf.len() {
        return Err(IoError::ShortRead {
            offset,
            got: n,
            wanted: buf.len(),
        });
    }
    Ok(())
}
