//! `ReadAt` over `IStream` (Seek + Read under a mutex).

use dither_zip_safe::io::{IoError, ReadAt};
use std::sync::Mutex;
use windows::core::Interface;
use windows::Win32::System::Com::{ISequentialStream, IStream, STREAM_SEEK_SET};

pub struct StreamReadAt {
    stream: Mutex<IStream>,
    size: u64,
    ops: u32,
    max_ops: u32,
    deadline: std::time::Instant,
}

impl StreamReadAt {
    pub fn new(stream: IStream, size: u64, max_ops: u32, deadline: std::time::Instant) -> Self {
        Self {
            stream: Mutex::new(stream),
            size,
            ops: 0,
            max_ops,
            deadline,
        }
    }
}

impl ReadAt for StreamReadAt {
    fn size(&self) -> u64 {
        self.size
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize, IoError> {
        if std::time::Instant::now() >= self.deadline {
            return Err(IoError::Timeout);
        }
        if self.ops >= self.max_ops {
            return Err(IoError::TooManyOps);
        }
        self.ops = self.ops.saturating_add(1);

        let guard = self
            .stream
            .lock()
            .map_err(|_| IoError::Io(std::io::Error::other("stream mutex poisoned")))?;

        // SAFETY: COM IStream methods on a valid interface pointer held by `guard`.
        unsafe {
            guard
                .Seek(offset as i64, STREAM_SEEK_SET, None)
                .map_err(|e| IoError::Io(std::io::Error::other(e.message())))?;
            let seq: ISequentialStream = guard
                .cast()
                .map_err(|e| IoError::Io(std::io::Error::other(e.message())))?;
            let mut read = 0u32;
            seq.Read(
                buf.as_mut_ptr() as *mut _,
                buf.len() as u32,
                Some(&mut read),
            )
            .ok()
            .map_err(|e| IoError::Io(std::io::Error::other(e.message())))?;
            Ok(read as usize)
        }
    }
}
