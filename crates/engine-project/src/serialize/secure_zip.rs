//! Secure in-memory ZIP loader (SPEC §6).
//!
//! Sole read path for `.dyproj` / `.dyuki`. Never extracts to the filesystem.
//! Declared header sizes are only used for early rejection; budgets count
//! **actually** read bytes via [`LimitedReader`].

use crate::serialize::limits::ArchiveLimits;
use std::collections::{HashMap, HashSet};
use std::io::{self, Cursor, Read};
use thiserror::Error;
use zip::read::ZipFile;
use zip::{CompressionMethod, ZipArchive};

/// Which archive flavour the caller expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedKind {
    Dyproj,
    Dyuki,
}

impl ExpectedKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ExpectedKind::Dyproj => "dyproj",
            ExpectedKind::Dyuki => "dyuki",
        }
    }

    pub fn mimetype(self) -> &'static str {
        match self {
            ExpectedKind::Dyproj => "application/vnd.dither.project+zip",
            ExpectedKind::Dyuki => "application/vnd.dither.pattern+zip",
        }
    }

    pub fn mimetype_legacy(self) -> &'static str {
        match self {
            ExpectedKind::Dyproj => "application/x-dither-project",
            ExpectedKind::Dyuki => "application/x-dither-pattern",
        }
    }

    pub fn accepts_mimetype(self, value: &[u8]) -> bool {
        value == self.mimetype().as_bytes() || value == self.mimetype_legacy().as_bytes()
    }
}

/// Errors from the secure ZIP loader (typed for UI mapping later).
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SecureZipError {
    #[error("archive exceeds size limit ({size} > {limit})")]
    ArchiveTooLarge { size: u64, limit: u64 },

    #[error("too many zip entries ({count} > {limit})")]
    TooManyEntries { count: u32, limit: u32 },

    #[error("invalid zip: {0}")]
    InvalidArchive(String),

    #[error("unsafe entry name: {0}")]
    UnsafeEntryName(String),

    #[error("duplicate entry name (case-insensitive): {0}")]
    DuplicateEntry(String),

    #[error("unsupported entry type or compression for {name}: {reason}")]
    UnsupportedEntry { name: String, reason: String },

    #[error("entry {name} exceeds uncompressed limit ({read} > {limit})")]
    EntryTooLarge { name: String, read: u64, limit: u64 },

    #[error("archive uncompressed total exceeds limit ({read} > {limit})")]
    TotalUncompressedTooLarge { read: u64, limit: u64 },

    #[error("suspicious compression ratio for {name}")]
    CompressionBomb { name: String },

    #[error("entry not found: {0}")]
    EntryNotFound(String),

    #[error("archive kind mismatch: expected {expected}")]
    KindMismatch { expected: String },

    #[error("corrupt archive: {0}")]
    Corrupt(String),
}

/// Validated zip held entirely in memory.
pub struct SecureZipArchive {
    inner: ZipArchive<Cursor<Vec<u8>>>,
    /// Canonical (as stored) names that passed allowlist semantics.
    allowed_names: HashSet<String>,
    /// Lowercased → canonical name for case-insensitive lookup.
    by_lower: HashMap<String, String>,
    limits: ArchiveLimits,
    total_read: u64,
    /// Soft notices (ignored unknown entries, missing legacy mimetype, …).
    pub warnings: Vec<String>,
}

impl std::fmt::Debug for SecureZipArchive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecureZipArchive")
            .field("entries", &self.allowed_names.len())
            .field("total_read", &self.total_read)
            .field("warnings", &self.warnings)
            .finish_non_exhaustive()
    }
}

impl SecureZipArchive {
    /// Open and validate central-directory metadata without fully decompressing.
    pub fn open(
        bytes: &[u8],
        expected: ExpectedKind,
        limits: ArchiveLimits,
    ) -> Result<Self, SecureZipError> {
        let size = bytes.len() as u64;
        if size > limits.max_archive_bytes {
            return Err(SecureZipError::ArchiveTooLarge {
                size,
                limit: limits.max_archive_bytes,
            });
        }

        let mut inner = ZipArchive::new(Cursor::new(bytes.to_vec()))
            .map_err(|e| SecureZipError::InvalidArchive(e.to_string()))?;

        let count = inner.len() as u32;
        if count > limits.max_entries {
            return Err(SecureZipError::TooManyEntries {
                count,
                limit: limits.max_entries,
            });
        }

        let mut allowed_names = HashSet::new();
        let mut by_lower = HashMap::new();
        let mut seen_lower = HashSet::new();
        let mut warnings = Vec::new();
        let mut first_name: Option<String> = None;

        // Index 0 is the first local-file order as exposed by the zip crate
        // (central directory order). Used for optional mimetype check.
        for i in 0..inner.len() {
            let file = inner
                .by_index(i)
                .map_err(|e| SecureZipError::InvalidArchive(e.to_string()))?;
            let raw_name = file.name().to_string();
            if first_name.is_none() {
                first_name = Some(raw_name.clone());
            }

            validate_entry_syntax(&raw_name, &limits)?;
            validate_entry_type(&file, &raw_name)?;

            let lower = raw_name.to_ascii_lowercase();
            if !seen_lower.insert(lower.clone()) {
                return Err(SecureZipError::DuplicateEntry(raw_name));
            }

            match classify_entry_name(&raw_name) {
                EntryClass::Allowed => {
                    by_lower.insert(lower, raw_name.clone());
                    allowed_names.insert(raw_name);
                }
                EntryClass::Unknown => {
                    // SPEC §6.2: ignore unknown *safe* names (no required features yet).
                    warnings.push(format!("ignoring unknown entry '{raw_name}'"));
                }
            }
        }

        let mut archive = Self {
            inner,
            allowed_names,
            by_lower,
            limits,
            total_read: 0,
            warnings,
        };

        archive.check_optional_mimetype(expected, first_name.as_deref())?;
        Ok(archive)
    }

