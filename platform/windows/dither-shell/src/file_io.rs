//! Random-access file IO for `dither-shell-diag render` (not the COM path).

use dither_zip_safe::io::{IoError, ReadAt};
use std::fs::File;
use std::path::Path;

pub struct FileAt {
    file: File,
    size: u64,
}

impl FileAt {
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let file = File::open(path)?;
        let size = file.metadata()?.len();
        Ok(Self { file, size })
    }
}

impl ReadAt for FileAt {
    fn size(&self) -> u64 {
        self.size
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize, IoError> {
        if offset > self.size {
            return Err(IoError::OutOfRange {
                offset,
                size: self.size,
            });
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileExt;
            Ok(self.file.read_at(buf, offset)?)
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileExt;
            Ok(self.file.seek_read(buf, offset)?)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (offset, buf);
            Err(IoError::Io(std::io::Error::other(
                "read_at is not implemented on this platform",
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dither_zip_safe::io::ReadAt;

    #[test]
    fn short_read_and_empty_file() {
        let dir = std::env::temp_dir();
        let empty = dir.join("dither-shell-empty-readat");
        let short = dir.join("dither-shell-short-readat");
        std::fs::write(&empty, b"").unwrap();
        std::fs::write(&short, b"abcd").unwrap();
        let empty_at = FileAt::open(&empty).unwrap();
        assert_eq!(empty_at.size(), 0);
        let mut short_at = FileAt::open(&short).unwrap();
        let mut buf = [0u8; 8];
        let n = short_at.read_at(0, &mut buf).unwrap();
        assert_eq!(n, 4);
        assert!(short_at.read_at(10, &mut buf).is_err());
        let _ = std::fs::remove_file(&empty);
        let _ = std::fs::remove_file(&short);
    }
}
