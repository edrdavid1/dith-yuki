//! Zip container helpers shared by `.dyproj` and `.dyuki`.

use std::io::{Cursor, Read, Write};
use thiserror::Error;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

/// Errors from zip archive I/O.
#[derive(Debug, Error)]
pub enum ArchiveError {
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("entry not found: {0}")]
    EntryNotFound(String),
}

/// In-memory zip writer that accumulates named byte entries.
pub struct ZipArchiveWriter {
    inner: ZipWriter<Cursor<Vec<u8>>>,
}

impl ZipArchiveWriter {
    pub fn new() -> Self {
        Self {
            inner: ZipWriter::new(Cursor::new(Vec::new())),
        }
    }

    /// Write (or overwrite-by-recreate) a named entry with raw bytes.
    pub fn write_entry(&mut self, name: &str, data: &[u8]) -> Result<(), ArchiveError> {
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        self.inner.start_file(name, options)?;
        self.inner.write_all(data)?;
        Ok(())
    }

    /// Finish the archive and return the zip bytes.
    pub fn finish(self) -> Result<Vec<u8>, ArchiveError> {
        let cursor = self.inner.finish()?;
        Ok(cursor.into_inner())
    }
}

impl Default for ZipArchiveWriter {
    fn default() -> Self {
        Self::new()
    }
}

/// Read named entries from zip bytes.
pub struct ZipArchiveReader {
    inner: ZipArchive<Cursor<Vec<u8>>>,
}

impl ZipArchiveReader {
    pub fn open(bytes: &[u8]) -> Result<Self, ArchiveError> {
        let inner = ZipArchive::new(Cursor::new(bytes.to_vec()))?;
        Ok(Self { inner })
    }

    /// Read a named entry into a byte vector.
    pub fn read_entry(&mut self, name: &str) -> Result<Vec<u8>, ArchiveError> {
        let mut file = self
            .inner
            .by_name(name)
            .map_err(|_| ArchiveError::EntryNotFound(name.to_string()))?;
        let mut buf = Vec::with_capacity(file.size() as usize);
        file.read_to_end(&mut buf)?;
        Ok(buf)
    }

    /// True if an entry with this exact name exists.
    pub fn contains(&mut self, name: &str) -> bool {
        self.inner.by_name(name).is_ok()
    }

    /// Names of all entries in the archive.
    pub fn entry_names(&self) -> Vec<String> {
        self.inner.file_names().map(|s| s.to_string()).collect()
    }
}

/// Create a zip from `(name, bytes)` pairs.
pub fn create_zip(entries: &[(&str, &[u8])]) -> Result<Vec<u8>, ArchiveError> {
    let mut writer = ZipArchiveWriter::new();
    for (name, data) in entries {
        writer.write_entry(name, data)?;
    }
    writer.finish()
}

/// Open zip bytes and read one named entry.
pub fn read_zip_entry(zip_bytes: &[u8], name: &str) -> Result<Vec<u8>, ArchiveError> {
    let mut reader = ZipArchiveReader::open(zip_bytes)?;
    reader.read_entry(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zip_round_trip_bytes() {
        let payload = b"hello dyproj assets";
        let zip = create_zip(&[
            ("manifest.json", br#"{"format_version":1}"#),
            ("assets/threshold_maps/abc.png", payload),
        ])
        .expect("create zip");

        let mut reader = ZipArchiveReader::open(&zip).expect("open zip");
        let got = reader
            .read_entry("assets/threshold_maps/abc.png")
            .expect("read entry");
        assert_eq!(got, payload);

        let manifest = reader.read_entry("manifest.json").unwrap();
        assert!(manifest.starts_with(b"{"));
    }

    #[test]
    fn missing_entry_errors() {
        let zip = create_zip(&[("a.txt", b"x")]).unwrap();
        let err = read_zip_entry(&zip, "missing.txt").unwrap_err();
        assert!(matches!(err, ArchiveError::EntryNotFound(_)));
    }
}