    fn check_optional_mimetype(
        &mut self,
        expected: ExpectedKind,
        first_name: Option<&str>,
    ) -> Result<(), SecureZipError> {
        if !self.allowed_names.contains("mimetype") {
            // Stage 4 will require mimetype; v1 goldens omit it.
            self.warnings
                .push("legacy archive without mimetype entry".into());
            return Ok(());
        }
        if first_name != Some("mimetype") {
            return Err(SecureZipError::Corrupt(
                "mimetype must be the first zip entry".into(),
            ));
        }
        let data = self.read_entry("mimetype")?;
        // SPEC: mimetype entry must be Stored (uncompressed).
        {
            let file = self
                .inner
                .by_name("mimetype")
                .map_err(|_| SecureZipError::EntryNotFound("mimetype".into()))?;
            if file.compression() != CompressionMethod::Stored {
                return Err(SecureZipError::Corrupt(
                    "mimetype entry must use Stored compression".into(),
                ));
            }
        }
        if !expected.accepts_mimetype(&data) {
            return Err(SecureZipError::KindMismatch {
                expected: expected.as_str().into(),
            });
        }
        Ok(())
    }

    /// Read an allowlisted entry with decompression budgets.
    pub fn read_entry(&mut self, name: &str) -> Result<Vec<u8>, SecureZipError> {
        let canonical = self
            .by_lower
            .get(&name.to_ascii_lowercase())
            .cloned()
            .ok_or_else(|| SecureZipError::EntryNotFound(name.to_string()))?;
        if !self.allowed_names.contains(&canonical) {
            return Err(SecureZipError::EntryNotFound(name.to_string()));
        }

        let entry_limit = self.limits.max_uncompressed_for_entry(&canonical);
        let mut file = self
            .inner
            .by_name(&canonical)
            .map_err(|_| SecureZipError::EntryNotFound(canonical.clone()))?;

        let declared = file.size();
        if declared > entry_limit {
            return Err(SecureZipError::EntryTooLarge {
                name: canonical,
                read: declared,
                limit: entry_limit,
            });
        }

        let compressed = file.compressed_size().max(1);
        let remaining_total = self
            .limits
            .max_total_uncompressed
            .saturating_sub(self.total_read);
        let budget = entry_limit.min(remaining_total);

        let mut limited = LimitedReader::new(&mut file, budget);
        let mut buf = Vec::new();
        // Do not pre-reserve from untrusted declared size beyond budget.
        let reserve = (declared as usize)
            .min(budget as usize)
            .min(16 * 1024 * 1024);
        buf.try_reserve_exact(reserve)
            .map_err(|_| SecureZipError::EntryTooLarge {
                name: canonical.clone(),
                read: declared,
                limit: budget,
            })?;

        match limited.read_to_end(&mut buf) {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                return Err(SecureZipError::EntryTooLarge {
                    name: canonical,
                    read: budget.saturating_add(1),
                    limit: budget,
                });
            }
            Err(e) => {
                return Err(SecureZipError::InvalidArchive(format!("{canonical}: {e}")));
            }
        }

        let read_n = buf.len() as u64;
        if !canonical.ends_with(".png") {
            let ratio = read_n / compressed;
            if ratio > self.limits.max_compression_ratio {
                return Err(SecureZipError::CompressionBomb { name: canonical });
            }
        }

        self.total_read = self.total_read.saturating_add(read_n);
        if self.total_read > self.limits.max_total_uncompressed {
            return Err(SecureZipError::TotalUncompressedTooLarge {
                read: self.total_read,
                limit: self.limits.max_total_uncompressed,
            });
        }

