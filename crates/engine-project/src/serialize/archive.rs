//! Zip container helpers shared by `.dyproj` and `.dyuki`.
//!
//! Write path is **deterministic** (SPEC §4 / Stage 4): fixed entry order,
//! fixed DOS mtime, fixed unix mode, PNG Stored / JSON Deflated, ZIP64 flags.

use std::io::{Cursor, Read, Write};
use thiserror::Error;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

/// Canonical MIME for `.dyproj` (SPEC §6.5 / §12).
pub const MIME_DYPROJ: &str = "application/vnd.dither.project+zip";
/// Canonical MIME for `.dyuki`.
pub const MIME_DYUKI: &str = "application/vnd.dither.pattern+zip";
/// Legacy alias accepted on read.
pub const MIME_DYPROJ_LEGACY: &str = "application/x-dither-project";
/// Legacy alias accepted on read.
pub const MIME_DYUKI_LEGACY: &str = "application/x-dither-pattern";

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

    /// Write a named entry with SPEC Stage-4 compression / attribute rules.
    pub fn write_entry(&mut self, name: &str, data: &[u8]) -> Result<(), ArchiveError> {
        let options = file_options_for(name);
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

fn fixed_mtime() -> DateTime {
    // SPEC: 1980-01-01 00:00:00 (DOS epoch lower bound).
    DateTime::from_date_and_time(1980, 1, 1, 0, 0, 0).expect("fixed mtime in range")
}

fn file_options_for(name: &str) -> SimpleFileOptions {
    let method = if name == "mimetype" || name.ends_with(".png") {
        CompressionMethod::Stored
    } else {
        CompressionMethod::Deflated
    };
    SimpleFileOptions::default()
        .compression_method(method)
        .last_modified_time(fixed_mtime())
        .unix_permissions(0o100644)
        .large_file(true)
}

/// Order entries for a Dither archive: `mimetype` (caller), then `manifest.json`,
/// then remaining names sorted lexicographically.
fn order_payload_entries(entries: &mut Vec<(String, Vec<u8>)>) {
    entries.retain(|(n, _)| n != "mimetype");
    entries.sort_by(|a, b| match (a.0.as_str(), b.0.as_str()) {
        ("manifest.json", "manifest.json") => std::cmp::Ordering::Equal,
        ("manifest.json", _) => std::cmp::Ordering::Less,
        (_, "manifest.json") => std::cmp::Ordering::Greater,
        (a, b) => a.cmp(b),
    });
}

/// Create a deterministic zip from `(name, bytes)` pairs (no `mimetype` required).
///
/// Entry order: `manifest.json` first if present, then lexicographic. Used by
/// tests and legacy helpers; production saves use [`create_dither_archive`].
pub fn create_zip(entries: &[(&str, &[u8])]) -> Result<Vec<u8>, ArchiveError> {
    let mut owned: Vec<(String, Vec<u8>)> = entries
        .iter()
        .map(|(n, b)| (n.to_string(), b.to_vec()))
        .collect();
    order_payload_entries(&mut owned);
    let mut writer = ZipArchiveWriter::new();
    for (name, data) in &owned {
        writer.write_entry(name, data)?;
    }
    writer.finish()
}

/// Create a Dither archive with `mimetype` as the first Stored entry.
pub fn create_dither_archive(
    mimetype: &str,
    entries: &[(&str, &[u8])],
) -> Result<Vec<u8>, ArchiveError> {
    let mut owned: Vec<(String, Vec<u8>)> = entries
        .iter()
        .map(|(n, b)| (n.to_string(), b.to_vec()))
        .collect();
    order_payload_entries(&mut owned);

    let mut writer = ZipArchiveWriter::new();
    writer.write_entry("mimetype", mimetype.as_bytes())?;
    for (name, data) in &owned {
        writer.write_entry(name, data)?;
    }
    writer.finish()
}

/// Peek the first zip entry as `mimetype` and return its ASCII payload.
pub fn peek_mimetype(zip_bytes: &[u8]) -> Result<Option<String>, ArchiveError> {
    let mut archive = ZipArchive::new(Cursor::new(zip_bytes.to_vec()))?;
    if archive.is_empty() {
        return Ok(None);
    }
    let mut file = archive.by_index(0)?;
    if file.name() != "mimetype" {
        return Ok(None);
    }
    let mut buf = Vec::with_capacity(file.size() as usize);
    file.read_to_end(&mut buf)?;
    Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
}

/// Detect archive kind from `mimetype` (canonical or legacy).
pub fn detect_archive_kind_from_bytes(
    zip_bytes: &[u8],
) -> Option<crate::serialize::migrate::ArchiveKind> {
    let mime = peek_mimetype(zip_bytes).ok().flatten()?;
    match mime.as_str() {
        MIME_DYPROJ | MIME_DYPROJ_LEGACY => Some(crate::serialize::migrate::ArchiveKind::Dyproj),
        MIME_DYUKI | MIME_DYUKI_LEGACY => Some(crate::serialize::migrate::ArchiveKind::Dyuki),
        _ => None,
    }
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

    #[test]
    fn dither_archive_mimetype_is_first_and_deterministic() {
        let a = create_dither_archive(
            MIME_DYPROJ,
            &[
                ("layers/2.png", b"\x89PNG"),
                ("document.json", b"{}"),
                ("manifest.json", b"{\"k\":1}"),
            ],
        )
        .unwrap();
        let b = create_dither_archive(
            MIME_DYPROJ,
            &[
                ("manifest.json", b"{\"k\":1}"),
                ("document.json", b"{}"),
                ("layers/2.png", b"\x89PNG"),
            ],
        )
        .unwrap();
        assert_eq!(a, b);

        let mut archive = ZipArchive::new(Cursor::new(a.clone())).unwrap();
        let first = archive.by_index(0).unwrap();
        assert_eq!(first.name(), "mimetype");
        assert_eq!(first.compression(), CompressionMethod::Stored);
        drop(first);

        let mime = peek_mimetype(&a).unwrap().unwrap();
        assert_eq!(mime, MIME_DYPROJ);
        assert_eq!(
            detect_archive_kind_from_bytes(&a),
            Some(crate::serialize::migrate::ArchiveKind::Dyproj)
        );
    }
}
