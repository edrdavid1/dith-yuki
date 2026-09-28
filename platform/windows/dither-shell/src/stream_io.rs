//! `ReadAt` over `IStream` (Seek + Read under a mutex).

use crate::diag_codes::{record, Reason};
use dither_zip_safe::io::{IoError, ReadAt};
use std::sync::Mutex;
use windows::core::{Error, Interface, Ref};
use windows::Win32::Foundation::{ERROR_ALREADY_INITIALIZED, E_INVALIDARG};
use windows::Win32::System::Com::{
    ISequentialStream, IStream, STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET,
};

pub struct BoundStream {
    pub stream: Option<IStream>,
    pub size: u64,
}

impl BoundStream {
    pub fn new() -> Self {
        Self {
            stream: None,
            size: 0,
        }
    }

    pub fn initialize(&mut self, pstream: Ref<'_, IStream>) -> windows::core::Result<()> {
        if self.stream.is_some() {
            record(Reason::AlreadyInitialized);
            return Err(Error::from(ERROR_ALREADY_INITIALIZED));
        }
        let stream = pstream.ok().map_err(|_| Error::from(E_INVALIDARG))?.clone();
        let size = unsafe {
            let mut stat = STATSTG::default();
            stream.Stat(&mut stat, STATFLAG_NONAME)?;
            stat.cbSize
        };
        if size == 0 {
            record(Reason::InvalidArg);
            return Err(Error::from(E_INVALIDARG));
        }
        self.stream = Some(stream);
        self.size = size;
        Ok(())
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::SHCreateMemStream;

    #[test]
    fn memory_stream_short_read() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let stream = unsafe { SHCreateMemStream(Some(&[9, 8, 7, 6])) }.expect("stream");
        let mut io = StreamReadAt::new(stream, 4, 8, Instant::now() + Duration::from_secs(2));
        let mut buf = [0u8; 8];
        assert_eq!(io.read_at(0, &mut buf).unwrap(), 4);
        assert_eq!(&buf[..4], &[9, 8, 7, 6]);
        assert_eq!(io.size(), 4);
    }
}