        Ok(buf)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.by_lower
            .get(&name.to_ascii_lowercase())
            .is_some_and(|c| self.allowed_names.contains(c))
    }

    pub fn entry_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.allowed_names.iter().cloned().collect();
        names.sort();
        names
    }

    /// Names under `ext/` (opaque forward-compat blobs).
    pub fn ext_entry_names(&self) -> Vec<String> {
        self.allowed_names
            .iter()
            .filter(|n| n.starts_with("ext/"))
            .cloned()
            .collect()
    }
}

/// Counts bytes actually produced by the inner reader.
struct LimitedReader<'a, R: Read> {
    inner: &'a mut R,
    remaining: u64,
}

impl<'a, R: Read> LimitedReader<'a, R> {
    fn new(inner: &'a mut R, budget: u64) -> Self {
        Self {
            inner,
            remaining: budget,
        }
    }
}

impl<R: Read> Read for LimitedReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            // Peek whether more data exists — if so, bomb.
            let mut probe = [0u8; 1];
            return match self.inner.read(&mut probe)? {
                0 => Ok(0),
                _ => Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "entry exceeded uncompressed budget",
                )),
            };
        }
        let max = (self.remaining as usize).min(buf.len());
        let n = self.inner.read(&mut buf[..max])?;
        self.remaining = self.remaining.saturating_sub(n as u64);
        Ok(n)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryClass {
    Allowed,
    Unknown,
}

/// Syntax checks for every entry (Zip Slip / Windows / NUL). Unsafe → hard error.
pub fn validate_entry_syntax(name: &str, limits: &ArchiveLimits) -> Result<(), SecureZipError> {
    if name.len() > limits.max_entry_name_bytes {
        return Err(SecureZipError::UnsafeEntryName(name.into()));
    }
    if name.is_empty() {
        return Err(SecureZipError::UnsafeEntryName("(empty)".into()));
    }
    if !name.is_ascii() {
        // SPEC: UTF-8 allowed, but our allowlist is ASCII-only; non-ASCII names
        // that are otherwise safe become Unknown via classify — still must pass
        // syntax. Reject NUL / controls here; non-ASCII without controls is OK
        // at syntax stage then ignored as Unknown if not allowlisted.
        if name.chars().any(|c| c.is_control() || c == '\0') {
            return Err(SecureZipError::UnsafeEntryName(name.into()));
        }
    } else if name.bytes().any(|b| b == 0 || b < 0x20 || b == 0x7f) {
        return Err(SecureZipError::UnsafeEntryName(name.into()));
    }

    if name.contains('\\') || name.starts_with('/') || name.contains(':') || name.contains("//") {
        return Err(SecureZipError::UnsafeEntryName(name.into()));
    }

    let segments: Vec<&str> = name.split('/').collect();
    if segments.len() as u32 > limits.max_path_segments {
        return Err(SecureZipError::UnsafeEntryName(name.into()));
    }
    for seg in &segments {
        if seg.is_empty() || *seg == "." || *seg == ".." {
            return Err(SecureZipError::UnsafeEntryName(name.into()));
        }
        if seg.ends_with(' ') || seg.ends_with('.') {
            return Err(SecureZipError::UnsafeEntryName(name.into()));
        }
    }

    // NFC must not change the name (reject combining-char tricks / unstable forms).
    let nfc = name.chars().collect::<String>(); // already NFC for pure ASCII; for
                                                // non-ASCII use unicode-normalization
                                                // when we add that dep — Stage 1 ASCII allowlist.
    if name.is_ascii() && nfc != *name {
        return Err(SecureZipError::UnsafeEntryName(name.into()));
    }

    Ok(())
}

fn classify_entry_name(name: &str) -> EntryClass {
    if is_allowlisted_entry(name) {
        EntryClass::Allowed
    } else {
        EntryClass::Unknown
    }
}

/// Semantic allowlist (SPEC §6.2 + Stage-0 decision: threshold hash is 32 hex BLAKE3).
pub fn is_allowlisted_entry(name: &str) -> bool {
    matches!(
        name,
        "mimetype"
            | "manifest.json"
            | "document.json"
            | "filters.json"
            | "palettes.json"
            | "composite.png"
            | "thumbnail.png"
    ) || is_layer_png(name)
        || is_threshold_map(name)
        || is_ext_entry(name)
}

fn is_layer_png(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("layers/") else {
        return false;
    };
    let Some(stem) = rest.strip_suffix(".png") else {
        return false;
    };
    is_id_token(stem)
}

