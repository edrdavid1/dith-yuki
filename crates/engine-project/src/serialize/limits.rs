//! Archive size / structure limits (SPEC §6.1).
//!
//! Values are starting points from the format spec; product may tune them later.
//! Tests may construct custom [`ArchiveLimits`].

/// Per-archive resource ceilings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchiveLimits {
    /// Maximum compressed archive byte length.
    pub max_archive_bytes: u64,
    /// Maximum number of zip entries (central directory).
    pub max_entries: u32,
    /// Cap on sum of actually decompressed entry payloads.
    pub max_total_uncompressed: u64,
    /// Cap for `manifest.json` uncompressed bytes.
    pub max_manifest_bytes: u64,
    /// Cap for primary JSON payloads (`document.json` / `filters.json` / `palettes.json`).
    pub max_json_payload_bytes: u64,
    /// Cap for a single PNG entry's uncompressed stream.
    pub max_png_entry_bytes: u64,
    /// Max `read_bytes / compressed_size` for non-PNG entries (integer ratio).
    pub max_compression_ratio: u64,
    /// Max `/`-separated path segments in an entry name.
    pub max_path_segments: u32,
    /// Max UTF-8 byte length of an entry name.
    pub max_entry_name_bytes: usize,
}

impl ArchiveLimits {
    /// Default `.dyproj` ceilings (SPEC §6.1).
    pub const fn dyproj() -> Self {
        Self {
            max_archive_bytes: 8 * GIB,
            max_entries: 20_000,
            max_total_uncompressed: 16 * GIB,
            max_manifest_bytes: MIB,
            max_json_payload_bytes: 64 * MIB,
            max_png_entry_bytes: GIB,
            max_compression_ratio: 200,
            max_path_segments: 4,
            max_entry_name_bytes: 200,
        }
    }

    /// Default `.dyuki` ceilings (SPEC §6.1).
    pub const fn dyuki() -> Self {
        Self {
            max_archive_bytes: 64 * MIB,
            max_entries: 2_000,
            max_total_uncompressed: 256 * MIB,
            max_manifest_bytes: MIB,
            max_json_payload_bytes: 16 * MIB,
            max_png_entry_bytes: 64 * MIB,
            max_compression_ratio: 200,
            max_path_segments: 4,
            max_entry_name_bytes: 200,
        }
    }

    /// Per-entry uncompressed budget by known name (fallback = png budget).
    pub fn max_uncompressed_for_entry(&self, name: &str) -> u64 {
        match name {
            "mimetype" => 128,
            "manifest.json" => self.max_manifest_bytes,
            "document.json" | "filters.json" | "palettes.json" => self.max_json_payload_bytes,
            _ if name.ends_with(".png") => self.max_png_entry_bytes,
            _ => self.max_png_entry_bytes.min(16 * MIB),
        }
    }
}

const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;
const GIB: u64 = 1024 * MIB;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dyuki_stricter_than_dyproj_on_archive_size() {
        assert!(ArchiveLimits::dyuki().max_archive_bytes < ArchiveLimits::dyproj().max_archive_bytes);
        assert!(ArchiveLimits::dyuki().max_entries < ArchiveLimits::dyproj().max_entries);
    }
}
