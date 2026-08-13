//! Shared project/pattern archive serialization (Track E0 + Track F).
//!
//! - [`archive`] — zip create/open and named entry I/O
//! - [`assets`] — threshold-map content hashing, embed paths, synthetic materialize
//! - [`migrate`] — per-kind `format_version` gates (`dyproj` vs `dyuki`)
//! - [`pattern`] — `.dyuki` pack/unpack (placeholders, app_version_min)

pub mod archive;
pub mod assets;
pub mod document_dto;
pub mod id_remap;
pub mod migrate;
pub mod pattern;
pub mod pixels;
pub mod project;

pub use archive::{ArchiveError, ZipArchiveReader, ZipArchiveWriter};
pub use assets::{
    asset_cache_root, content_hash, materialize_threshold_map, materialize_threshold_map_with_hash,
    parse_threshold_basename, threshold_map_basename, threshold_map_zip_entry,
    threshold_maps_cache_dir, AssetsError, THRESHOLD_MAPS_PREFIX,
};
pub use document_dto::DocumentFile;
pub use id_remap::{remap_document_file, IdRemapTables, RemappedDocument};
pub use migrate::{
    check_format_version, migrate_dyproj, migrate_dyuki, ArchiveKind, Manifest, ProjectError,
    SOFT_SIZE_WARN_BYTES, SUPPORTED_DYPROJ_VERSION, SUPPORTED_DYUKI_VERSION,
};
pub use pixels::{assemble_layer_png, decode_png_to_f32, soft_size_warning};
pub use pattern::{
    check_app_version_min, export_pattern_from_document, import_pattern_into_document,
    min_app_version_for_filters, pack_pattern_to_bytes, unpack_pattern_from_bytes,
    write_pattern_to_path, ImportPatternResult, PalettePayload, PatternExportMeta,
    PatternFilterFile, PatternManifest, UnpackedPattern,
};
pub use project::{
    open_project_from_bytes, open_project_from_path, read_png_file, save_project_to_bytes,
    save_project_to_path, OpenProjectResult, SaveProjectResult,
};