fn is_threshold_map(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("assets/threshold_maps/") else {
        return false;
    };
    let Some(stem) = rest.strip_suffix(".png") else {
        return false;
    };
    // Current on-disk hash = 32 lowercase hex (BLAKE3 truncated). SPEC's future
    // sha256 (64 hex) is also accepted so Stage 4 can switch without loader churn.
    let len = stem.len();
    (len == 32 || len == 64) && stem.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn is_ext_entry(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("ext/") else {
        return false;
    };
    let segments: Vec<&str> = rest.split('/').collect();
    if segments.is_empty() || segments.len() > 3 {
        return false;
    }
    segments.iter().all(|s| is_ext_token(s))
}

fn is_id_token(s: &str) -> bool {
    let len = s.len();
    (1..=64).contains(&len)
        && s.bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-'))
}

fn is_ext_token(s: &str) -> bool {
    let len = s.len();
    (1..=64).contains(&len)
        && s.bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-'))
}

fn validate_entry_type(file: &ZipFile<'_>, name: &str) -> Result<(), SecureZipError> {
    if file.is_dir() {
        return Err(SecureZipError::UnsupportedEntry {
            name: name.into(),
            reason: "directory entries are not allowed".into(),
        });
    }
    if file.encrypted() {
        return Err(SecureZipError::UnsupportedEntry {
            name: name.into(),
            reason: "encrypted entries are not allowed".into(),
        });
    }
    if let Some(mode) = file.unix_mode() {
        const S_IFMT: u32 = 0o170000;
        const S_IFLNK: u32 = 0o120000;
        const S_IFREG: u32 = 0o100000;
        let ft = mode & S_IFMT;
        if ft == S_IFLNK {
            return Err(SecureZipError::UnsupportedEntry {
                name: name.into(),
                reason: "symlink entries are not allowed".into(),
            });
        }
        if ft != 0 && ft != S_IFREG {
            return Err(SecureZipError::UnsupportedEntry {
                name: name.into(),
                reason: format!("non-regular unix mode {mode:#o}"),
            });
        }
    }
    match file.compression() {
        CompressionMethod::Stored | CompressionMethod::Deflated => Ok(()),
        other => Err(SecureZipError::UnsupportedEntry {
            name: name.into(),
            reason: format!("compression {other:?}"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serialize::archive::create_zip;

    fn open_ok(bytes: &[u8]) -> SecureZipArchive {
        SecureZipArchive::open(bytes, ExpectedKind::Dyproj, ArchiveLimits::dyproj()).unwrap()
    }

    #[test]
    fn rejects_zip_slip_dotdot() {
        // Crafting `../x` via zip crate still stores the name; open must reject.
        let zip = create_zip(&[("../x", b"nope")]).unwrap();
        let err = SecureZipArchive::open(&zip, ExpectedKind::Dyproj, ArchiveLimits::dyproj())
            .unwrap_err();
        assert!(matches!(err, SecureZipError::UnsafeEntryName(_)), "{err:?}");
    }

    #[test]
    fn rejects_absolute_and_backslash() {
        for name in ["/etc/passwd", "..\\x", "C:/x", "a/../../b"] {
            let zip = create_zip(&[(name, b"x")]).unwrap();
            let err = SecureZipArchive::open(&zip, ExpectedKind::Dyproj, ArchiveLimits::dyproj())
                .unwrap_err();
            assert!(
                matches!(err, SecureZipError::UnsafeEntryName(_)),
                "{name} → {err:?}"
            );
        }
    }

    #[test]
    fn rejects_case_collision() {
        let zip = create_zip(&[("manifest.json", b"{}"), ("Manifest.json", b"{}")]).unwrap();
        let err = SecureZipArchive::open(&zip, ExpectedKind::Dyproj, ArchiveLimits::dyproj())
            .unwrap_err();
        assert!(matches!(err, SecureZipError::DuplicateEntry(_)), "{err:?}");
    }

    #[test]
    fn reads_allowlisted_round_trip() {
        let zip = create_zip(&[
            ("manifest.json", br#"{"format_version":1}"#),
            ("document.json", b"{}"),
            ("layers/1.png", b"\x89PNG"),
        ])
        .unwrap();
        let mut ar = open_ok(&zip);
        assert_eq!(
            ar.read_entry("manifest.json").unwrap(),
            br#"{"format_version":1}"#
        );
        assert!(ar.contains("layers/1.png"));
    }

    #[test]
    fn ignores_unknown_safe_entry() {
        let zip = create_zip(&[("manifest.json", b"{}"), ("readme.txt", b"hi")]).unwrap();
        let ar = open_ok(&zip);
        assert!(ar.warnings.iter().any(|w| w.contains("readme.txt")));
        assert!(!ar.contains("readme.txt"));
    }

    #[test]
    fn allowlist_threshold_32_and_64_hex() {
        assert!(is_threshold_map(
            "assets/threshold_maps/0123456789abcdef0123456789abcdef.png"
        ));
        assert!(is_threshold_map(
            "assets/threshold_maps/0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef.png"
        ));
        assert!(!is_threshold_map("assets/threshold_maps/xyz.png"));
    }
}
