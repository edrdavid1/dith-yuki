//! Archive / preview resource ceilings.

/// Per-archive ceilings shared with the document loader (SPEC format §6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchiveLimits {
    pub max_archive_bytes: u64,
    pub max_entries: u32,
    pub max_total_uncompressed: u64,
    pub max_manifest_bytes: u64,
    pub max_json_payload_bytes: u64,
    pub max_png_entry_bytes: u64,
    pub max_compression_ratio: u64,
    pub max_path_segments: u32,
    pub max_entry_name_bytes: usize,
}

impl ArchiveLimits {
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

/// Limits for the OS preview extractor (preview SPEC §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThumbLimits {
    pub max_archive_dyproj: u64,
    pub max_archive_dyuki: u64,
    pub max_entries_dyproj: u32,
    pub max_entries_dyuki: u32,
    pub max_central_directory: u64,
    pub max_mimetype: u64,
    pub max_thumbnail_compressed: u64,
    pub max_thumbnail_uncompressed: u64,
    pub max_png_side: u32,
    pub max_png_pixels: u64,
    pub deadline_ms: u64,
    pub max_peak_memory: u64,
    pub max_read_at_ops: u32,
}

impl Default for ThumbLimits {
    fn default() -> Self {
        Self {
            max_archive_dyproj: 8 * GIB,
            max_archive_dyuki: 64 * MIB,
            max_entries_dyproj: 20_000,
            max_entries_dyuki: 2_000,
            max_central_directory: 8 * MIB,
            max_mimetype: 128,
            max_thumbnail_compressed: 4 * MIB,
            max_thumbnail_uncompressed: 4 * MIB,
            max_png_side: 2048,
            max_png_pixels: 4_194_304,
            deadline_ms: 2_000,
            max_peak_memory: 64 * MIB,
            max_read_at_ops: 10_000,
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
    fn dyuki_stricter_than_dyproj() {
        assert!(ArchiveLimits::dyuki().max_archive_bytes < ArchiveLimits::dyproj().max_archive_bytes);
    }
}
